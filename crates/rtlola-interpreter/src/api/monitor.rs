//! The [Monitor] is the single threaded version of the API.
//! Consequently deadlines of timed streams are only evaluated with a new event.
//! Hence this API is more suitable for offline monitoring or embedded scenarios.
//!
//! The [Monitor] is parameterized over its input and output method.
//! The preferred method to create an API is using the [ConfigBuilder](crate::ConfigBuilder) and the [monitor](crate::ConfigBuilder::monitor) method.
//!
//! # Input Method
//! An input method has to implement the [EventFactory](crate::input::EventFactory) trait. Out of the box two different methods are provided:
//! * [ArrayFactory](crate::input::ArrayFactory): Provides a basic input method for anything that already is an [Event] or that can be transformed into one using `TryInto<[Value]>`.
//! * [MappedFactory](crate::input::MappedFactory): Is a more elaborate input method. It allows to provide a custom data structure to the monitor as an event, as long as it implements the [InputMap](crate::input::InputMap) trait.
//!     If implemented this traits provides functionality to generate a new value for any input stream from the data structure.
//!
//! # Output Method
//! The [Monitor] can provide output with a varying level of detail captured by the [VerdictRepresentation] trait. The different output formats are:
//! * [Incremental]: For each processed event a condensed list of monitor state changes is provided.
//! * [Total]: For each event a complete snapshot of the current monitor state is returned
//! * [TotalIncremental](crate::monitor::TotalIncremental): For each processed event a complete list of monitor state changes is provided
//! * [TriggerMessages]: For each event a list of violated triggers with their description is produced.
use std::cell::RefCell;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use itertools::Itertools;
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, TriggerReference, Type};
#[cfg(feature = "serde")]
use serde::Serialize;

use crate::config::{Config, ExecutionMode};
use crate::configuration::time::{OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::input::{EventFactory, EventFactoryError};
use crate::schedule::schedule_manager::ScheduleManager;
use crate::schedule::DynamicSchedule;
use crate::storage::Value;
use crate::{CondSerialize, Time};

/// An event to be handled by the interpreter
pub type Event = Vec<Value>;

/**
    Provides the functionality to generate a snapshot of the streams values.
*/
pub trait VerdictRepresentation: Clone + Debug + Send + CondSerialize + 'static {
    /// This subtype captures the tracing capabilities of the verdict representation.
    type Tracing: Tracer;

    /// Creates a snapshot of the streams values.
    fn create(data: RawVerdict) -> Self;

    /// Creates a snapshot of the streams values including tracing data.
    fn create_with_trace(data: RawVerdict, _tracing: Self::Tracing) -> Self {
        Self::create(data)
    }

    /// Returns whether the verdict is empty. I.e. it doesn't contain any information.
    fn is_empty(&self) -> bool;
}

/**
Provides the functionality to collect additional tracing data during evaluation.
The 'start' methods are guaranteed to be called before the 'end' method, while either both or none of them are called.
 */
pub trait Tracer: Default + Clone + Debug + Send + 'static {
    /// This method is invoked at the start of event parsing
    fn parse_start(&mut self) {}
    /// This method is invoked at the end of event parsing
    fn parse_end(&mut self) {}

    /// This method is invoked at the start of the evaluation cycle.
    fn eval_start(&mut self) {}
    /// This method is invoked at the end of the evaluation cycle.
    fn eval_end(&mut self) {}

    /// This method is invoked at the start of the spawn evaluation of stream `output`
    fn spawn_start(&mut self, _output: OutputReference) {}
    /// This method is invoked at the end of the spawn evaluation of stream `output`
    fn spawn_end(&mut self, _output: OutputReference) {}

    /// This method is invoked at the start of the evaluation of stream `output`
    fn instance_eval_start(&mut self, _output: OutputReference, _instance: &[Value]) {}
    /// This method is invoked at the end of the evaluation of stream `output`
    fn instance_eval_end(&mut self, _output: OutputReference, _instance: &[Value]) {}

    /// This method is invoked at the start of the close evaluation of stream `output`
    fn close_start(&mut self, _output: OutputReference, _instance: &[Value]) {}
    /// This method is invoked at the end of the close evaluation of stream `output`
    fn close_end(&mut self, _output: OutputReference, _instance: &[Value]) {}
}

