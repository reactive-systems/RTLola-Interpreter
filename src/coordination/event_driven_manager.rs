use crate::basics::{
    create_event_source, AbsoluteTimeFormat, EvalConfig, EventSource, ExecutionMode, OutputHandler, RawTime,
};
use crate::coordination::{WorkItem, CAP_LOCAL_QUEUE};
use crate::storage::Value;
use crate::{Time, TimeRepresentation};
use crossbeam_channel::Sender;
use rtlola_frontend::mir::RtLolaMir;
use std::error::Error;
use std::ops::AddAssign;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub(crate) type EventEvaluation = Vec<Value>;

/// Represents the current cycle count for event-driven events.
//TODO(marvin): u128? wouldn't u64 suffice?
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
struct EventDrivenCycleCount(u128);

type Edm = EventDrivenManager;

impl From<u128> for EventDrivenCycleCount {
    fn from(i: u128) -> EventDrivenCycleCount {
        EventDrivenCycleCount(i)
    }
}

impl AddAssign<u128> for EventDrivenCycleCount {
    fn add_assign(&mut self, i: u128) {
        *self = EventDrivenCycleCount(self.0 + i)
    }
}

pub(crate) struct EventDrivenManager {
    current_cycle: EventDrivenCycleCount,
    out_handler: Arc<OutputHandler>,
    event_source: Box<dyn EventSource>,
    last_event_time: Duration,
    time_repr: TimeRepresentation,
    start_time: Option<SystemTime>,
}

impl EventDrivenManager {
    /// Creates a new EventDrivenManager managing event-driven output streams.
    pub(crate) fn setup(
        ir: RtLolaMir,
        config: EvalConfig,
        out_handler: Arc<OutputHandler>,
        monitor_start: SystemTime,
    ) -> EventDrivenManager {
        let event_source = match create_event_source(config.source.clone(), &ir) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot create input reader: {}", e);
                std::process::exit(1);
            }
        };
        let time_repr = match config.mode {
            ExecutionMode::Offline(tr) => tr,
            //Exact time format does not matter here
            ExecutionMode::Online => TimeRepresentation::Absolute(AbsoluteTimeFormat::UnixTimeFloat),
        };

        let start_time = match time_repr {
            TimeRepresentation::Relative(_) | TimeRepresentation::Incremental(_) => {
                config.start_time.or(Some(monitor_start))
            }
            // Default time should be the one of first event
            TimeRepresentation::Absolute(_) => None,
        };
        if let Some(start_time) = start_time {
            out_handler.set_start_time(start_time);
        }

        Edm {
            current_cycle: 0.into(),
            out_handler,
            event_source,
            last_event_time: Duration::default(),
            time_repr,
            start_time,
        }
    }

    pub(crate) fn finalize_time(&mut self, time: RawTime) -> Time {
        match self.time_repr {
            TimeRepresentation::Relative(_) => time.relative(),
            TimeRepresentation::Incremental(_) => {
                self.last_event_time += time.relative();
                self.last_event_time
            }
            TimeRepresentation::Absolute(_) => {
                let t = time.absolute();
                if self.start_time.is_none() {
                    self.start_time = Some(t);
                    self.out_handler.set_start_time(t);
                }
                t.duration_since(self.start_time.unwrap()).expect("Time did not behave monotonically!")
            }
        }
    }

    pub(crate) fn start_online(mut self, work_queue: Sender<WorkItem>) -> ! {
        loop {
            if !self.event_source.has_event() {
                let _ = work_queue.send(WorkItem::End); // Whether it fails or not, we really don't care.
                                                        // Sleep until you slowly fade into nothingness...
                loop {
                    std::thread::sleep(std::time::Duration::new(u64::MAX, 0))
                }
            }
            let (event, raw_time) = self.event_source.get_event();
            let time = self.finalize_time(raw_time);
            self.out_handler.new_input(time);
            match work_queue.send(WorkItem::Event(event, time)) {
                Ok(_) => {}
                Err(e) => self.out_handler.runtime_warning(|| format!("Error when sending work item. {}", e)),
            }
            self.current_cycle += 1;
        }
    }

    pub(crate) fn start_offline(mut self, work_queue: Sender<Vec<WorkItem>>) -> Result<(), Box<dyn Error>> {
        loop {
            let mut local_queue = Vec::with_capacity(CAP_LOCAL_QUEUE);
            for _i in 0..local_queue.capacity() {
                if !self.event_source.has_event() {
                    local_queue.push(WorkItem::End);
                    let _ = work_queue.send(local_queue);
                    return Ok(());
                }
                let (event, raw_time) = self.event_source.get_event();
                let time = self.finalize_time(raw_time);

                local_queue.push(WorkItem::Event(event, time));
                self.current_cycle += 1;
            }
            match work_queue.send(local_queue) {
                Ok(_) => {}
                Err(e) => self.out_handler.runtime_warning(|| format!("Error when sending local queue. {}", e)),
            }
        }
    }
}
