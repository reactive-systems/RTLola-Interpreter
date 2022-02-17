use crate::basics::{create_event_source, EventSource, OutputHandler};
use crate::config::Config;
use crate::configuration::time::TimeRepresentation;
use crate::coordination::{WorkItem, CAP_LOCAL_QUEUE};
use crate::Value;
use crossbeam_channel::Sender;
use std::error::Error;
use std::sync::Arc;

pub(crate) type EventEvaluation = Vec<Value>;

pub(crate) struct EventDrivenManager<IT: TimeRepresentation, OT: TimeRepresentation> {
    output_handler: Arc<OutputHandler<OT>>,
    event_source: Box<dyn EventSource<IT>>,
}

impl<RT: TimeRepresentation, OT: TimeRepresentation> EventDrivenManager<RT, OT> {
    /// Creates a new EventDrivenManager managing event-driven output streams.
    pub(crate) fn setup<IT: TimeRepresentation>(
        config: Config<IT, OT>,
        output_handler: Arc<OutputHandler<OT>>,
    ) -> Self {
        let Config { ir, source, start_time, mode, .. } = config;
        let event_source = match create_event_source::<IT>(source, &ir) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot create input reader: {}", e);
                std::process::exit(1);
            }
        };

        Self { output_handler, event_source }
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
            self.output_handler.new_input(time);
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
                let (event, raw_time) = self.event_source.get_event();
                let time = self.finalize_time(raw_time);

                local_queue.push(WorkItem::Event(event, time));
            }
            match work_queue.send(local_queue) {
                Ok(_) => {}
                Err(e) => self.output_handler.runtime_warning(|| format!("Error when sending local queue. {}", e)),
            }
        }
    }
}
