mod config;
mod csv_input;
mod io_handler;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

pub type Time = Duration;

pub use self::config::{EvalConfig, ExecutionMode, Statistics, TimeFormat, TimeRepresentation, Verbosity};
pub use self::io_handler::OutputChannel;
pub(crate) use self::io_handler::{create_event_source, EventSource, EventSourceConfig, OutputHandler};

pub use self::csv_input::{CSVEventSource, CSVInputSource};

#[cfg(feature = "pcap_interface")]
pub use self::pcap_input::{PCAPEventSource, PCAPInputSource};
use std::time::Duration;
