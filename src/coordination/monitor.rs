use crate::basics::{EvalConfig, OutputHandler, Time};
use crate::coordination::time_driven_manager::TimeDrivenManager;
use crate::coordination::{DynamicSchedule, Event};
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::storage::Value;
use rtlola_frontend::mir::{InputReference, OutputReference, RtLolaMir, Type};
use std::marker::PhantomData;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/**
    Provides the functionality to generate a snapshot of the streams values.
*/
pub trait VerdictRepresentation {
    /// Creates a snapshot of the streams values.
    fn create(data: VerdictCreationData) -> Self
    where
        Self: Sized;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamState {
    NonParameterized(Value),
    Parameterized(Vec<(Vec<Value>, Value)>)
}

/**
    Represents a snapshot of the monitor containing the value of all updated output stream.
*/
pub type Incremental = Vec<(OutputReference, StreamState)>;

impl VerdictRepresentation for Incremental {
    fn create(data: VerdictCreationData) -> Self {
        data.eval.peek_fresh()
    }
}

/**
    Represents a snapshot of the monitor containing the current value of each output stream.

    The ith value in the vector is the current value of the ith input or output stream.
*/
#[derive(Debug)]
pub struct Total {
    pub inputs: Vec<Option<Value>>,
    pub outputs: Vec<Option<StreamState>>,
}

impl VerdictRepresentation for Total {
    fn create(data: VerdictCreationData) -> Self {
        Total { inputs: data.eval.peek_inputs(), outputs: data.eval.peek_outputs() }
    }
}

/**
    Represents the index and the formated message of all violated triggers.
*/
pub type TriggerMessages = Vec<(OutputReference, String)>;

impl VerdictRepresentation for TriggerMessages {
    fn create(data: VerdictCreationData) -> Self
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
    fn create(data: VerdictCreationData) -> Self
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
    phantom: PhantomData<V>,
}

/// Crate-public interface
impl<V: VerdictRepresentation> Monitor<V> {
    ///setup
    pub(crate) fn setup(
        ir: RtLolaMir,
        config: EvalConfig,
    ) -> Monitor<V> {
        let output_handler = Arc::new(OutputHandler::new(&config, ir.triggers.len()));
        let dyn_schedule = Arc::new((Mutex::new(DynamicSchedule::new()), Condvar::new()));

        let eval_data = EvaluatorData::new(ir.clone(), output_handler.clone(), dyn_schedule.clone());

        let time_manager = TimeDrivenManager::setup(ir.clone(), output_handler.clone(), dyn_schedule)
            .expect("Error computing schedule for time-driven streams");

        Monitor { ir, eval: eval_data.into_evaluator(), output_handler, time_manager, phantom: PhantomData }
    }
}

/// The data used to create a Verdict
#[allow(missing_debug_implementations)]
pub struct VerdictCreationData<'a> {
    eval: &'a Evaluator,
}

impl<'a> From<&'a Evaluator> for VerdictCreationData<'a> {
    fn from(eval: &'a Evaluator) -> Self {
        VerdictCreationData { eval }
    }
}

/// Public interface
impl<V: VerdictRepresentation> Monitor<V> {
    /**
    Computes all periodic streams up through the new timestamp and then handles the input event.

    The new event is therefore not seen by periodic streams up through the new timestamp.
    */
    pub fn accept_event<E: Into<Event>>(&mut self, ev: E, ts: Time) -> Verdicts<V> {
        let ev = ev.into();
        self.output_handler.debug(|| format!("Accepted {:?}.", ev));

        let timed = self.accept_time_of_event(ts);

        // Evaluate
        self.output_handler.new_event();
        self.eval.eval_event(ev.as_slice(), ts);
        let event_change = V::create(VerdictCreationData::from(&self.eval));

        Verdicts::<V> { timed, event: event_change }
    }

    fn accept_time_of_event(&mut self, ts: Time) -> Vec<(Time, V)> {
        let mut timed_changes: Vec<(Time, V)> = vec![];

        self.time_manager.accept_time_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed_changes.push((due, V::create(VerdictCreationData::from(eval))))
        });

        timed_changes
    }

    /**
    Computes all periodic streams up through the new timestamp.
    */
    pub fn accept_time(&mut self, ts: Time) -> Vec<(Time, V)> {
        let mut timed_changes: Vec<(Time, V)> = vec![];

        // Eval all timed streams with due < ts
        self.time_manager.accept_time_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed_changes.push((due, V::create(VerdictCreationData::from(eval))))
        });
        // Eval all timed streams with due = ts
        self.time_manager.end_offline_with_callback(&mut self.eval, ts, |due, eval| {
            timed_changes.push((due, V::create(VerdictCreationData::from(eval))))
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
        let Monitor { ir, eval, output_handler, time_manager, phantom: _ } = self;
        Monitor::<T> { ir, eval, output_handler, time_manager, phantom: PhantomData }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Monitor, EvalConfig, TimeRepresentation, TimeFormat, Config, VerdictRepresentation, Total, Value, Incremental};
    use rtlola_frontend::mir::StreamReference;
    use std::time::{Instant, Duration};
    use crate::coordination::monitor::StreamState;
    use ordered_float::NotNan;

    fn setup<V: VerdictRepresentation>(spec: &str) -> (Instant, Monitor<V>) {
        // Init Monitor API
        let config = rtlola_frontend::ParserConfig::for_string(spec.to_string());
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        let eval_conf = EvalConfig::api(TimeRepresentation::Absolute(TimeFormat::FloatSecs));
        (Instant::now(), Config::new_api(eval_conf, ir).as_api())
    }

    #[test]
    fn test_const_output_literals() {
        use Value::*;
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
        let v = Unsigned(3);
        let res = monitor.accept_event(vec![v.clone()], start.elapsed());
        assert!(res.timed.is_empty());
        let res = res.event;
        assert_eq!(res.inputs[0], Some(v));
        assert_eq!(res.outputs[0], Some(StreamState::NonParameterized(Bool(true))));
        assert_eq!(res.outputs[1], Some(StreamState::NonParameterized(Unsigned(3))));
        assert_eq!(res.outputs[2], Some(StreamState::NonParameterized(Signed(-5))));
        assert_eq!(res.outputs[3], Some(StreamState::NonParameterized(Value::new_float(-123.456))));
        assert_eq!(res.outputs[4], Some(StreamState::NonParameterized(Str("foobar".into()))));
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
        assert!(res.timed.iter().all(|(time, change)| time.as_secs() % 4 == 0 && change[0].0 == 0 && change[0].1 == StreamState::NonParameterized(Value::Unsigned(0))));
        // for v in 2..=n {
        //     let res = monitor.accept_event(vec![Value::Unsigned(v)], time);
        //     time += Duration::from_secs(1);
        // }
        // time += Duration::from_secs(1);
        // // 71 secs have passed. All values should be within the window.
        // eval_stream_timed!(eval, 0, vec![], time);
        // let expected = Unsigned(n);
        // assert_eq!(eval.peek_value(out_ref, &Vec::new(), 0).unwrap(), expected);
    }

}
