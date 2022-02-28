use crate::coordination::EvaluationTask;
use crate::Time;
use crate::Value;
use priority_queue::PriorityQueue;
use rtlola_frontend::mir::OutputReference;
use rtlola_frontend::RtLolaMir;
use std::cmp::Reverse;
use std::sync::Condvar;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// A struct representing a scheduled task
pub(crate) struct ScheduledTask {
    /// The task to be executed
    task: EvaluationTask,
    /// The period of the task. Used to reschedule this task when popped.
    period: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicDeadline {
    /// Relative to the start of the monitor
    pub(crate) due: Time,
    pub(crate) tasks: Vec<EvaluationTask>,
}

impl DynamicDeadline {
    pub(crate) fn sort(&mut self, ir: &RtLolaMir) {
        self.tasks.sort_by_key(|s| s.get_sort_key(ir));
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicSchedule {
    queue: PriorityQueue<ScheduledTask, Reverse<Time>>,
}

impl DynamicSchedule {
    pub(crate) fn new() -> Self {
        DynamicSchedule { queue: PriorityQueue::new() }
    }

    /// Schedule the evaluation of stream or of an instance if parameters are given.
    pub(crate) fn schedule_evaluation(
        &mut self,
        schedule_changed: &Condvar,
        target: OutputReference,
        parameter: &[Value],
        now: Time,
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Evaluate(target, parameter.to_vec()), period };
        self.queue.push(task, Reverse(now + period));
        schedule_changed.notify_all();
    }

    /// Schedule the close evaluation of a stream or of an instance if parameters are given.
    pub(crate) fn schedule_close(
        &mut self,
        schedule_changed: &Condvar,
        target: OutputReference,
        parameter: &[Value],
        now: Time,
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Close(target, parameter.to_vec()), period };
        self.queue.push(task, Reverse(now + period));
        schedule_changed.notify_all();
    }

    /// Removes a scheduled evaluation from the schedule
    pub(crate) fn remove_evaluation(
        &mut self,
        schedule_changed: &Condvar,
        target: OutputReference,
        parameter: &[Value],
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Evaluate(target, parameter.to_vec()), period };
        self.queue.remove(&task);
        schedule_changed.notify_all();
    }

    /// Removes a scheduled close from the schedule
    pub(crate) fn remove_close(
        &mut self,
        schedule_changed: &Condvar,
        target: OutputReference,
        parameter: &[Value],
        period: Duration,
    ) {
        let task = ScheduledTask { task: EvaluationTask::Close(target, parameter.to_vec()), period };
        self.queue.remove(&task);
        schedule_changed.notify_all();
    }

    /// Returns the next scheduled task until and including the given time
    pub(crate) fn get_next_deadline(&mut self, now: Time) -> Option<DynamicDeadline> {
        if self.queue.peek().is_none() || self.queue.peek().unwrap().1 .0 > now {
            return None;
        }
        let (task, task_due) = self.queue.pop().unwrap();
        // Reschedule Task
        self.queue.push(task.clone(), Reverse(task_due.0 + task.period));

        let mut deadlines: Vec<EvaluationTask> = vec![task.task];
        let due = task_due.0;

        // Pop all tasks that are due at the same time
        while self.queue.peek().is_some() && self.queue.peek().unwrap().1 .0 == due {
            let (task, task_due) = self.queue.pop().unwrap();
            // Reschedule Task
            self.queue.push(task.clone(), Reverse(task_due.0 + task.period));
            deadlines.push(task.task);
        }

        Some(DynamicDeadline { due, tasks: deadlines })
    }

    /// Return the time when the next deadline is due or None if there is no next deadline
    pub(crate) fn get_next_deadline_due(&self) -> Option<Time> {
        self.queue.peek().map(|(_, due)| due.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::coordination::dynamic_schedule::DynamicSchedule;
    use crate::coordination::EvaluationTask;
    use crate::Value;
    use std::sync::Condvar;
    use std::time::Duration;

    #[test]
    fn test_reschedule() {
        let cond = Condvar::new();
        let mut schedule = DynamicSchedule::new();
        let now = Duration::default();
        schedule.schedule_evaluation(&cond, 0, &[], now, Duration::from_secs(5));
        schedule.schedule_close(&cond, 1, &[], now, Duration::from_secs(2));
        schedule.schedule_evaluation(&cond, 2, &[], now, Duration::from_secs(7));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(2));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(4));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(5));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(0, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(6));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(7));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(2, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(8));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(10));
        assert!(res.tasks.contains(&EvaluationTask::Close(1, vec![])));
        assert!(res.tasks.contains(&EvaluationTask::Evaluate(0, vec![])));
    }

    #[test]
    fn test_unschedule() {
        let cond = Condvar::new();
        let mut schedule = DynamicSchedule::new();
        let now = Duration::default();
        schedule.schedule_evaluation(&cond, 0, &[], now, Duration::from_secs(5));
        schedule.schedule_close(&cond, 1, &[], now, Duration::from_secs(2));
        schedule.schedule_evaluation(&cond, 2, &[], now, Duration::from_secs(7));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(2));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(4));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        schedule.remove_close(&cond, 1, &[], Duration::from_secs(2));
        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(5));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(0, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(7));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(2, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(10));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(0, vec![])]);

        schedule.remove_evaluation(&cond, 0, &[], Duration::from_secs(5));
        let res = schedule.get_next_deadline(Duration::from_secs(20)).unwrap();
        assert_eq!(res.due, Duration::from_secs(14));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(2, vec![])]);
        assert!(schedule.get_next_deadline(Duration::from_secs(20)).is_none());

        let res = schedule.get_next_deadline(Duration::from_secs(30)).unwrap();
        assert_eq!(res.due, Duration::from_secs(21));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(2, vec![])]);
        schedule.remove_evaluation(&cond, 2, &[], Duration::from_secs(7));
        assert!(schedule.get_next_deadline(Duration::from_secs(50)).is_none());
    }

    #[test]
    fn test_involved() {
        let cond = Condvar::new();
        let mut schedule = DynamicSchedule::new();
        let now = Duration::default();
        schedule.schedule_evaluation(&cond, 0, &[], now, Duration::from_secs(5));
        schedule.schedule_close(&cond, 1, &[], now, Duration::from_secs(2));
        schedule.schedule_evaluation(&cond, 2, &[], now, Duration::from_secs(7));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(2));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(4));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let para = vec![Value::Bool(true), Value::Signed(42)];
        schedule.schedule_evaluation(&cond, 3, &para, Duration::from_secs(4), Duration::from_secs(1));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(5));
        assert!(res.tasks.contains(&EvaluationTask::Evaluate(0, vec![])));
        assert!(res.tasks.contains(&EvaluationTask::Evaluate(3, para.clone())));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(6));
        assert!(res.tasks.contains(&EvaluationTask::Close(1, vec![])));
        assert!(res.tasks.contains(&EvaluationTask::Evaluate(3, para.clone())));

        schedule.remove_evaluation(&cond, 3, &para, Duration::from_secs(1));

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(7));
        assert_eq!(res.tasks, vec![EvaluationTask::Evaluate(2, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(8));
        assert_eq!(res.tasks, vec![EvaluationTask::Close(1, vec![])]);

        let res = schedule.get_next_deadline(Duration::from_secs(10)).unwrap();
        assert_eq!(res.due, Duration::from_secs(10));
        assert!(res.tasks.contains(&EvaluationTask::Close(1, vec![])));
        assert!(res.tasks.contains(&EvaluationTask::Evaluate(0, vec![])));
    }
}
