use crate::basics::{EvalConfig, OutputHandler, Time};
use crate::coordination::{Event, EvaluationTask};
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::storage::Value;
use rtlola_frontend::mir::{Deadline, InputReference, OutputReference, RtLolaMir, Task, Type};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

/**
    Provides the functionality to generate a snapshot of the streams values.
*/
pub trait VerdictRepresentation {
    /// Creates a snapshot of the streams values.
    fn create(mon: &Monitor<Self>) -> Self
    where
        Self: Sized;
}

/**
    Represents a snapshot of the monitor containing the value of all updated output stream.
*/
pub type Incremental = Vec<(OutputReference, Value)>;

impl VerdictRepresentation for Incremental {
    fn create(mon: &Monitor<Incremental>) -> Self {
        mon.eval.peek_fresh()
    }
}

/**
    Represents a snapshot of the monitor containing the current value of each output stream.

    The ith value in the vector is the current value of the ith input or output stream.
*/
#[derive(Debug)]
pub struct Total {
    pub inputs: Vec<Option<Value>>,
    pub outputs: Vec<Option<Value>>,
}

impl VerdictRepresentation for Total {
    fn create(mon: &Monitor<Total>) -> Self {
        Total { inputs: mon.eval.peek_inputs(), outputs: mon.eval.peek_outputs() }
    }
}

/**
    Represents the index and the formated message of all violated triggers.
*/
pub type TriggerMessages = Vec<(OutputReference, String)>;

impl VerdictRepresentation for TriggerMessages {
    fn create(mon: &Monitor<Self>) -> Self
    where
        Self: Sized,
    {
        let violated_trigger = mon.eval.peek_violated_triggers();
        violated_trigger.into_iter().map(|sr| (sr, mon.eval.format_trigger_message(sr))).collect()
    }
}

/**
    Represents the index and the info values of all violated triggers.
*/
pub type TriggersWithInfoValues = Vec<(OutputReference, Vec<Option<Value>>)>;

impl VerdictRepresentation for TriggersWithInfoValues {
    fn create(mon: &Monitor<Self>) -> Self
    where
        Self: Sized,
    {
        let violated_trigger = mon.eval.peek_violated_triggers();
        violated_trigger.into_iter().map(|sr| (sr, mon.eval.peek_info_stream_values(sr))).collect()
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
    deadlines: Vec<Deadline>,
    next_dl: Option<Duration>,
    dl_ix: usize,
    phantom: PhantomData<V>,
}

/// Crate-public interface
impl<V: VerdictRepresentation> Monitor<V> {
    ///setup
    pub(crate) fn setup(ir: RtLolaMir, output_handler: Arc<OutputHandler>, config: EvalConfig) -> Monitor<V> {
        // Note: start_time only accessed in online mode.
        let eval_data = EvaluatorData::new(ir.clone(), config, output_handler.clone(), None);

        let deadlines: Vec<Deadline> = if ir.time_driven.is_empty() {
            vec![]
        } else {
            ir.compute_schedule().expect("Creation of schedule failed.").deadlines
        };

        Monitor {
            ir,
            eval: eval_data.into_evaluator(),
            output_handler,
            deadlines,
            next_dl: None,
            dl_ix: 0,
            phantom: PhantomData,
        }
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

        let timed = self.accept_time(ts);

        // Evaluate
        self.output_handler.new_event();
        self.eval.eval_event(ev.as_slice(), ts);
        let event_change = V::create(self);

        Verdicts::<V> { timed, event: event_change }
    }

    /**
    Computes all periodic streams up through the new timestamp.

    */
    pub fn accept_time(&mut self, ts: Time) -> Vec<(Time, V)> {
        if self.deadlines.is_empty() {
            return vec![];
        }
        assert!(!self.deadlines.is_empty());

        if self.next_dl.is_none() {
            assert_eq!(self.dl_ix, 0);
            self.next_dl = Some(ts + self.deadlines[0].pause);
        }

        let mut next_deadline = self.next_dl.clone().expect("monitor lacks start time");
        let mut timed_changes: Vec<(Time, V)> = vec![];

        while ts > next_deadline {
            // Go back in time and evaluate,...
            let dl = &self.deadlines[self.dl_ix];
            self.output_handler.debug(|| format!("Schedule Timed-Event {:?}.", (&dl.due, next_deadline)));
            self.output_handler.new_event();
            let eval_tasks: Vec<EvaluationTask<'_>> = dl.due.iter().map(|t| (*t).into()).collect();
            self.eval.eval_time_driven_tasks(eval_tasks, next_deadline);
            self.dl_ix = (self.dl_ix + 1) % self.deadlines.len();
            timed_changes.push((next_deadline, V::create(self)));
            let dl = &self.deadlines[self.dl_ix];
            assert!(dl.pause > Duration::from_secs(0));
            next_deadline += dl.pause;
        }
        self.next_dl = Some(next_deadline);
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
        let Monitor { ir, eval, output_handler, deadlines, next_dl, dl_ix, phantom: _ } = self;
        Monitor::<T> { ir, eval, output_handler, deadlines, next_dl, dl_ix, phantom: PhantomData }
    }
}
