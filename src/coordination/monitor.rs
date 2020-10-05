use crate::basics::{EvalConfig, OutputHandler, Time};
use crate::coordination::Event;
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::storage::Value;
use rtlola_frontend::ir::{Deadline, InputReference, OutputReference, RTLolaIR};
use std::sync::Arc;
use std::time::Duration;

pub type StateSlice = Vec<(OutputReference, Value)>;

#[derive(Debug)]
pub struct Update {
    pub timed: Vec<(Time, StateSlice)>,
    pub event: StateSlice,
}
#[rustfmt::skip]
/**
The `Monitor` accepts new events and computes streams.

The `Monitor` is the central object exposed by the API.  
It can compute event-based streams based on new events through `accept_event`.  
It can also simply advance periodic streams up to a given timestamp through `accept_time`.  
*/
#[allow(missing_debug_implementations)]
pub struct Monitor {
    ir: RTLolaIR, // probably not necessary to store here.
    eval: Evaluator,
    pub(crate) output_handler: Arc<OutputHandler>,
    deadlines: Vec<Deadline>,
    next_dl: Option<Duration>,
    dl_ix: usize,
}

// Crate-public interface
impl Monitor {
    pub(crate) fn setup(ir: RTLolaIR, output_handler: Arc<OutputHandler>, config: EvalConfig) -> Monitor {
        // Note: start_time only accessed in online mode.
        let eval_data = EvaluatorData::new(ir.clone(), config.clone(), output_handler.clone(), None);

        let deadlines: Vec<Deadline> = if ir.time_driven.is_empty() {
            vec![]
        } else {
            ir.compute_schedule().expect("Creation of schedule failed.").deadlines
        };

        Monitor { ir, eval: eval_data.into_evaluator(), output_handler, deadlines, next_dl: None, dl_ix: 0 }
    }
}

// Public interface
impl Monitor {
    /**
    Computes all periodic streams up through the new timestamp and then handles the input event.

    The new event is therefore not seen by periodic streams up through the new timestamp.
    */
    pub fn accept_event<E: Into<Event>>(&mut self, ev: E, ts: Time) -> Update {
        let ev = ev.into();
        self.output_handler.debug(|| format!("Accepted {:?}.", ev));

        let timed = self.accept_time(ts);

        // Evaluate
        self.output_handler.new_event();
        self.eval.eval_event(ev.as_slice(), ts);
        let event_change = self.eval.peek_fresh();

        Update { timed, event: event_change }
    }

    /**
    Computes all periodic streams up through the new timestamp.

    */
    pub fn accept_time(&mut self, ts: Time) -> Vec<(Time, StateSlice)> {
        if self.deadlines.is_empty() {
            return vec![];
        }
        assert!(self.deadlines.len() > 0);

        if self.next_dl.is_none() {
            assert_eq!(self.dl_ix, 0);
            self.next_dl = Some(ts + self.deadlines[0].pause);
        }

        let mut next_deadline = self.next_dl.clone().expect("monitor lacks start time");
        let mut timed_changes: Vec<(Time, StateSlice)> = vec![];

        while ts > next_deadline {
            // Go back in time and evaluate,...
            let dl = &self.deadlines[self.dl_ix];
            self.output_handler.debug(|| format!("Schedule Timed-Event {:?}.", (&dl.due, next_deadline)));
            self.output_handler.new_event();
            self.eval.eval_time_driven_outputs(&dl.due, next_deadline);
            self.dl_ix = (self.dl_ix + 1) % self.deadlines.len();
            timed_changes.push((next_deadline.clone(), self.eval.peek_fresh()));
            let dl = &self.deadlines[self.dl_ix];
            assert!(dl.pause > Duration::from_secs(0));
            next_deadline += dl.pause;
        }
        self.next_dl = Some(next_deadline);
        timed_changes
    }

    /**
    Get the name of an input stream based on its `InputReference`

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_input(&self, id: InputReference) -> &str {
        self.ir.inputs[id].name.as_str()
    }

    /**
    Get the name of an output stream based on its `OutputReference`

    The reference is valid for the lifetime of the monitor.
    */
    pub fn name_for_output(&self, id: OutputReference) -> &str {
        self.ir.outputs[id].name.as_str()
    }
}
