//! The API of the RTLola interpreter. It is used to evaluate input events on a given specification and output the results.
//! The main structure is the [Monitor] which is parameterized over its input and output method.
//! The preferred method to create a [Monitor] is using the [ConfigBuilder](crate::ConfigBuilder) and the [monitor](crate::ConfigBuilder::monitor) method.
//!
//! # Input Method
//! An input method has to implement the [Input] trait. Out of the box two different methods are provided:
//! * [EventInput]: Provides a basic input method for anything that already is an [Event] or that can be transformed into one using `Into<Event>`.
//! * [RecordInput]: Is a more elaborate input method. It allows to provide a custom data structure to the monitor as an input, as long as it implements the [Record] trait.
//!     If implemented this traits provides functionality to generate a new value for any input stream from the data structure.
//!
//! # Output Method
//! The [Monitor] can provide output with a varying level of detail captured by the [VerdictRepresentation] trait. The different output formats are:
//! * [Incremental]: For each processed event a condensed list of monitor state changes is provided.
//! * [Total]: For each event a complete snapshot of the current monitor state is returned
//! * [TriggerMessages]: For each event a list of violated triggers with their description is produced.
//! * [TriggersWithInfoValues]: For each event a list of violated triggers with their specified corresponding values is returned.

use crate::basics::OutputHandler;
use crate::config::{Config, ExecutionMode, TimeRepresentation};
use crate::coordination::time_driven_manager::TimeDrivenManager;
use crate::coordination::DynamicSchedule;
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::storage::Value;
use crate::Time;
use itertools::Itertools;
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, Type};
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
/// An event to be handled by the interpreter
pub type Event = Vec<Value>;

/**
    Provides the functionality to generate a snapshot of the streams values.
*/
pub trait VerdictRepresentation {
    /// Creates a snapshot of the streams values.
    fn create(data: RawVerdict) -> Self
    where
        Self: Sized;
}

/// A type representing the parameters of a stream.
/// If a stream is not dynamically created it defaults to `None`.
/// If a stream is dynamically created but does not have parameters it defaults to `Some(vec![])`
pub type Parameters = Option<Vec<Value>>;

/// A stream instance. First element represents the parameter values of the instance, the second element the value of the instance.
pub type Instance = (Parameters, Option<Value>);

/// An enum representing a change in the monitor.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Change {
    /// Indicates that a new instance of a stream was created with the given values as parameters.
    Spawn(Vec<Value>),
    /// Indicates that an instance got a new value. The instance is identified through the given [Parameters].
    Value(Parameters, Value),
    /// Indicates that an instance was closed. The given values are the parameters of the closed instance.
    Close(Vec<Value>),
}

impl Display for Change {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Change::Spawn(para) => write!(f, "Spawn<{}>", para.iter().join(", ")),
            Change::Close(para) => write!(f, "Close<{}>", para.iter().join(", ")),
            Change::Value(para, value) => match para {
                Some(para) => write!(f, "Instance<{}> = {}", para.iter().join(", "), value),
                None => write!(f, "Value = {}", value),
            },
        }
    }
}

/**
    Represents the changes of the monitor state. Each element represents a set of [Change]s of a specific output stream.
*/
pub type Incremental = Vec<(OutputReference, Vec<Change>)>;

impl VerdictRepresentation for Incremental {
    fn create(data: RawVerdict) -> Self {
        data.eval.peek_fresh()
    }
}

/**
    Represents a snapshot of the monitor state containing the current value of each output and input stream.
*/
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Total {
    /// The ith value in this vector is the current value of the ith input stream.
    pub inputs: Vec<Option<Value>>,

    /// The ith value in this vector is the vector of instances of the ith output stream.
    /// If the stream has no instance yet, this vector is empty. If a stream is not parameterized, the vector will always be of size 1.
    pub outputs: Vec<Vec<Instance>>,
}

impl VerdictRepresentation for Total {
    fn create(data: RawVerdict) -> Self {
        Total { inputs: data.eval.peek_inputs(), outputs: data.eval.peek_outputs() }
    }
}

