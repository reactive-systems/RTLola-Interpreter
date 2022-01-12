use super::WorkItem;
use crate::basics::{OutputHandler, Time};

use crate::coordination::dynamic_schedule::DynamicSchedule;
use crate::evaluator::Evaluator;
use crate::Value;
use crossbeam_channel::Sender;
use rtlola_frontend::mir::{Deadline, OutputReference, PacingType, RtLolaMir, Stream, Task};
use std::cmp::Ordering;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EvaluationTask {
    /// Evaluate a specific instance of the output stream.
    Evaluate(OutputReference, Vec<Value>),
    /// Spawn a new instance of the output stream.
    Spawn(OutputReference),
    /// Evaluate the close condition for a specific instance.
    Close(OutputReference, Vec<Value>),
}

impl From<Task> for EvaluationTask {
    fn from(task: Task) -> Self {
        match task {
            Task::Evaluate(idx) => EvaluationTask::Evaluate(idx, vec![]),
            Task::Spawn(idx) => EvaluationTask::Spawn(idx),
            Task::Close(idx) => EvaluationTask::Close(idx, vec![]),
        }
    }
}

impl EvaluationTask {
    pub(crate) fn get_sort_key(&self, ir: &RtLolaMir) -> usize {
        match self {
            EvaluationTask::Evaluate(idx, _) => ir.outputs[*idx].eval_layer().inner(),
            EvaluationTask::Spawn(idx) => ir.outputs[*idx].spawn_layer().inner(),
            EvaluationTask::Close(_, _) => usize::MAX,
        }
    }
}

pub(crate) type TimeEvaluation = Vec<EvaluationTask>;

pub(crate) struct TimeDrivenManager {
    ir: RtLolaMir,
    has_time_driven: bool,
    deadlines: Vec<Deadline>,
    dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
    handler: Arc<OutputHandler>,
    cur_static_deadline_idx: usize,
    next_static_deadline: Option<Time>,
    static_due_streams: Vec<Task>,
}

