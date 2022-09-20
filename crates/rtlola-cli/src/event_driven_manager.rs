use crate::config::Config;
use crate::configuration::time::{OutputTimeRepresentation, TimeRepresentation};
use crate::coordination::{WorkItem, CAP_LOCAL_QUEUE};
use crate::Value;
use crossbeam_channel::Sender;
use std::error::Error;
use std::sync::Arc;

pub(crate) type EventEvaluation = Vec<Value>;

pub(crate) struct EventDrivenManager<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> {
    output_handler: Arc<OutputHandler<OutputTime>>,
    event_source: Box<dyn EventSource<InputTime>>,
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> EventDrivenManager<InputTime, OutputTime> {
    /// Creates a new EventDrivenManager managing event-driven output streams.
    pub(crate) fn setup(
        config: Config<InputTime, OutputTime>,
        output_handler: Arc<OutputHandler<OutputTime>>,
    ) -> EventDrivenManager<InputTime, OutputTime> {
        let Config { ir, source, start_time, input_time_representation, .. } = config;
        let event_source = match create_event_source::<InputTime>(source, &ir, start_time, input_time_representation) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot create input reader: {}", e);
                std::process::exit(1);
            }
        };

        EventDrivenManager { output_handler, event_source }
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
            let (event, time) = self.event_source.get_event();
            match work_queue.send(WorkItem::Event(event, time)) {
                Ok(_) => {}
                Err(e) => self.output_handler.runtime_warning(|| format!("Error when sending work item. {}", e)),
            }
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
                let (event, time) = self.event_source.get_event();

                local_queue.push(WorkItem::Event(event, time));
            }
            match work_queue.send(local_queue) {
                Ok(_) => {}
                Err(e) => self.output_handler.runtime_warning(|| format!("Error when sending local queue. {}", e)),
            }
        }
    }
}
