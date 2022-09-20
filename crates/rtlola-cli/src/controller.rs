use super::event_driven_manager::EventDrivenManager;
use super::time_driven_manager::TimeDrivenManager;
use super::{WorkItem, CAP_WORK_QUEUE};
use crate::config::{Config, ExecutionMode::*};
use crate::configuration::time::{OutputTimeRepresentation, TimeRepresentation};
use crate::coordination::dynamic_schedule::DynamicSchedule;
use crate::coordination::TimeEvaluation;
use crate::evaluator::{Evaluator, EvaluatorData};
use crate::Time;
use crate::Value;
use crossbeam_channel::{bounded, unbounded};
use std::error::Error;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

pub(crate) struct Controller<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> {
    config: Config<InputTime, OutputTime>,

    /// Dynamic schedules handles dynamic deadlines; The condition is notified whenever the schedule changes
    dyn_schedule: Arc<(Mutex<DynamicSchedule>, Condvar)>,
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> Controller<InputTime, OutputTime> {
    pub(crate) fn new(config: Config<InputTime, OutputTime>) -> Self {
        let dyn_schedule = Arc::new((Mutex::new(DynamicSchedule::new()), Condvar::new()));

        Self { config, dyn_schedule }
    }

    pub(crate) fn start(self) -> Result<(), Box<dyn Error>> {
        // TODO: Returning the Arc here makes no sense, fix asap.
        match self.config.mode {
            Offline => self.evaluate_offline(),
            Online => self.evaluate_online(),
        }
    }

    /// Starts the online evaluation process, i.e. periodically computes outputs for time-driven streams
    /// and fetches/expects events from specified input source.
    fn evaluate_online(&self) -> Result<(), Box<dyn Error>> {
        let (work_tx, work_rx) = unbounded();
        let copy_output_handler = self.output_handler.clone();

        if self.config.ir.has_time_driven_features() {
            let work_tx_clone = work_tx.clone();
            let ir_clone = self.config.ir.clone();
            let ds_clone = self.dyn_schedule.clone();
            let _ = thread::Builder::new().name("TimeDrivenManager".into()).spawn(move || {
                let time_manager = TimeDrivenManager::setup(ir_clone, copy_output_handler, ds_clone)
                    .unwrap_or_else(|s| panic!("{}", s));
                time_manager.start_online(work_tx_clone);
            });
        };

        let copy_output_handler = self.output_handler.clone();

        let cfg_clone = self.config.clone();
        // TODO: Wait until all events have been read.
        let _event = thread::Builder::new().name("EventDrivenManager".into()).spawn(move || {
            let event_manager = EventDrivenManager::<InputTime, OutputTime>::setup(cfg_clone, copy_output_handler);
            event_manager.start_online(work_tx);
        });

        let copy_output_handler = self.output_handler.clone();
        let evaluatordata = EvaluatorData::new(self.config.ir.clone(), copy_output_handler, self.dyn_schedule.clone());

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

        // Setup EventDrivenManager
        let output_copy_handler = self.output_handler.clone();
        let cfg_clone = self.config.clone();
        let edm_thread = thread::Builder::new()
            .name("EventDrivenManager".into())
            .spawn(move || {
                let event_manager = EventDrivenManager::<InputTime, OutputTime>::setup(cfg_clone, output_copy_handler);
                event_manager
                    .start_offline(work_tx)
                    .unwrap_or_else(|e| unreachable!("EventDrivenManager failed: {}", e));
            })
            .unwrap_or_else(|e| unreachable!("Failed to start EventDrivenManager thread: {}", e));

        // Setup TimeDrivenManager
        let ir_clone = self.config.ir.clone();
        let output_copy_handler = self.output_handler.clone();
        let mut time_manager = TimeDrivenManager::setup(ir_clone, output_copy_handler, self.dyn_schedule.clone())?;

        // Setup Evaluator
        let output_copy_handler = self.output_handler.clone();
        let evaluatordata = EvaluatorData::new(self.config.ir.clone(), output_copy_handler, self.dyn_schedule.clone());
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
    pub(crate) fn evaluate_timed_item(&self, evaluator: &mut Evaluator<OutputTime>, t: TimeEvaluation, ts: Time) {
        self.output_handler.new_event();
        evaluator.eval_time_driven_tasks(t, ts);
    }

    #[inline]
    pub(crate) fn evaluate_event_item(&self, evaluator: &mut Evaluator<OutputTime>, e: &[Value], ts: Time) {
        self.output_handler.new_event();
        evaluator.eval_event(e, ts)
    }
}