impl TimeDrivenManager {
    /// Creates a new TimeDrivenManager managing time-driven output streams.
    pub(crate) fn setup(
        ir: RtLolaMir,
        handler: Arc<OutputHandler>,
        dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
    ) -> Result<TimeDrivenManager, String> {
        let contains_time_driven = ir.has_time_driven_features();
        if !contains_time_driven {
            // return dummy
            return Ok(TimeDrivenManager {
                ir,
                has_time_driven: false,
                deadlines: vec![],
                dyn_schedule,
                handler,
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

        Ok(TimeDrivenManager {
            ir,
            has_time_driven: true,
            deadlines: schedule.deadlines,
            dyn_schedule,
            handler,
            cur_static_deadline_idx: cur_deadline_idx,
            next_static_deadline,
            static_due_streams: due_streams,
        })
    }
    #[allow(dead_code)]
    pub(crate) fn get_next_due(&self) -> Option<Time> {
        self.get_next_due_locked(&self.dyn_schedule.0.lock().unwrap())
    }

    pub(crate) fn get_next_due_locked(&self, dyn_schedule: &MutexGuard<DynamicSchedule>) -> Option<Time> {
        let dd = dyn_schedule.get_next_deadline_due();
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
        self.static_due_streams = deadline.due.clone();
        res
    }

    #[allow(dead_code)]
    pub(crate) fn get_next_deadline(&mut self, now: Time) -> Vec<EvaluationTask> {
        let schedule_copy = self.dyn_schedule.clone();
        let mut lock = schedule_copy.0.lock().unwrap();
        self.get_next_deadline_locked(now, &mut lock)
    }

    pub(crate) fn get_next_deadline_locked(
        &mut self,
        now: Time,
        dyn_schedule: &mut MutexGuard<DynamicSchedule>,
    ) -> Vec<EvaluationTask> {
        debug_assert!(self.has_time_driven);

        let static_due = self.next_static_deadline;
        let dyn_due = dyn_schedule.get_next_deadline_due();

        match (static_due, dyn_due) {
            (None, None) => vec![],
            (None, Some(_)) => {
                let mut dyn_deadline =
                    dyn_schedule.get_next_deadline(now).expect("Should not happen when there is a dynamic due time.");
                dyn_deadline.sort(&self.ir);
                dyn_deadline.tasks
            }
            (Some(_), None) => self.get_next_static_deadline(),
            (Some(sd), Some(dd)) => match sd.cmp(&dd) {
                Ordering::Less => self.get_next_static_deadline(),
                Ordering::Equal => {
                    let static_deadline = self.get_next_static_deadline();
                    let dyn_deadline = dyn_schedule.get_next_deadline(now).unwrap().tasks;
                    let mut res =
                        static_deadline.into_iter().chain(dyn_deadline.into_iter()).collect::<Vec<EvaluationTask>>();
                    res.sort_by_key(|t| t.get_sort_key(&self.ir));
                    res
                }
                Ordering::Greater => {
                    let mut dyn_deadline = dyn_schedule
                        .get_next_deadline(now)
                        .expect("Should not happen when there is a dynamic due time.");
                    dyn_deadline.sort(&self.ir);
                    dyn_deadline.tasks
                }
            },
        }
    }

    pub(crate) fn start_online(mut self, start_time: SystemTime, work_chan: Sender<WorkItem>) -> ! {
        debug_assert!(self.has_time_driven);
        let now = SystemTime::now();

        // Shift next static deadline relative to start time
        let time = now.duration_since(start_time).expect("System Time did not behave monotonically");
        self.next_static_deadline = self.next_static_deadline.map(|d| d + time);

        let schedule_copy = self.dyn_schedule.clone();

        loop {
            let mut schedule = schedule_copy.0.lock().unwrap();
            let mut opt_due_time = self.get_next_due_locked(&schedule);

            // Wait until there is a deadline
            while opt_due_time.is_none() {
                schedule = self.dyn_schedule.1.wait(schedule).unwrap();
                opt_due_time = self.get_next_due_locked(&schedule);
            }
            let mut due_time = opt_due_time.unwrap();

            let now = SystemTime::now();
            let mut time = now.duration_since(start_time).expect("System Time did not behave monotonically");

            // Wait for next deadline or until schedule changes
            while time < due_time {
                let wait_time = due_time - time;

                //Todo: This wait is inaccurate.

                let (new_schedule, _) = self.dyn_schedule.1.wait_timeout(schedule, wait_time).unwrap();
                schedule = new_schedule;
                //Note: spurious wake-ups should not be a problem

                let now = SystemTime::now();
                time = now.duration_since(start_time).expect("System Time did not behave monotonically");

                let mut opt_due_time = self.get_next_due_locked(&schedule);

                // Wait until there is a deadline
                while opt_due_time.is_none() {
                    schedule = self.dyn_schedule.1.wait(schedule).unwrap();
                    opt_due_time = self.get_next_due_locked(&schedule);
                }
                due_time = opt_due_time.unwrap();
            }

            let eval_tasks: Vec<EvaluationTask> = self.get_next_deadline_locked(due_time, &mut schedule);

            let item = WorkItem::Time(eval_tasks, due_time);
            if work_chan.send(item).is_err() {
                self.handler.runtime_warning(|| "TDM: Sending failed; evaluation cycle lost.");
            }
        }
    }

    /// Evaluates all deadlines due before time `ts`
    pub(crate) fn accept_time_offline(&mut self, evaluator: &mut Evaluator, ts: Time) {
        self.accept_time_offline_with_callback(evaluator, ts, |_, _| ());
    }

    /// Evaluates all deadlines due before time `ts` and calls the callback after the evaluation of each deadline
    pub(crate) fn accept_time_offline_with_callback<T>(&mut self, evaluator: &mut Evaluator, ts: Time, mut callback: T)
    where
        T: FnMut(Time, &Evaluator),
    {
        if !self.has_time_driven {
            return;
        }
        let schedule_copy = self.dyn_schedule.clone();
        let mut schedule = schedule_copy.0.lock().unwrap();
        while self.get_next_due_locked(&schedule).is_some() {
            let due = self.get_next_due_locked(&schedule).unwrap();
            if due >= ts {
                break;
            }
            let deadline = self.get_next_deadline_locked(ts, &mut schedule);
            drop(schedule);
            self.eval_deadline(evaluator, deadline, due);
            callback(due, evaluator);
            schedule = schedule_copy.0.lock().unwrap();
        }
    }

    /// Evaluates all deadlines due at time `ts`
    pub(crate) fn end_offline(&mut self, evaluator: &mut Evaluator, ts: Time) {
        self.end_offline_with_callback(evaluator, ts, |_, _| ());
    }

    /// Evaluates all deadlines due at time `ts` and calls the callback after the evaluation of each deadline
    pub(crate) fn end_offline_with_callback<T>(&mut self, evaluator: &mut Evaluator, ts: Time, mut callback: T)
    where
        T: FnMut(Time, &Evaluator),
    {
        if !self.has_time_driven {
            return;
        }
        let schedule_copy = self.dyn_schedule.clone();
        let mut schedule = schedule_copy.0.lock().unwrap();
        while self.get_next_due_locked(&schedule).is_some() {
            let due = self.get_next_due_locked(&schedule).unwrap();
            if due != ts {
                break;
            }
            let deadline = self.get_next_deadline_locked(ts, &mut schedule);
            drop(schedule);
            self.eval_deadline(evaluator, deadline, due);
            callback(due, evaluator);
            schedule = schedule_copy.0.lock().unwrap();
        }
    }

    /// Evaluates the given deadline
    pub(crate) fn eval_deadline(&mut self, evaluator: &mut Evaluator, deadline: Vec<EvaluationTask>, due: Time) {
        debug_assert!(
            !self.ir.time_driven.is_empty()
                || self.ir.outputs.iter().any(|o| matches!(o.instance_template.spawn.pacing, PacingType::Periodic(_)))
        );

        self.handler.debug(|| format!("Schedule Timed-Event {:?}.", (&deadline, due)));
        self.handler.new_event();
        evaluator.eval_time_driven_tasks(deadline, due);
    }
}