/**
    Represents the index and the formatted message of all violated triggers.
*/
pub type TriggerMessages = Vec<(OutputReference, String)>;

impl VerdictRepresentation for TriggerMessages {
    fn create(data: RawVerdict) -> Self
    where
        Self: Sized,
    {
        let violated_trigger = data.eval.peek_violated_triggers();
        violated_trigger.into_iter().map(|sr| (sr, data.eval.format_trigger_message(sr))).collect()
    }
}

/**
    Represents the index and the info values of all violated triggers.
*/
pub type TriggersWithInfoValues = Vec<(OutputReference, Vec<Option<Value>>)>;

impl VerdictRepresentation for TriggersWithInfoValues {
    fn create(data: RawVerdict) -> Self
    where
        Self: Sized,
    {
        let violated_trigger = data.eval.peek_violated_triggers();
        violated_trigger.into_iter().map(|sr| (sr, data.eval.peek_info_stream_values(sr))).collect()
    }
}

/**
    The [Verdicts] struct represents the verdict of the API.

    It contains the output of the periodic streams with the `timed` field and the output of the event-based streams with `event`.
    The field `timed` is a vector, containing all updates of periodic streams since the last event.
*/
#[derive(Debug)]
pub struct Verdicts<V: VerdictRepresentation> {
    /// All verdicts caused by timed streams given at each deadline that occurred.
    pub timed: Vec<(Time, V)>,
    /// The verdict that resulted from evaluation the event.
    pub event: V,
}

/**
The Monitor is the central object exposed by the API.

The [Monitor] accepts new events and computes streams.
It can compute event-based streams based on new events through `accept_event`.
It can also simply advance periodic streams up to a given timestamp through `accept_time`.
The generic argument `V` implements the [VerdictRepresentation] trait describing the output format of the API that is by default [Incremental].
The generic argument `S` implements the [Input] trait describing the input source of the API.
*/
#[allow(missing_debug_implementations)]
pub struct Monitor<S: Input, V: VerdictRepresentation = Incremental> {
    /// The representation of the specification. See [RTLolaMir](rtlola_frontend::mir::RtLolaMir).
    pub ir: RtLolaMir,
    eval: Evaluator,
    pub(crate) output_handler: Arc<OutputHandler>,
    time_manager: TimeDrivenManager,

    last_event_time: Duration,
    time_repr: TimeRepresentation,
    start_time: Option<SystemTime>,

    source: S,

    phantom: PhantomData<V>,
}

/// Crate-public interface
impl<S: Input, V: VerdictRepresentation> Monitor<S, V> {
    ///setup
    pub(crate) fn setup(config: Config, setup_data: S::CreationData) -> Monitor<S, V> {
        let output_handler = Arc::new(OutputHandler::new(&config, config.ir.triggers.len()));
        let dyn_schedule = Arc::new((Mutex::new(DynamicSchedule::new()), Condvar::new()));
        let monitor_start = SystemTime::now();

        let time_repr = match &config.mode {
            ExecutionMode::Offline(repr) => *repr,
            ExecutionMode::Online => unreachable!(),
        };
        let start_time = match time_repr {
            TimeRepresentation::Relative(_) | TimeRepresentation::Incremental(_) => Some(monitor_start),
            // If None, Default time should be the one of first event
            TimeRepresentation::Absolute(_) => None,
        };
        if let Some(start_time) = start_time {
            output_handler.set_start_time(start_time);
        }

        let input_map = config.ir.inputs.iter().map(|i| (i.name.clone(), i.reference.in_ix())).collect();

        let eval_data = EvaluatorData::new(config.ir.clone(), output_handler.clone(), dyn_schedule.clone());

        let time_manager = TimeDrivenManager::setup(config.ir.clone(), output_handler.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");

        Monitor {
            ir: config.ir,
            eval: eval_data.into_evaluator(),
            output_handler,
            time_manager,
            last_event_time: Duration::default(),
            time_repr,
            start_time,
            source: S::new(input_map, setup_data),
            phantom: PhantomData,
        }
    }

    /// Computes transforms the given time into the internal time representation.
    /// Following the given input time format
    /// Note: `TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339)` is currently not supported
    fn finalize_time(&mut self, ts: Time) -> Time {
        match self.time_repr {
            TimeRepresentation::Relative(_) => ts,
            TimeRepresentation::Incremental(_) => {
                self.last_event_time += ts;
                self.last_event_time
            }
            TimeRepresentation::Absolute(_) => {
                let unix = UNIX_EPOCH + ts;
                if self.start_time.is_none() {
                    self.start_time = Some(unix);
                    self.output_handler.set_start_time(unix);
                }
                unix.duration_since(self.start_time.unwrap()).expect("Time did not behave monotonically!")
            }
        }
    }
}

/// A raw verdict that is transformed into the respective representation
#[allow(missing_debug_implementations)]
pub struct RawVerdict<'a> {
    eval: &'a Evaluator,
}

