mod config;
mod csv_input;
mod io_handler;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

pub type Time = Duration;

pub use self::config::{
    AbsoluteTimeFormat, EvalConfig, ExecutionMode, RelativeTimeFormat, Statistics, TimeRepresentation, Verbosity,
};
pub(crate) use self::io_handler::{create_event_source, EventSource, OutputHandler, RawTime};
pub use self::io_handler::{EventSourceConfig, OutputChannel};

pub use self::csv_input::{CsvEventSource, CsvInputSource};

#[cfg(feature = "pcap_interface")]
pub use self::pcap_input::{PCAPEventSource, PCAPInputSource};
use std::time::Duration;