/// This tracer provides no tracing data at all and serves as a default value.
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTracer {}
impl Tracer for NoTracer {}

/// A generic VerdictRepresentation suitable to use with any tracer.
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone)]
pub struct TracingVerdict<T: Tracer, V: VerdictRepresentation> {
    /// The contained tracing information.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub tracer: T,
    /// The verdict given in the chosen VerdictRepresentation
    pub verdict: V,
}

impl<T: Tracer, V: VerdictRepresentation<Tracing = NoTracer>> VerdictRepresentation for TracingVerdict<T, V> {
    type Tracing = T;

    fn create(data: RawVerdict) -> Self {
        Self {
            tracer: T::default(),
            verdict: V::create(data),
        }
    }

    fn create_with_trace(data: RawVerdict, tracing: Self::Tracing) -> Self {
        Self {
            tracer: tracing,
            verdict: V::create(data),
        }
    }

    fn is_empty(&self) -> bool {
        V::is_empty(&self.verdict)
    }
}

/// A type representing the parameters of a stream.
/// If a stream is not dynamically created it defaults to `None`.
/// If a stream is dynamically created but does not have parameters it defaults to `Some(vec![])`
pub type Parameters = Option<Vec<Value>>;

/// A stream instance. First element represents the parameter values of the instance, the second element the value of the instance.
pub type Instance = (Parameters, Option<Value>);

/// An enum representing a change in the monitor.
#[cfg_attr(feature = "serde", derive(Serialize))]
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
            Change::Value(para, value) => {
                match para {
                    Some(para) => write!(f, "Instance<{}> = {}", para.iter().join(", "), value),
                    None => write!(f, "Value = {}", value),
                }
            },
        }
    }
}

/**
    Represents the changes of the monitor state. Each element represents a set of [Change]s of a specific output stream.
*/
pub type Incremental = Vec<(OutputReference, Vec<Change>)>;

impl VerdictRepresentation for Incremental {
    type Tracing = NoTracer;

    fn create(data: RawVerdict) -> Self {
        data.eval.peek_fresh_outputs()
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}

/**
Represents the changes of the monitor state divided into inputs, outputs and trigger.
Changes of output streams are represented by a set of [Change]s.
A change of an input is represented by its new [Value].
A change of a trigger is represented by its [TriggerReference].

Note: Only streams that actually changed are included in the collections.
 */
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone)]
pub struct TotalIncremental {
    /// The set of changed inputs.
    pub inputs: Vec<(InputReference, Value)>,
    /// The set of changed outputs.
    pub outputs: Vec<(OutputReference, Vec<Change>)>,
    /// The set of changed triggers. I.e. all triggers that were activated.
    pub trigger: Vec<TriggerReference>,
}

impl VerdictRepresentation for TotalIncremental {
    type Tracing = NoTracer;

    fn create(data: RawVerdict) -> Self {
        let inputs = data.eval.peek_fresh_input();
        let outputs = data.eval.peek_fresh_outputs();
        let trigger = data.eval.peek_violated_triggers();
        Self {
            inputs,
            outputs,
            trigger,
        }
    }

    fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.trigger.is_empty()
    }
}

/**
    Represents a snapshot of the monitor state containing the current value of each output and input stream.
*/
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Total {
    /// The ith value in this vector is the current value of the ith input stream.
    pub inputs: Vec<Option<Value>>,

    /// The ith value in this vector is the vector of instances of the ith output stream.
    /// If the stream has no instance yet, this vector is empty. If a stream is not parameterized, the vector will always be of size 1.
    pub outputs: Vec<Vec<Instance>>,
}

impl VerdictRepresentation for Total {
    type Tracing = NoTracer;