impl<'a> From<&'a Evaluator> for RawVerdict<'a> {
    fn from(eval: &'a Evaluator) -> Self {
        RawVerdict { eval }
    }
}

/// This trait provides the functionality to pass inputs to the monitor.
/// You can either implement this trait for your own Datatype or use one of the predefined input methods.
/// See [RecordInput] and [EventInput]
pub trait Input {
    /// The type from which an event is generated by the input source.
    type Record;

    /// Arbitrary type of the data provided to the input source at creation time.
    type CreationData: Clone;

    /// Creates a new input source from a HashMap mapping the names of the inputs in the specification to their position in the event.
    fn new(map: HashMap<String, InputReference>, setup_data: Self::CreationData) -> Self;

    /// This function converts a record to an event.
    fn get_event(&self, rec: Self::Record) -> Event;
}

/// This trait provides functionality to parse a record into an event.
/// It is only used in combination with the [RecordInput].
pub trait Record {
    /// Arbitrary type of the data provided at creation time to help initializing the input method.
    type CreationData: Clone;
    /// Given the name of an input this function returns a function that given a record returns the value for that input.
    fn func_for_input(name: &str, data: Self::CreationData) -> Box<dyn Fn(&Self) -> Value>;
}

/// An input method for types that implement the [Record] trait. Useful if you do not want to bother with the order of the input streams in an event.
/// Assuming the specification has 3 inputs: 'a', 'b' and 'c'. You could implement this trait for your custom 'MyType' as follows:
/// ```
/// use rtlola_interpreter::Value;
/// use rtlola_interpreter::monitor::Record;
///
/// struct MyType {
///     a: u64,
///     b: Option<bool>,
///     c: String,
/// }
///
/// impl MyType {
///     // Generate a new value for input stream 'a'
///     fn a(rec: &Self) -> Value {
///         Value::from(rec.a)
///     }
///
///     // Generate a new value for input stream 'b'
///     fn b(rec: &Self) -> Value {
///         rec.b.map(|b| Value::from(b)).unwrap_or(Value::None)
///     }
///
///     // Generate a new value for input stream 'c'
///     fn c(rec: &Self) -> Value {
///         Value::Str(rec.c.clone().into_boxed_str())
///     }
/// }
///
/// impl Record for MyType {
///     type CreationData = ();
///
///     fn func_for_input(name: &str, _data: Self::CreationData) -> Box<dyn (Fn(&MyType) -> Value)> {
///        Box::new(match name {
///             "a" => Self::a,
///             "b" => Self::b,
///             "c" => Self::c,
///             x => panic!("Unexpected input stream {} in specification.", x),
///         })
///     }
/// }
/// ```
#[allow(missing_debug_implementations)]
pub struct RecordInput<P: Record> {
    translators: Vec<Box<dyn (Fn(&P) -> Value)>>,
}

impl<P: Record> Input for RecordInput<P> {
    type Record = P;
    type CreationData = P::CreationData;

