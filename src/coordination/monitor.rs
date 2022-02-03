use crate::basics::{EvalConfig, OutputHandler, Time};
use crate::coordination::time_driven_manager::TimeDrivenManager;
use crate::coordination::{DynamicSchedule, Event};
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::storage::Value;
use crate::{ExecutionMode, TimeRepresentation};
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, Type};
use std::collections::HashMap;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
/// If a stream not dynamically created it defaults to `None`.
/// If a stream is dynamically created but does not have parameters it defaults to `Some(vec![])`
pub type Parameters = Option<Vec<Value>>;

/// A stream instance. First element represents the parameter values of the instance, the second element the value of the instance.
pub type Instance = (Parameters, Option<Value>);

// Todo: Implement Display
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Change {
    Spawn(Vec<Value>),
    Close(Vec<Value>),
    Value(Parameters, Value),
}

/**
    Represents a snapshot of the monitor containing the value of all updated output stream.
*/
pub type Incremental = Vec<(OutputReference, Vec<Change>)>;

impl VerdictRepresentation for Incremental {
    fn create(data: RawVerdict) -> Self {
        data.eval.peek_fresh()
    }
}

/**
    Represents a snapshot of the monitor containing the current value of each output stream.

    The ith value in the inputs vector is the current value of the ith input stream.
    The ith value in the outputs vector is the vector of instances of the ith output stream.
    If the stream has no instance yet, this vector is empty. If a stream is not parameterized, the vector will always be of size 1.

*/
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Total {
    pub inputs: Vec<Option<Value>>,
    pub outputs: Vec<Vec<Instance>>,
}

impl VerdictRepresentation for Total {
    fn create(data: RawVerdict) -> Self {
        Total { inputs: data.eval.peek_inputs(), outputs: data.eval.peek_outputs() }
    }
}

/**
    Represents the index and the formated message of all violated triggers.
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
    pub timed: Vec<(Time, V)>,
    pub event: V,
}

/**
The [Monitor] accepts new events and computes streams.

The [Monitor] is the central object exposed by the API.
It can compute event-based streams based on new events through `accept_event`.
It can also simply advance periodic streams up to a given timestamp through `accept_time`.
The generic argument `V` implements the [VerdictRepresentation] trait describing the outputformat of the API that is by default [Incremental].
*/
#[allow(missing_debug_implementations)]
pub struct Monitor<V: VerdictRepresentation = Incremental> {
    pub ir: RtLolaMir,
    eval: Evaluator,
    pub(crate) output_handler: Arc<OutputHandler>,
    time_manager: TimeDrivenManager,

    last_event_time: Duration,
    time_repr: TimeRepresentation,
    start_time: Option<SystemTime>,

    phantom: PhantomData<V>,
}

/// Crate-public interface
impl<V: VerdictRepresentation> Monitor<V> {
    ///setup
    pub(crate) fn setup(ir: RtLolaMir, config: EvalConfig) -> Monitor<V> {
        let output_handler = Arc::new(OutputHandler::new(&config, ir.triggers.len()));
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

        let eval_data = EvaluatorData::new(ir.clone(), output_handler.clone(), dyn_schedule.clone());

        let time_manager = TimeDrivenManager::setup(ir.clone(), output_handler.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");

        Monitor {
            ir,
            eval: eval_data.into_evaluator(),
            output_handler,
            time_manager,
            last_event_time: Duration::default(),
            time_repr,
            start_time,
            phantom: PhantomData,
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

// External Value without None
// Default Record = Into<Event>

trait Input {
    type Record;

    fn new(map: HashMap<String, InputReference>) -> Self;

    fn get_event(&self, rec: Self::Record) -> Event;
}

trait Record {
    fn func_for_input(name: &str) -> fn(&Self) -> Value;
}

#[derive(Debug, Clone)]
pub(crate) struct TestInputPayload {
    mapping: Vec<usize>,
    timestamp: Duration,
    a: f64,
    d: Messages,
}

#[derive(Debug, Clone)]
enum Messages {
    M0 { b: f64 },
    M1 { c: u64 },
}

impl TestInputPayload {
    fn a(p: &Self) -> Value {
        Value::from(p.a)
    }
    fn b(p: &Self) -> Value {
        match p.d {
            Messages::M0 { b } => Value::from(b),
            _ => Value::None,
        }
    }
    fn c(p: &Self) -> Value {
        match p.d {
            Messages::M1 { c } => Value::from(c),
            _ => Value::None,
        }
    }
}

struct RecordParser<P: Record> {
    translator: Vec<fn(&P) -> Value>,
}

impl<P: Record> Input for RecordParser<P> {
    type Record = P;

    fn new(map: HashMap<String, InputReference>) -> Self {
        let mut translator = Vec::with_capacity(map.len());
        map.iter().for_each(|(input_name, index)| translator[*index] = P::func_for_input(input_name.as_str()));
        Self { translator }
    }

    fn get_event(&self, rec: P) -> Event {
        self.translator.iter().map(|f| f(&rec)).collect()
    }
}

/// Public interface
impl<V: VerdictRepresentation> Monitor<V> {
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

    /**
    Computes all periodic streams up through the new timestamp and then handles the input event.

    The new event is therefore not seen by periodic streams up through the new timestamp.
    */
    pub fn accept_event<E: Into<Event>>(&mut self, ev: E, ts: Time) -> Verdicts<V> {
        let ev = ev.into();
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
    Computes all periodic streams up through the new timestamp.
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
    pub fn with_verdict_representation<T: VerdictRepresentation>(self) -> Monitor<T> {
        let Monitor { ir, eval, output_handler, time_manager, last_event_time, time_repr, start_time, phantom: _ } =
            self;
        Monitor::<T> {
            ir,
            eval,
            output_handler,
            time_manager,
            last_event_time,
            time_repr,
            start_time,
            phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::coordination::monitor::Change;
    use crate::{
        Config, EvalConfig, Incremental, Monitor, RelativeTimeFormat, TimeRepresentation, Total, Value,
        VerdictRepresentation,
    };
    use std::time::{Duration, Instant};

    fn setup<V: VerdictRepresentation>(spec: &str) -> (Instant, Monitor<V>) {
        // Init Monitor API
        let config = rtlola_frontend::ParserConfig::for_string(spec.to_string());
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        let eval_conf = EvalConfig::api(TimeRepresentation::Relative(RelativeTimeFormat::FloatSecs));
        (Instant::now(), Config::new(eval_conf, ir).as_api())
    }

    fn sort_total(res: Total) -> Total {
        let Total { inputs, mut outputs } = res;
        outputs.iter_mut().for_each(|s| s.sort());
        Total { inputs, outputs }
    }

    fn sort_incremental(mut res: Incremental) -> Incremental {
        res.iter_mut().for_each(|(out_ref, changes)| changes.sort());
        res
    }

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
        dbg!(&res.event);
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
