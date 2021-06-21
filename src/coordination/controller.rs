use super::event_driven_manager::EventDrivenManager;
use super::time_driven_manager::TimeDrivenManager;
use super::{WorkItem, CAP_WORK_QUEUE};
use crate::basics::{EvalConfig, ExecutionMode::*, OutputHandler, Time};
use crate::coordination::dynamic_schedule::DynamicSchedule;
use crate::coordination::monitor::Monitor;
use crate::coordination::{EventEvaluation, TimeEvaluation};
use crate::evaluator::{Evaluator, EvaluatorData};
use crossbeam_channel::{bounded, unbounded};
use rtlola_frontend::mir::{Deadline, OutputReference, PacingType, RtLolaMir, Task};
use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Instant;

pub(crate) struct Controller {
    ir: RtLolaMir,

    config: EvalConfig,

    /// Handles all kind of output behavior according to config.
    pub(crate) output_handler: Arc<OutputHandler>,

    /// Dynamic schedules handles dynamic deadlines; The condition is notified whenever the schedule changes
    dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
}

impl Controller {
    pub(crate) fn new(ir: RtLolaMir, config: EvalConfig) -> Self {
        let output_handler = Arc::new(OutputHandler::new(&config, ir.triggers.len()));
        let dyn_schedule = Arc::new((Mutex::new(DynamicSchedule::new()), Condvar::new()));
        Self { ir, config, output_handler, dyn_schedule }
    }

    pub(crate) fn start(self) -> Result<Arc<OutputHandler>, Box<dyn Error>> {
        // TODO: Returning the Arc here makes no sense, fix asap.
        match self.config.mode {
            Offline => self.evaluate_offline().map(|_| self.output_handler),
            Online => self.evaluate_online().map(|_| self.output_handler),
            Api => unreachable!(),
        }
    }

    /// Starts the online evaluation process, i.e. periodically computes outputs for time-driven streams
    /// and fetches/expects events from specified input source.
    fn evaluate_online(&self) -> Result<(), Box<dyn Error>> {
        let (work_tx, work_rx) = unbounded();
        let now = Instant::now();
        let copy_output_handler = self.output_handler.clone();

        if self.ir.has_time_driven_features() {
            let work_tx_clone = work_tx.clone();
            let ir_clone = self.ir.clone();
            let ds_clone = self.dyn_schedule.clone();
            let _ = thread::Builder::new().name("TimeDrivenManager".into()).spawn(move || {
                let time_manager = TimeDrivenManager::setup(ir_clone, copy_output_handler, ds_clone)
                    .unwrap_or_else(|s| panic!("{}", s));
                time_manager.start_online(now, work_tx_clone);
            });
        };

        let copy_output_handler = self.output_handler.clone();

        let ir_clone = self.ir.clone();
        let cfg_clone = self.config.clone();
        // TODO: Wait until all events have been read.
        let _event = thread::Builder::new().name("EventDrivenManager".into()).spawn(move || {
            let event_manager = EventDrivenManager::setup(ir_clone, cfg_clone, copy_output_handler, now);
            event_manager.start_online(work_tx);
        });

        let copy_output_handler = self.output_handler.clone();
        let evaluatordata = EvaluatorData::new(
            self.ir.clone(),
            self.config.clone(),
            copy_output_handler,
            Some(now),
            self.dyn_schedule.clone(),
        );

        let mut evaluator = evaluatordata.into_evaluator();

        loop {
            let item = match work_rx.recv() {
                Ok(item) => item,
                Err(e) => panic!("Both producers hung up! {}", e),
            };
            self.output_handler.debug(|| format!("Received {:?}.", item));
            match item {
                WorkItem::Event(e, ts) => self.evaluate_event_item(&mut evaluator, &e, ts),
                WorkItem::Time(t, ts) => self.evaluate_timed_item(&mut evaluator, t, ts),
                WorkItem::End => {
                    self.output_handler.output(|| "Finished entire input. Terminating.");
                    std::process::exit(0);
                }
            }
        }
    }

    /// Starts the offline evaluation process, i.e. periodically computes outputs for time-driven streams
    /// and fetches/expects events from specified input source.
    fn evaluate_offline(&self) -> Result<(), Box<dyn Error>> {
        // Use a bounded channel for offline mode, as we "control" time.
        let (work_tx, work_rx) = bounded(CAP_WORK_QUEUE);
        let (time_tx, time_rx) = bounded(1);

        // Setup EventDrivenManager
        let output_copy_handler = self.output_handler.clone();
        let ir_clone = self.ir.clone();
        let cfg_clone = self.config.clone();
        let edm_thread = thread::Builder::new()
            .name("EventDrivenManager".into())
            .spawn(move || {
                let event_manager = EventDrivenManager::setup(ir_clone, cfg_clone, output_copy_handler, Instant::now());
                event_manager
                    .start_offline(work_tx, time_tx)
                    .unwrap_or_else(|e| unreachable!("EventDrivenManager failed: {}", e));
            })
            .unwrap_or_else(|e| unreachable!("Failed to start EventDrivenManager thread: {}", e));

        // Get start time
        let start_time = match time_rx.recv() {
            Err(e) => unreachable!("Did not receive a start event in offline mode! {}", e),
            Ok(ts) => ts,
        };
        let mut start_time_ref = self.output_handler.start_time.lock().unwrap();
        *start_time_ref = start_time;
        drop(start_time_ref);

        // Setup TimeDrivenManager
        let ir_clone = self.ir.clone();
        let output_copy_handler = self.output_handler.clone();
        let mut time_manager = TimeDrivenManager::setup(ir_clone, output_copy_handler, self.dyn_schedule.clone())?;

        // Setup Evaluator
        let output_copy_handler = self.output_handler.clone();
        let evaluatordata = EvaluatorData::new(
            self.ir.clone(),
            self.config.clone(),
            output_copy_handler,
            Some(Instant::now()),
            self.dyn_schedule.clone(),
        );
        let mut evaluator = evaluatordata.into_evaluator();

        let mut current_time = Time::default();
        'outer: loop {
            let local_queue = work_rx.recv().unwrap_or_else(|e| panic!("EventDrivenManager hung up! {}", e));
            for item in local_queue {
                self.output_handler.debug(|| format!("Received {:?}.", item));
                match item {
                    WorkItem::Event(e, ts) => {
                        time_manager.accept_time_offline(&mut evaluator, ts);
                        self.output_handler.debug(|| format!("Schedule Event {:?}.", (&e, ts)));
                        self.evaluate_event_item(&mut evaluator, &e, ts);
                        current_time = ts;
                    }
                    WorkItem::Time(_, _) => panic!("Received time command in offline mode."),
                    WorkItem::End => {
                        time_manager.end_offline(&mut evaluator, current_time);
                        self.output_handler.output(|| "Finished entire input. Terminating.");
                        self.output_handler.terminate();
                        break 'outer;
                    }
                }
            }
        }

        edm_thread.join().expect("Could not join on EventDrivenManger thread");
        Ok(())
    }

    #[inline]
    pub(crate) fn evaluate_timed_item(&self, evaluator: &mut Evaluator, t: TimeEvaluation, ts: Time) {
        self.output_handler.new_event();
        evaluator.eval_time_driven_tasks(t, ts);
    }

    #[inline]
    pub(crate) fn evaluate_event_item(&self, evaluator: &mut Evaluator, e: &EventEvaluation, ts: Time) {
        self.output_handler.new_event();
        evaluator.eval_event(e, ts)
    }
}