    fn new(map: HashMap<String, InputReference>, setup_data: Self::CreationData) -> Self {
        let mut translators: Vec<Option<_>> = (0..map.len()).map(|_| None).collect();
        map.iter().for_each(|(input_name, index)| {
            translators[*index] = Some(P::func_for_input(input_name.as_str(), setup_data.clone()))
        });
        let translators = translators.into_iter().map(Option::unwrap).collect();
        Self { translators }
    }

    fn get_event(&self, rec: P) -> Event {
        self.translators.iter().map(|f| f(&rec)).collect()
    }
}

/// The simplest input method to the monitor. It accepts any type that implements `Into<Event>`.
/// The conversion to values and the order of inputs must be handled externally.
#[derive(Debug, Clone)]
pub struct EventInput<E: Into<Event>> {
    phantom: PhantomData<E>,
}

impl<E: Into<Event>> Input for EventInput<E> {
    type Record = E;
    type CreationData = ();

    fn new(_map: HashMap<String, InputReference>, _setup_data: Self::CreationData) -> Self {
        EventInput { phantom: PhantomData }
    }

    fn get_event(&self, rec: Self::Record) -> Event {
        rec.into()
    }
}

/// Public interface
impl<S: Input, V: VerdictRepresentation> Monitor<S, V> {
    /**
    Computes all periodic streams up through the new timestamp and then handles the input event.

    The new event is therefore not seen by periodic streams up through a new timestamp.
    */
    pub fn accept_event(&mut self, ev: S::Record, ts: Time) -> Verdicts<V> {
        let ev = self.source.get_event(ev);
        self.output_handler.debug(|| format!("Accepted {:?}.", ev));

        let ts = self.finalize_time(ts);

        // Evaluate timed streams with due < ts
        let mut timed: Vec<(Time, V)> = vec![];
        self.time_manager.accept_time_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed.push((due, V::create(RawVerdict::from(eval))))
        });

        // Evaluate
        self.output_handler.new_event();
        self.eval.eval_event(ev.as_slice(), ts);
        let event_change = V::create(RawVerdict::from(&self.eval));

        Verdicts::<V> { timed, event: event_change }
    }

    /**
    Computes all periodic streams up through and including the timestamp.
    */
    pub fn accept_time(&mut self, ts: Time) -> Vec<(Time, V)> {
        let mut timed_changes: Vec<(Time, V)> = vec![];

        let ts = self.finalize_time(ts);

        // Eval all timed streams with due < ts
        self.time_manager.accept_time_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed_changes.push((due, V::create(RawVerdict::from(eval))))
        });
        // Eval all timed streams with due = ts
        self.time_manager.end_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed_changes.push((due, V::create(RawVerdict::from(eval))))
        });

        timed_changes
    }

    /**
    Get the name of an input stream based on its [InputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_input(&self, id: InputReference) -> &str {
        self.ir.inputs[id].name.as_str()
    }

    /**
    Get the name of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_output(&self, id: OutputReference) -> &str {
        self.ir.outputs[id].name.as_str()
    }

    /**
    Get the message of a trigger based on its index.

    The reference is valid for the lifetime of the monitor.
    */
    pub fn trigger_message(&self, id: usize) -> &str {
        self.ir.triggers[id].message.as_str()
    }

    /**
    Get the [OutputReference] of a trigger based on its index.
    */
    pub fn trigger_stream_index(&self, id: usize) -> usize {
        self.ir.triggers[id].reference.out_ix()
    }

    /**
    Get the number of input streams.
    */
    pub fn number_of_input_streams(&self) -> usize {
        self.ir.inputs.len()
    }

    /**
    Get the number of output streams (this includes one output stream for each trigger).
    */
    pub fn number_of_output_streams(&self) -> usize {
        self.ir.outputs.len()
    }

    /**
    Get the number of triggers.
    */
    pub fn number_of_triggers(&self) -> usize {
        self.ir.triggers.len()
    }

    /**
    Get the type of an input stream based on its [InputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn type_of_input(&self, id: InputReference) -> &Type {
        &self.ir.inputs[id].ty
    }

    /**
    Get the type of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn type_of_output(&self, id: OutputReference) -> &Type {
        &self.ir.outputs[id].ty
    }

    /**
    Get the extend rate of an output stream based on its [OutputReference].

    The reference is valid for the lifetime of the monitor.
    */
    pub fn extend_rate_of_output(&self, id: OutputReference) -> Option<Duration> {
        self.ir
            .time_driven
            .iter()
            .find(|time_driven_stream| time_driven_stream.reference.out_ix() == id)
            .map(|time_driven_stream| time_driven_stream.period_in_duration())
    }

    /// Switch [VerdictRepresentation]s of the [Monitor].
    pub fn with_verdict_representation<T: VerdictRepresentation>(self) -> Monitor<S, T> {
        let Monitor {
            ir,
            eval,
            output_handler,
            time_manager,
            last_event_time,
            time_repr,
            start_time,
            source,
            phantom: _,
        } = self;
        Monitor::<S, T> {
            ir,
            eval,
            output_handler,
            time_manager,
            last_event_time,
            time_repr,
            start_time,
            source,
            phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::RelativeTimeFormat;
    use crate::coordination::monitor::Change;
    use crate::monitor::{Event, EventInput, Incremental, Monitor, Total, Value, VerdictRepresentation};
    use crate::ConfigBuilder;
    use std::time::{Duration, Instant};

    fn setup<V: VerdictRepresentation>(spec: &str) -> (Instant, Monitor<EventInput<Event>, V>) {
        // Init Monitor API
        let monitor =
            ConfigBuilder::api().relative_input_time(RelativeTimeFormat::FloatSecs).spec_str(spec).monitor(());
        (Instant::now(), monitor)
    }

    fn sort_total(res: Total) -> Total {
        let Total { inputs, mut outputs } = res;
        outputs.iter_mut().for_each(|s| s.sort());
        Total { inputs, outputs }
    }

    fn sort_incremental(mut res: Incremental) -> Incremental {
        res.iter_mut().for_each(|(_, changes)| changes.sort());
        res
    }

    // #[derive(Debug, Clone)]
    // pub(crate) struct TestInputPayload {
    //     mapping: Vec<usize>,
    //     timestamp: Duration,
    //     a: f64,
    //     d: Messages,
    // }
    //
    // #[derive(Debug, Clone)]
    // enum Messages {
    //     M0 { b: f64 },
    //     M1 { c: u64 },
    // }
    // impl TestInputPayload {
    //     fn a(p: &Self) -> Value {
    //         Value::from(p.a)
    //     }
    //     fn b(p: &Self) -> Value {
    //         match p.d {
    //             Messages::M0 { b } => Value::from(b),
    //             _ => Value::None,
    //         }
    //     }
    //     fn c(p: &Self) -> Value {
    //         match p.d {
    //             Messages::M1 { c } => Value::from(c),
    //             _ => Value::None,
    //         }
    //     }
    // }

    #[test]
    fn test_const_output_literals() {
        let (start, mut monitor) = setup::<Total>(
            r#"
        input i_0: UInt8

        output o_0: Bool @i_0 := true
        output o_1: UInt8 @i_0 := 3
        output o_2: Int8 @i_0 := -5
        output o_3: Float32 @i_0 := -123.456
        output o_4: String @i_0 := "foobar"
        "#,
        );
        let v = Value::Unsigned(3);
        let res = monitor.accept_event(vec![v.clone()], start.elapsed());
        assert!(res.timed.is_empty());
        let res = res.event;
        assert_eq!(res.inputs[0], Some(v));
        assert_eq!(res.outputs[0][0], (None, Some(Value::Bool(true))));
        assert_eq!(res.outputs[1][0], (None, Some(Value::Unsigned(3))));
        assert_eq!(res.outputs[2][0], (None, Some(Value::Signed(-5))));
        assert_eq!(res.outputs[3][0], (None, Some(Value::new_float(-123.456))));
        assert_eq!(res.outputs[4][0], (None, Some(Value::Str("foobar".into()))));
    }

    #[test]
    fn test_count_window() {
        let (_, mut monitor) =
            setup::<Incremental>("input a: UInt16\noutput b: UInt16 @0.25Hz := a.aggregate(over: 40s, using: #)");

        let n = 25;
        let mut time = Duration::from_secs(45);
        let res = monitor.accept_event(vec![Value::Unsigned(1)], time);
        assert!(res.event.is_empty());
        assert_eq!(res.timed.len(), 11);
        assert!(res.timed.iter().all(|(time, change)| time.as_secs() % 4 == 0
            && change[0].0 == 0
            && change[0].1[0] == Change::Value(None, Value::Unsigned(0))));
        for v in 2..=n {
            time += Duration::from_secs(1);
            let res = monitor.accept_event(vec![Value::Unsigned(v)], time);

            assert_eq!(res.event.len(), 0);
            if (v - 1) % 4 == 0 {
                assert_eq!(res.timed.len(), 1);
                assert_eq!(res.timed[0].1[0].1[0], Change::Value(None, Value::Unsigned(v - 1)));
            } else {
                assert_eq!(res.timed.len(), 0);
            }
        }
    }

    #[test]
    fn test_spawn_eventbased() {
        let (_, mut monitor) = setup::<Total>(
            "input a: Int32\n\
                  input b: Int32\n\
                  output c(x: Int32) spawn with a := x + a\n\
                  output d := b",
        );

        let res = monitor.accept_event(vec![Value::Signed(15), Value::None], Duration::from_secs(1));
        let expected = Total {
            inputs: vec![Some(Value::Signed(15)), None],
            outputs: vec![vec![(Some(vec![Value::Signed(15)]), Some(Value::Signed(30)))], vec![(None, None)]],
        };
        assert_eq!(res.event, expected);
        assert_eq!(res.timed.len(), 0);

        let res = monitor.accept_event(vec![Value::Signed(20), Value::Signed(7)], Duration::from_secs(2));
        let expected = Total {
            inputs: vec![Some(Value::Signed(20)), Some(Value::Signed(7))],
            outputs: vec![
                vec![
                    (Some(vec![Value::Signed(15)]), Some(Value::Signed(35))),
                    (Some(vec![Value::Signed(20)]), Some(Value::Signed(40))),
                ],
                vec![(None, Some(Value::Signed(7)))],
            ],
        };
        assert_eq!(sort_total(res.event), sort_total(expected));
        assert_eq!(res.timed.len(), 0);

        let res = monitor.accept_event(vec![Value::None, Value::Signed(42)], Duration::from_secs(3));
        let expected = Total {
            inputs: vec![Some(Value::Signed(20)), Some(Value::Signed(42))],
            outputs: vec![
                vec![
                    (Some(vec![Value::Signed(15)]), Some(Value::Signed(35))),
                    (Some(vec![Value::Signed(20)]), Some(Value::Signed(40))),
                ],
                vec![(None, Some(Value::Signed(42)))],
            ],
        };
        assert_eq!(sort_total(res.event), sort_total(expected));
        assert_eq!(res.timed.len(), 0);
    }

    #[test]
    fn test_eval_close() {
        let (_, mut monitor) = setup::<Incremental>(
            "input a: Int32\n\
                  output c(x: Int32)\n\
                    spawn with a \n\
                    close @a true\n\
                  := x + a",
        );

        let res = monitor.accept_event(vec![Value::Signed(15)], Duration::from_secs(1));
        let mut expected = vec![
            Change::Spawn(vec![Value::Signed(15)]),
            Change::Value(Some(vec![Value::Signed(15)]), Value::Signed(30)),
            Change::Close(vec![Value::Signed(15)]),
        ];
        expected.sort();
        assert!(res.timed.is_empty());
        assert_eq!(res.event[0].0, 0);

        assert_eq!(sort_incremental(res.event)[0].1, expected);
    }
}