    fn create(data: RawVerdict) -> Self {
        Total {
            inputs: data.eval.peek_inputs(),
            outputs: data.eval.peek_outputs(),
        }
    }

    fn is_empty(&self) -> bool {
        false
    }
}

/**
    Represents the index and the formatted message of all violated triggers.
*/
pub type TriggerMessages = Vec<(OutputReference, Parameters, String)>;

impl VerdictRepresentation for TriggerMessages {
    type Tracing = NoTracer;

    fn create(data: RawVerdict) -> Self
    where
        Self: Sized,
    {
        data.eval.peek_violated_triggers_messages()
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}

/**
    The [Verdicts] struct represents the verdict of the API.

    It contains the output of the periodic streams with the `timed` field and the output of the event-based streams with `event`.
    The field `timed` is a vector, containing all updates of periodic streams since the last event.
*/
#[cfg_attr(feature = "serde", derive(Serialize))]
#[derive(Debug, Clone)]
pub struct Verdicts<V: VerdictRepresentation, VerdictTime: OutputTimeRepresentation> {
    /// All verdicts caused by timed streams given at each deadline that occurred.
    pub timed: Vec<(VerdictTime::InnerTime, V)>,
    /// The verdict that resulted from evaluation the event.
    pub event: V,
    /// The time of the event.
    pub ts: VerdictTime::InnerTime,
}

/**
The Monitor is the central object exposed by the API.

The [Monitor] accepts new events and computes streams.
It can compute event-based streams based on new events through `accept_event`.
It can also simply advance periodic streams up to a given timestamp through `accept_time`.
The generic argument `Source` implements the [EventFactory] trait describing the input source of the API.
The generic argument `SourceTime` implements the [TimeRepresentation] trait defining the input time format.
The generic argument `Verdict` implements the [VerdictRepresentation] trait describing the output format of the API that is by default [Incremental].
The generic argument `VerdictTime` implements the [TimeRepresentation] trait defining the output time format. It defaults to [RelativeFloat]
 */
#[allow(missing_debug_implementations)]
pub struct Monitor<Source, Mode, Verdict = Incremental, VerdictTime = RelativeFloat>
where
    Source: EventFactory,
    Mode: ExecutionMode,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation + 'static,
{
    ir: RtLolaMir,
    eval: Evaluator,

    schedule_manager: ScheduleManager,

    source: Source,

    source_time: Mode::SourceTime,
    output_time: VerdictTime,

    phantom: PhantomData<Verdict>,
}

/// Crate-public interface
impl<Source, Mode, Verdict, VerdictTime> Monitor<Source, Mode, Verdict, VerdictTime>
where
    Source: EventFactory,
    Mode: ExecutionMode,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    ///setup
    pub fn setup(
        config: Config<Mode, VerdictTime>,
        setup_data: Source::CreationData,
    ) -> Result<Monitor<Source, Mode, Verdict, VerdictTime>, EventFactoryError> {
        let dyn_schedule = Rc::new(RefCell::new(DynamicSchedule::new()));
        let mut source_time = config.mode.time_representation().clone();
        let mut output_time = VerdictTime::default();

        let st = source_time.init_start_time(config.start_time);
        output_time.set_start_time(st);

        let input_map = config
            .ir
            .inputs
            .iter()
            .map(|i| (i.name.clone(), i.reference.in_ix()))
            .collect();

        let eval_data = EvaluatorData::new(config.ir.clone(), dyn_schedule.clone());

        let time_manager = ScheduleManager::setup(config.ir.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");

        Ok(Monitor {
            ir: config.ir,
            eval: eval_data.into_evaluator(),

            schedule_manager: time_manager,

            source: Source::new(input_map, setup_data)?,

            source_time,
            output_time,

            phantom: PhantomData,
        })
    }

    fn eval_deadlines(&mut self, ts: Time, only_before: bool) -> Vec<(Time, Verdict)> {
        let mut timed: Vec<(Time, Verdict)> = vec![];
        while self.schedule_manager.get_next_due().is_some() {
            let mut tracer = Verdict::Tracing::default();
            tracer.eval_start();
            let due = self.schedule_manager.get_next_due().unwrap();
            if due > ts || (only_before && due == ts) {
                break;
            }
            let deadline = self.schedule_manager.get_next_deadline(ts);

            self.eval.eval_time_driven_tasks(deadline, due, &mut tracer);
            tracer.eval_end();
            timed.push((due, Verdict::create_with_trace(RawVerdict::from(&self.eval), tracer)))
        }
        timed
    }
}

/// A raw verdict that is transformed into the respective representation
#[allow(missing_debug_implementations)]
#[derive(Copy, Clone)]
pub struct RawVerdict<'a> {
    eval: &'a Evaluator,
}

