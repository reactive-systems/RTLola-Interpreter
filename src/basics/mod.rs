pub mod config;
mod csv_input;
mod io_handler;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

use std::time::Duration;
pub type Time = Duration;

pub use self::io_handler::OutputChannel;
pub(crate) use self::io_handler::{create_event_source, EventSource, OutputHandler, RawTime};
pub use csv_input::{CsvEventSource, CsvInputSource, CsvInputSourceKind};
#[cfg(feature = "pcap_interface")]
pub use pcap_input::{PCAPEventSource, PCAPInputSource};
