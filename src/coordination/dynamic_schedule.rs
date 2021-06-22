use crate::coordination::EvaluationTask;
use crate::{Time, Value};
use priority_queue::PriorityQueue;
use rtlola_frontend::mir::{OutputReference, Schedule};
use std::cmp::Reverse;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// A struct representing a scheduled task
pub(crate) struct ScheduledTask<'a> {
    /// The task to be executed
    task: EvaluationTask<'a>,
    /// The period of the task. Used to reschedule this task when popped.
    period: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduleSlice<'a>(Vec<(Time, Vec<EvaluationTask<'a>>)>);

impl<'a> ScheduleSlice<'a> {
    // Todo: Impl sort tasks in same deadline by eval order
    // Todo: Impl remove task from slice
}

pub(crate) struct DynamicSchedule<'a> {
    queue: PriorityQueue<ScheduledTask<'a>, Reverse<Time>>,
}

impl<'a> DynamicSchedule<'a> {
    pub(crate) fn new() -> Self {
        DynamicSchedule { queue: PriorityQueue::new() }
    }

    /// Schedule the evaluation of stream or of an instance if parameters are given.
    pub(crate) fn schedule_evaluation(
        &mut self,
        target: OutputReference,
        parameter: &'a [Value],
        now: Time,
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Evaluate(target, parameter), period };
        self.queue.push(task, Reverse(now + period));
    }

    /// Schedule the close evaluation of a stream or of an instance if parameters are given.
    pub(crate) fn schedule_close(
        &mut self,
        target: OutputReference,
        parameter: &'a [Value],
        now: Time,
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Close(target, parameter), period };
        self.queue.push(task, Reverse(now + period));
    }

    /// Removes a scheduled evaluation from the schedule
    pub(crate) fn remove_evaluation(&mut self, target: OutputReference, parameter: &'a [Value], period: Duration) {
        let task = ScheduledTask { task: EvaluationTask::Evaluate(target, parameter), period };
        self.queue.remove(&task);
    }

    /// Removes a scheduled close from the schedule
    pub(crate) fn remove_close(&mut self, target: OutputReference, parameter: &'a [Value], period: Duration) {
        let task = ScheduledTask { task: EvaluationTask::Close(target, parameter), period };
        self.queue.remove(&task);
    }

    /// Returns all scheduled task until and including the given time
    pub(crate) fn get_schedule_until(&mut self, now: Time) -> ScheduleSlice<'a> {
        let mut res: Vec<(Time, Vec<EvaluationTask>)> = vec![];
        while self.queue.peek().is_some() && self.queue.peek().unwrap().1 .0 <= now {
            let (t, due) = self.queue.pop().unwrap();
            // Reschedule Task
            self.queue.push(t.clone(), Reverse(due.0 + t.period));
            if res.last().is_some() && res.last().unwrap().0 == due.0 {
                res.last_mut().unwrap().1.push(t.task);
            } else {
                res.push((due.0, vec![t.task]));
            }
        }
        ScheduleSlice(res)
    }
}

#[cfg(test)]
mod tests {
    use crate::coordination::dynamic_schedule::DynamicSchedule;
    use crate::coordination::EvaluationTask;
    use std::time::Duration;

    #[test]
    fn test_reschedule() {
        let mut schedule = DynamicSchedule::new();
        let now = Duration::default();
        schedule.schedule_evaluation(0, &[], now, Duration::from_secs(5));
        schedule.schedule_close(1, &[], now, Duration::from_secs(2));
        schedule.schedule_evaluation(2, &[], now, Duration::from_secs(7));

        let res = schedule.get_schedule_until(Duration::from_secs(10));

        assert_eq!(res.0[0].0, Duration::from_secs(2));
        assert_eq!(res.0[0].1, vec![EvaluationTask::Close(1, &[])]);

        assert_eq!(res.0[1].0, Duration::from_secs(4));
        assert_eq!(res.0[1].1, vec![EvaluationTask::Close(1, &[])]);

        assert_eq!(res.0[2].0, Duration::from_secs(5));
        assert_eq!(res.0[2].1, vec![EvaluationTask::Evaluate(0, &[])]);

        assert_eq!(res.0[3].0, Duration::from_secs(6));
        assert_eq!(res.0[3].1, vec![EvaluationTask::Close(1, &[])]);

        assert_eq!(res.0[4].0, Duration::from_secs(7));
        assert_eq!(res.0[4].1, vec![EvaluationTask::Evaluate(2, &[])]);

        assert_eq!(res.0[5].0, Duration::from_secs(8));
        assert_eq!(res.0[5].1, vec![EvaluationTask::Close(1, &[])]);

        assert_eq!(res.0[6].0, Duration::from_secs(10));
        assert!(res.0[6].1.contains(&EvaluationTask::Close(1, &[])));
        assert!(res.0[6].1.contains(&EvaluationTask::Evaluate(0, &[])));
    }

    #[test]
    fn test_unschedule() {
        let mut schedule = DynamicSchedule::new();
        let now = Duration::default();
        schedule.schedule_evaluation(0, &[], now, Duration::from_secs(5));
        schedule.schedule_close(1, &[], now, Duration::from_secs(2));
        schedule.schedule_evaluation(2, &[], now, Duration::from_secs(7));

        let res = schedule.get_schedule_until(Duration::from_secs(4));

        assert_eq!(res.0[0].0, Duration::from_secs(2));
        assert_eq!(res.0[0].1, vec![EvaluationTask::Close(1, &[])]);

        assert_eq!(res.0[1].0, Duration::from_secs(4));
        assert_eq!(res.0[1].1, vec![EvaluationTask::Close(1, &[])]);

        schedule.remove_close(1, &[], Duration::from_secs(2));
        let res = schedule.get_schedule_until(Duration::from_secs(10));

        assert_eq!(res.0[0].0, Duration::from_secs(5));
        assert_eq!(res.0[0].1, vec![EvaluationTask::Evaluate(0, &[])]);

        assert_eq!(res.0[1].0, Duration::from_secs(7));
        assert_eq!(res.0[1].1, vec![EvaluationTask::Evaluate(2, &[])]);

        assert_eq!(res.0[2].0, Duration::from_secs(10));
        assert_eq!(res.0[2].1, vec![EvaluationTask::Evaluate(0, &[])]);
    }
}