impl<'a> From<&'a Evaluator> for RawVerdict<'a> {
    fn from(eval: &'a Evaluator) -> Self {
        RawVerdict { eval }
    }
}

/// Public interface
impl<Source, Mode, Verdict, VerdictTime> Monitor<Source, Mode, Verdict, VerdictTime>
where
    Source: EventFactory,
    Mode: ExecutionMode,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    /**
    Computes all periodic streams up through the new timestamp and then handles the input event.

    The new event is therefore not seen by periodic streams up through a new timestamp.
    */
    pub fn accept_event(
        &mut self,
        ev: Source::Record,
        ts: <Mode::SourceTime as TimeRepresentation>::InnerTime,
    ) -> Result<Verdicts<Verdict, VerdictTime>, EventFactoryError> {
        let mut tracer = Verdict::Tracing::default();

        tracer.parse_start();
        let ev = self.source.get_event(ev)?;
        tracer.parse_end();
        let ts = self.source_time.convert_from(ts);

        // Evaluate timed streams with due < ts
        let timed = if self.ir.has_time_driven_features() {
            self.eval_deadlines(ts, true)
        } else {
            vec![]
        };

        // Evaluate
        tracer.eval_start();
        self.eval.eval_event(ev.as_slice(), ts, &mut tracer);
        tracer.eval_end();
        let event_change = Verdict::create_with_trace(RawVerdict::from(&self.eval), tracer);

        let timed = timed
            .into_iter()
            .map(|(t, v)| (self.output_time.convert_into(t), v))
            .collect();

        Ok(Verdicts::<Verdict, VerdictTime> {
            timed,
            event: event_change,
            ts: self.output_time.convert_into(ts),
        })
    }

    /**
    Computes all periodic streams up through and including the timestamp.
    */
    pub fn accept_time(
        &mut self,
        ts: <Mode::SourceTime as TimeRepresentation>::InnerTime,
    ) -> Vec<(VerdictTime::InnerTime, Verdict)> {
        let ts = self.source_time.convert_from(ts);

        let timed = if self.ir.has_time_driven_features() {
            self.eval_deadlines(ts, false)
        } else {
            vec![]
        };

        timed
            .into_iter()
            .map(|(t, v)| (self.output_time.convert_into(t), v))
            .collect()
    }

    /// Returns the underlying representation of the specification as an [RtLolaMir]
    pub fn ir(&self) -> &RtLolaMir {
        &self.ir
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
    Get the [OutputReference] of a trigger based on its index.
    */
    pub fn trigger_stream_index(&self, id: usize) -> usize {
        self.ir.triggers[id].output_reference.out_ix()
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
    pub fn with_verdict_representation<T: VerdictRepresentation>(self) -> Monitor<Source, Mode, T, VerdictTime> {
        let Monitor {
            ir,
            eval,
            schedule_manager: time_manager,
            source_time,
            source,
            output_time,
            phantom: _,
        } = self;
        Monitor {
            ir,
            eval,
            schedule_manager: time_manager,
            source_time,
            source,
            output_time,
            phantom: PhantomData,
        }
    }
}

#[cfg(test)]
#[cfg(not(feature = "serde"))]
mod tests {
    use std::convert::Infallible;
    use std::time::{Duration, Instant};

    use crate::api::monitor::Change;
    use crate::config::OfflineMode;
    use crate::input::ArrayFactory;
    use crate::monitor::{Incremental, Monitor, Total, Value, VerdictRepresentation};
    use crate::time::RelativeFloat;
    use crate::ConfigBuilder;

    fn setup<const N: usize, V: VerdictRepresentation>(
        spec: &str,
    ) -> (
        Instant,
        Monitor<ArrayFactory<N, Infallible, [Value; N]>, OfflineMode<RelativeFloat>, V, RelativeFloat>,
    ) {
        // Init Monitor API
        let monitor = ConfigBuilder::new()
            .spec_str(spec)
            .offline::<RelativeFloat>()
            .with_array_events::<N, Infallible, [Value; N]>()
            .with_verdict::<V>()
            .monitor()
            .expect("Failed to create monitor");
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

    #[test]
    fn test_const_output_literals() {
        let (start, mut monitor) = setup::<1, Total>(
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
        let res = monitor
            .accept_event([v.clone()], start.elapsed())
            .expect("Failed to accept value");
        assert!(res.timed.is_empty());
        let res = res.event;
        assert_eq!(res.inputs[0], Some(v));
        assert_eq!(res.outputs[0][0], (None, Some(Value::Bool(true))));
        assert_eq!(res.outputs[1][0], (None, Some(Value::Unsigned(3))));
        assert_eq!(res.outputs[2][0], (None, Some(Value::Signed(-5))));
        assert_eq!(res.outputs[3][0], (None, Some(Value::try_from(-123.456).unwrap())));
        assert_eq!(res.outputs[4][0], (None, Some(Value::Str("foobar".into()))));
    }

    #[test]
    fn test_count_window() {
        let (_, mut monitor) = setup::<1, Incremental>(
            "input a: UInt16\noutput b: UInt16 @0.25Hz := a.aggregate(over: 40s, using: count)",
        );

        let n = 25;
        let mut time = Duration::from_secs(45);
        let res = monitor
            .accept_event([Value::Unsigned(1)], time)
            .expect("Failed to accept value");
        assert!(res.event.is_empty());
        assert_eq!(res.timed.len(), 11);
        assert!(res.timed.iter().all(|(time, change)| {
            time.as_secs() % 4 == 0 && change[0].0 == 0 && change[0].1[0] == Change::Value(None, Value::Unsigned(0))
        }));
        for v in 2..=n {
            time += Duration::from_secs(1);
            let res = monitor
                .accept_event([Value::Unsigned(v)], time)
                .expect("Failed to accept value");

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
        let (_, mut monitor) = setup::<2, Total>(
            "input a: Int32\n\
                  input b: Int32\n\
                  output c(x: Int32) spawn with a eval with x + a\n\
                  output d := b",
        );

        let res = monitor
            .accept_event([Value::Signed(15), Value::None], Duration::from_secs(1))
            .expect("Failed to accept value");
        let expected = Total {
            inputs: vec![Some(Value::Signed(15)), None],
            outputs: vec![
                vec![(Some(vec![Value::Signed(15)]), Some(Value::Signed(30)))],
                vec![(None, None)],
            ],
        };
        assert_eq!(res.event, expected);
        assert_eq!(res.timed.len(), 0);

        let res = monitor
            .accept_event([Value::Signed(20), Value::Signed(7)], Duration::from_secs(2))
            .expect("Failed to accept value");
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

        let res = monitor
            .accept_event([Value::None, Value::Signed(42)], Duration::from_secs(3))
            .expect("Failed to accept value");
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
        let (_, mut monitor) = setup::<1, Incremental>(
            "input a: Int32\n\
                  output c(x: Int32)\n\
                    spawn with a \n\
                    close @a when true\n\
                    eval with x + a",
        );

        let res = monitor
            .accept_event([Value::Signed(15)], Duration::from_secs(1))
            .expect("Failed to accept value");
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
