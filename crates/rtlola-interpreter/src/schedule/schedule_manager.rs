use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::time::Duration;

use rtlola_frontend::mir::{Deadline, OutputReference, RtLolaMir, Stream, Task};

use crate::schedule::dynamic_schedule::DynamicSchedule;
use crate::{Time, Value};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EvaluationTask {
    /// Evaluate a specific instance of the output stream.
    Evaluate(OutputReference, Vec<Value>),
    /// Evalutate all instances of the output stream.
    EvaluateInstances(OutputReference),
    /// Spawn a new instance of the output stream.
    Spawn(OutputReference),
    /// Evaluate the close condition for a specific instance.
    Close(OutputReference, Vec<Value>),
}

impl From<Task> for EvaluationTask {
    fn from(task: Task) -> Self {
        match task {
            Task::Evaluate(idx) => EvaluationTask::EvaluateInstances(idx),
            Task::Spawn(idx) => EvaluationTask::Spawn(idx),
            Task::Close(idx) => EvaluationTask::Close(idx, vec![]),
        }
    }
}

impl EvaluationTask {
    pub(crate) fn get_sort_key(&self, ir: &RtLolaMir) -> usize {
        match self {
            EvaluationTask::Evaluate(idx, _) | EvaluationTask::EvaluateInstances(idx) => {
                ir.outputs[*idx].eval_layer().inner()
            },
            EvaluationTask::Spawn(idx) => ir.outputs[*idx].spawn_layer().inner(),
            EvaluationTask::Close(_, _) => usize::MAX,
        }
    }
}

/// A structure to combine the static and dynamic schedule.
pub(crate) struct ScheduleManager {
    ir: RtLolaMir,
    has_time_driven: bool,
    deadlines: Vec<Deadline>,
    dyn_schedule: Rc<RefCell<DynamicSchedule>>,
    cur_static_deadline_idx: usize,
    next_static_deadline: Option<Time>,
    static_due_streams: Vec<Task>,
}

impl ScheduleManager {
    /// Creates a new TimeDrivenManager managing time-driven output streams.
    pub(crate) fn setup(ir: RtLolaMir, dyn_schedule: Rc<RefCell<DynamicSchedule>>) -> Result<ScheduleManager, String> {
        let contains_time_driven = ir.has_time_driven_features();
        if !contains_time_driven {
            // return dummy
            return Ok(ScheduleManager {
                ir,
                has_time_driven: false,
                deadlines: vec![],
                dyn_schedule,
                cur_static_deadline_idx: 0,
                next_static_deadline: None,
                static_due_streams: vec![],
            });
        }

        let schedule = ir.compute_schedule()?;
        let (due_streams, next_static_deadline) = match schedule.deadlines.first() {
            None => (vec![], None),
            Some(deadline) => (deadline.due.clone(), Some(deadline.pause)),
        };
        let cur_deadline_idx = 0;

        Ok(ScheduleManager {
            ir,
            has_time_driven: true,
            deadlines: schedule.deadlines,
            dyn_schedule,
            cur_static_deadline_idx: cur_deadline_idx,
            next_static_deadline,
            static_due_streams: due_streams,
        })
    }

    pub(crate) fn get_next_due(&self) -> Option<Time> {
        let dd = self.dyn_schedule.borrow().get_next_deadline_due();
        match (self.next_static_deadline, dd) {
            (None, None) => None,
            (Some(x), None) | (None, Some(x)) => Some(x),
            (Some(sd), Some(dd)) => Some(sd.min(dd)),
        }
    }

    fn get_next_static_deadline(&mut self) -> Vec<EvaluationTask> {
        debug_assert!(!self.static_due_streams.is_empty() && self.next_static_deadline.is_some());
        let res = self.static_due_streams.iter().map(|t| (*t).into()).collect();
        self.cur_static_deadline_idx = (self.cur_static_deadline_idx + 1) % self.deadlines.len();
        let deadline = &self.deadlines[self.cur_static_deadline_idx];
        assert!(deadline.pause > Duration::from_secs(0));
        self.next_static_deadline = self.next_static_deadline.map(|d| d + deadline.pause);
        self.static_due_streams.clone_from(&deadline.due);
        res
    }

    pub(crate) fn get_next_deadline(&mut self, now: Time) -> Vec<EvaluationTask> {
        debug_assert!(self.has_time_driven);

        let static_due = self.next_static_deadline;
        let dyn_due = self.dyn_schedule.borrow().get_next_deadline_due();

        match (static_due, dyn_due) {
            (None, None) => vec![],
            (None, Some(_)) => {
                let mut dyn_deadline = (*self.dyn_schedule)
                    .borrow_mut()
                    .get_next_deadline(now)
                    .expect("Should not happen when there is a dynamic due time.");
                dyn_deadline.sort(&self.ir);
                dyn_deadline.tasks
            },
            (Some(_), None) => self.get_next_static_deadline(),
            (Some(sd), Some(dd)) => {
                match sd.cmp(&dd) {
                    Ordering::Less => self.get_next_static_deadline(),
                    Ordering::Equal => {
                        let static_deadline = self.get_next_static_deadline();
                        let dyn_deadline = (*self.dyn_schedule).borrow_mut().get_next_deadline(now).unwrap().tasks;
                        let mut res = static_deadline
                            .into_iter()
                            .chain(dyn_deadline)
                            .collect::<Vec<EvaluationTask>>();
                        res.sort_by_key(|t| t.get_sort_key(&self.ir));
                        res
                    },
                    Ordering::Greater => {
                        let mut dyn_deadline = (*self.dyn_schedule)
                            .borrow_mut()
                            .get_next_deadline(now)
                            .expect("Should not happen when there is a dynamic due time.");
                        dyn_deadline.sort(&self.ir);
                        dyn_deadline.tasks
                    },
                }
            },
        }
    }
}
