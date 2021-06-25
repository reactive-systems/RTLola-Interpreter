use super::WorkItem;
use crate::basics::{OutputHandler, Time};

use crate::coordination::dynamic_schedule::DynamicSchedule;
use crate::evaluator::Evaluator;
use crate::Value;
use crossbeam_channel::Sender;
use rtlola_frontend::mir::{Deadline, OutputReference, PacingType, RtLolaMir, Stream, Task};
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EvaluationTask {
    /// Evaluate a specific instance of the output stream, or all instances if the slice is empty
    Evaluate(OutputReference, Vec<Value>),
    /// Spawn a new instance of the output stream,
    Spawn(OutputReference),
    /// Evaluate the close condition for this specific instance, or all instances if the slice is empty.
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

pub(crate) type TimeEvaluation = Vec<EvaluationTask>;

pub(crate) struct TimeDrivenManager {
    ir: RtLolaMir,
    has_time_drive: bool,
    deadlines: Vec<Deadline>,
    dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
    handler: Arc<OutputHandler>,
    // The following fields are only used for offline evaluation
    cur_static_deadline_idx: usize,
    next_static_deadline: Duration,
    static_due_streams: Vec<Task>,
}

impl TimeDrivenManager {
    /// Creates a new TimeDrivenManager managing time-driven output streams.
    pub(crate) fn setup(
        ir: RtLolaMir,
        handler: Arc<OutputHandler>,
        dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
    ) -> Result<TimeDrivenManager, String> {
        let contains_time_driven = !ir.time_driven.is_empty()
            || ir.outputs.iter().any(|o| matches!(o.instance_template.spawn.pacing, PacingType::Periodic(_)));
        if !contains_time_driven {
            // return dummy
            return Ok(TimeDrivenManager {
                ir,
                has_time_drive: false,
                deadlines: vec![],
                dyn_schedule,
                handler,
                cur_static_deadline_idx: 0,
                next_static_deadline: Duration::default(),
                static_due_streams: vec![],
            });
        }

        let schedule = ir.compute_schedule()?;
        let first_deadline = schedule.deadlines.first().expect("Schedule should not be empty");
        let due_streams = first_deadline.due.clone();
        let cur_deadline_idx = 0;
        let next_static_deadline = first_deadline.pause;

        Ok(TimeDrivenManager {
            ir,
            has_time_drive: true,
            deadlines: schedule.deadlines,
            dyn_schedule,
            handler,
            cur_static_deadline_idx: cur_deadline_idx,
            next_static_deadline,
            static_due_streams: due_streams,
        })
    }

    pub(crate) fn get_next_due(&self) -> Time {
        self.get_next_due_locked(&self.dyn_schedule.0.lock().unwrap())
    }

    fn get_next_due_locked(&self, dyn_schedule: &MutexGuard<DynamicSchedule>) -> Time {
        self.next_static_deadline.min(dyn_schedule.get_next_deadline_due().unwrap_or(self.next_static_deadline))
    }

    pub(crate) fn get_next_deadline(&mut self, now: Time) -> Vec<EvaluationTask> {
        let schedule_copy = self.dyn_schedule.clone();
        let lock = schedule_copy.0.lock();
        let res = self.get_next_deadline_locked(now, &mut lock.unwrap());
        res
    }

    fn get_next_deadline_locked(
        &mut self,
        now: Time,
        dyn_schedule: &mut MutexGuard<DynamicSchedule>,
    ) -> Vec<EvaluationTask> {
        debug_assert!(self.has_time_drive);

        let static_due = self.next_static_deadline;
        let dyn_due = dyn_schedule.get_next_deadline_due();

        match (static_due, dyn_due) {
            (sd, Some(dd)) if dd < sd => {
                let mut dyn_deadline = dyn_schedule.get_next_deadline(now).unwrap();
                dyn_deadline.sort(&self.ir);
                dyn_deadline.tasks
            }
            (sd, Some(dd)) if dd == sd => {
                let static_deadline: Vec<EvaluationTask> =
                    self.static_due_streams.iter().map(|t| (*t).into()).collect();
                self.cur_static_deadline_idx = (self.cur_static_deadline_idx + 1) % self.deadlines.len();
                let deadline = &self.deadlines[self.cur_static_deadline_idx];
                assert!(deadline.pause > Duration::from_secs(0));
                self.next_static_deadline += deadline.pause;
                self.static_due_streams = deadline.due.clone();

                let dyn_deadline = dyn_schedule.get_next_deadline(now).unwrap().tasks;

                let mut res =
                    static_deadline.into_iter().chain(dyn_deadline.into_iter()).collect::<Vec<EvaluationTask>>();
                res.sort_by_key(|t| match t {
                    EvaluationTask::Evaluate(idx, _) => self.ir.outputs[*idx].eval_layer().inner(),
                    EvaluationTask::Spawn(idx) => self.ir.outputs[*idx].spawn_layer().inner(),
                    EvaluationTask::Close(_, _) => usize::MAX,
                });
                res
            }
            _ => {
                // There is no dynamic deadline or it is after a static one
                let res = self.static_due_streams.iter().map(|t| (*t).into()).collect();
                self.cur_static_deadline_idx = (self.cur_static_deadline_idx + 1) % self.deadlines.len();
                let deadline = &self.deadlines[self.cur_static_deadline_idx];
                assert!(deadline.pause > Duration::from_secs(0));
                self.next_static_deadline += deadline.pause;
                self.static_due_streams = deadline.due.clone();
                res
            }
        }
    }

    pub(crate) fn start_online(mut self, start_time: Instant, work_chan: Sender<WorkItem>) -> ! {
        debug_assert!(self.has_time_drive);
        let now = Instant::now();
        assert!(now >= start_time, "Time does not behave monotonically!");
        let time = now - start_time;
        self.next_static_deadline += time;

        let schedule_copy = self.dyn_schedule.clone();

        loop {
            let mut schedule = schedule_copy.0.lock().unwrap();
            let mut due_time = self.get_next_due_locked(&schedule);

            let now = Instant::now();
            assert!(now >= start_time, "Time does not behave monotonically!");
            let mut time = now - start_time;

            if time < due_time {
                // Sleep until deadline or until schedule changes
                while time < due_time {
                    let wait_time = due_time - time;

                    //Todo: This wait is inaccurate.
                    let (new_schedule, wait_res) = self.dyn_schedule.1.wait_timeout(schedule, wait_time).unwrap();
                    schedule = new_schedule;
                    //Note: spurious wake-ups should not be a problem

                    let now = Instant::now();
                    assert!(now >= start_time, "Time does not behave monotonically!");
                    time = now - start_time;
                    due_time = self.get_next_due_locked(&schedule);
                }
            }
            let mut eval_tasks: Vec<EvaluationTask> = self.get_next_deadline_locked(due_time, &mut schedule);

            let item = WorkItem::Time(eval_tasks, due_time);
            if work_chan.send(item).is_err() {
                self.handler.runtime_warning(|| "TDM: Sending failed; evaluation cycle lost.");
            }
        }
    }

    /// Evaluates all deadlines due before time `ts`
    pub(crate) fn accept_time_offline(&mut self, evaluator: &mut Evaluator, ts: Time) {
        if !self.has_time_drive {
            return;
        }
        while ts > self.get_next_due() {
            self.eval_next_deadline(evaluator, ts);
        }
    }

    /// Evaluates all deadlines due at time `ts`
    pub(crate) fn end_offline(&mut self, evaluator: &mut Evaluator, ts: Time) {
        if !self.has_time_drive {
            return;
        }

        // schedule last timed event before terminating
        while ts == self.get_next_due() {
            self.eval_next_deadline(evaluator, ts);
        }
    }

    /// Evaluates the next deadline that is due
    pub(crate) fn eval_next_deadline(&mut self, evaluator: &mut Evaluator, ts: Time) {
        debug_assert!(
            !self.ir.time_driven.is_empty()
                || self.ir.outputs.iter().any(|o| matches!(o.instance_template.spawn.pacing, PacingType::Periodic(_)))
        );

        let schedule_copy = self.dyn_schedule.clone();
        let mut schedule = schedule_copy.0.lock().unwrap();
        let next_due = self.get_next_due_locked(&schedule);
        if ts >= next_due {
            let due_streams = self.get_next_deadline_locked(ts, &mut schedule);
            drop(schedule);
            self.handler.debug(|| format!("Schedule Timed-Event {:?}.", (&due_streams, next_due)));
            self.handler.new_event();
            evaluator.eval_time_driven_tasks(due_streams, next_due);
        }
    }

    //The following code is useful and could partly be used again for robustness.

    /*

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TDMState {
        cycle: TimeDrivenCycleCount,
        deadline: usize,
        time: Time,
        // Debug/statistics information.
    }

    #[allow(dead_code)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum StateCompare {
        Equal,
        TimeTravel,
        Next,
        Skipped { cycles: u64, deadlines: u64 },
    }

    /// Represents the current cycle count for time-driven events. `u128` is sufficient to represent
    /// 10^22 years of runtime for evaluation cycles that are 1ns apart.
    #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
    pub(crate) struct TimeDrivenCycleCount(u128);

    impl From<u128> for TimeDrivenCycleCount {
        fn from(i: u128) -> TimeDrivenCycleCount {
            TimeDrivenCycleCount(i)
        }
    }


        pub(crate) fn get_current_deadline(&self, time: Time) -> (Duration, Vec<OutputReference>) {
            let earliest_deadline_state = self.earliest_deadline_state(time);
            let current_deadline = &self.deadlines[earliest_deadline_state.deadline];
            let time_of_deadline = earliest_deadline_state.time + current_deadline.pause;
            assert!(time_of_deadline >= time, "Time does not behave monotonically!");
            let pause = time_of_deadline - time;
            (pause, current_deadline.due.clone())
        }

        /// Compute the state for the deadline earliest deadline that is not missed yet.
        /// Example: If this function is called immediately after setup, it returns
        /// a state containing the start time and deadline 0.
        fn earliest_deadline_state(&self, time: Time) -> TDMState {
            let start_time = self.start_time;
            let hyper_nanos = dur_as_nanos(self.hyper_period);
            assert!(time >= start_time, "Time does not behave monotonically!");
            let time_since_start = (time - start_time).as_nanos();

            let hyper = time_since_start / hyper_nanos;
            let time_within_hyper = time_since_start % hyper_nanos;

            // Determine the index of the current deadline.
            let mut sum = 0u128;
            for (ix, dl) in self.deadlines.iter().enumerate() {
                let pause = dur_as_nanos(dl.pause);
                if sum + pause < time_within_hyper {
                    sum += pause
                } else {
                    let offset_from_start = dur_from_nanos(hyper_nanos * hyper + sum);
                    let dl_time = start_time + offset_from_start;
                    return TDMState { cycle: hyper.into(), deadline: ix, time: dl_time };
                }
            }
            unreachable!()
        }


        /// Determines how long the current thread can wait until the next time-based evaluation cycle
        /// needs to be started. Calls are time-sensitive, i.e. successive calls do not necessarily
        /// yield identical results.
        ///
        /// *Returns:* `WaitingTime` _t_ and `TimeDrivenCycleCount` _i_ where _t_ nanoseconds can pass
        /// until the _i_th time-driven evaluation cycle needs to be started.
        fn wait_for(&self, time: Option<SystemTime>) -> (Duration, TimeDrivenCycleCount) {
            let time = time.unwrap_or_else(SystemTime::now);
            let current_state = self.current_state(time);
            if let Some(last_state) = self.last_state {
                match self.compare_states(last_state, current_state) {
                    StateCompare::TimeTravel => panic!("Bug: Traveled back in time!"),
                    StateCompare::Skipped { cycles, deadlines } => self.skipped_deadline(cycles, deadlines), // Carry on.
                    StateCompare::Next => self.skipped_deadline(0, 1), // Carry one.
                    StateCompare::Equal => {
                        // Nice, we did not miss a deadline!
                    }
                }
            }
            let deadline = &self.deadlines[current_state.deadline];
            let offset = self.time_since_last_deadline(time);
            assert!(offset < deadline.pause);
            (deadline.pause - offset, current_state.cycle)
        }

        /// Returns all time-driven streams that are due to be extended in time-driven evaluation
        /// cycle `c`. The returned collection is ordered according to the evaluation order.
        fn due_streams(&mut self, time: Option<SystemTime>) -> Option<&Vec<StreamReference>> {
            let time = time.unwrap_or_else(SystemTime::now);
            let state = self.current_state(time);
            if let Some(old_state) = self.last_state {
                match self.compare_states(old_state, state) {
                    StateCompare::Next => {} // Perfect, skip!
                    StateCompare::TimeTravel => panic!("Bug: Traveled back in time!"),
                    StateCompare::Equal => {
                        self.query_too_soon();
                        return None;
                    }
                    StateCompare::Skipped { cycles, deadlines } => self.skipped_deadline(cycles, deadlines),
                }
            }
            self.last_state = Some(state);
            Some(&self.deadlines[state.deadline].due)
        }

        fn skipped_deadline(&self, cycles: u64, deadlines: u64) {
            if cfg!(debug_assertion) {
                // Only panic in non-release config.
                panic!("Missed {} cycles and {} deadlines.", cycles, deadlines);
            } else {
                // Otherwise, inform the output handler and carry on.
                self.handler.runtime_warning(|| {
                    format!("Warning: Pressure exceeds capacity! missed {} cycles and {} deadlines", cycles, deadlines)
                })
            }
        }

        fn query_too_soon(&self) {
            if cfg!(debug_assertion) {
                // Only panic in non-release config.
                panic!("Called `TimeDrivenManager::wait_for` too early; no deadline has passed.");
            } else {
                // Otherwise, inform the output handler and carry on.
                self.handler
                    .debug(|| String::from("Warning: Called `TimeDrivenManager::wait_for` twice for the same deadline."))
            }
        }

        fn time_since_last_deadline(&self, time: SystemTime) -> Duration {
            let last_deadline = self.last_deadline_state(time);
            time.duration_since(last_deadline.time)
        }

        /// Compares two given states in terms of their temporal relation.
        /// Detects equality, successivity, contradiction, i.e. the `new` state is older than the `old`
        /// one, and the amount of missed deadlines in-between `old` and `new`.
        fn compare_states(&self, old: TDMState, new: TDMState) -> StateCompare {
            let c1 = old.cycle.0;
            let c2 = new.cycle.0;
            let d1 = old.deadline;
            let d2 = new.deadline;
            match c1.cmp(&c2) {
                Ordering::Greater => StateCompare::TimeTravel,
                Ordering::Equal => {
                    match d1.cmp(&d2) {
                        Ordering::Greater => StateCompare::TimeTravel,
                        Ordering::Equal => StateCompare::Equal,
                        Ordering::Less => {
                            let diff = (d2 - d1) as u64; // Safe: d2 must be greater than d1.
                            if diff == 1 {
                                StateCompare::Next
                            } else {
                                // diff >= 2
                                StateCompare::Skipped { cycles: 0, deadlines: diff - 1 }
                            }
                        }
                    }
                }
                Ordering::Less => {
                    let diff = (c2 - c1) as u64; // Safe: c2 must to be greater than c1.
                    if diff == 1 && d1 == self.deadlines.len() - 1 && d2 == 0 {
                        StateCompare::Next
                    } else {
                        let d_diff = (d2 as i128) - (d1 as i128); // Widen to assure safe arithmetic.
                        if d_diff > 0 {
                            StateCompare::Skipped { cycles: diff, deadlines: (d_diff - 1) as u64 }
                        } else if d_diff == 0 {
                            // d_diff <= 0
                            StateCompare::Skipped { cycles: diff - 1, deadlines: self.deadlines.len() as u64 }
                        } else {
                            // d_dif < 0
                            let abs = -d_diff as u64;
                            let actual_d_diff = self.deadlines.len() as u64 - abs;
                            StateCompare::Skipped { cycles: diff - 1, deadlines: actual_d_diff - 1 }
                        }
                    }
                }
            }
        }

        /// Computes the last deadline that should have been evaluated according to the given `time`.
        /// The state's time is the earliest point in time where this state could have been the current
        /// one.
        /// Requires start_time to be non-none.
        fn last_deadline_state(&self, time: SystemTime) -> TDMState {
            let start_time = self.start_time.unwrap();
            assert!(start_time < time);
            let hyper_nanos = Self::dur_as_nanos(self.hyper_period);
            let running = Self::dur_as_nanos(time.duration_since(start_time));
            let cycle = running / hyper_nanos;
            let in_cycle = running % hyper_nanos;
            let mut sum = 0u128;
            for (ix, dl) in self.deadlines.iter().enumerate() {
                let pause = Self::dur_as_nanos(dl.pause);
                if sum + pause < in_cycle {
                    sum += pause
                } else {
                    let offset_duration = Self::dur_from_nanos(hyper_nanos * cycle + sum);
                    let dl_time = start_time + offset_duration;
                    return TDMState { cycle: cycle.into(), deadline: ix, time: dl_time };
                }
            }
            unreachable!()
        }

        /// Computes the TDMState in which we should be right now given the supplied `time`.
        fn current_state(&self, time: SystemTime) -> TDMState {
            let state = self.last_deadline_state(time);
            TDMState { time, ..state }
        }

        */
}
