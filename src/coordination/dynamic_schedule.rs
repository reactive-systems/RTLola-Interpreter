use crate::coordination::EvaluationTask;
use crate::{Time, Value};
use rtlola_frontend::mir::OutputReference;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
/// A struct representing a scheduled task
pub(crate) struct ScheduledTask<'a> {
    /// The task to be executed
    task: EvaluationTask<'a>,
    /// The point in time when it should be executed, measured as the the duration relative to the start time of the monitor.
    due: Time,
    /// The period of the task. Used to reschedule this task when popped.
    period: Duration,
}

impl PartialOrd for ScheduledTask<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for ScheduledTask<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.due.cmp(&other.due).reverse() {
            Ordering::Equal => match self.period.cmp(&other.period).reverse() {
                Ordering::Equal => self.task.cmp(&other.task).reverse(),
                res => res,
            },
            res => res,
        }
    }
}

pub(crate) struct DynamicSchedule<'a> {
    queue: BinaryHeap<ScheduledTask<'a>>,
}

impl DynamicSchedule<'_> {
    pub(crate) fn new() -> Self {
        DynamicSchedule { queue: BinaryHeap::new() }
    }

    /// creates a new Task in the Schedule
    pub(crate) fn schedule_close(&mut self, target: OutputReference, now: Time, period: Duration) {}

    /// Schedule an instance for evaluation
    pub(crate) fn schedule_instance(
        &mut self,
        target: OutputReference,
        parameter: &[Value],
        now: Time,
        period: Duration,
    ) {
    }

    /// Removes a scheduled instance from the schedule
    pub(crate) fn remove_instance(&mut self, target: OutputReference, parameter: &[Value]) {}
}
