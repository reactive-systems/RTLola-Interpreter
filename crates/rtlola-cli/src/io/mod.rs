//! This module exposes basic functionality that is useful when dealing with the interpreter or the Api.

mod csv_input;
mod io_handler;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

pub use csv_input::{CsvEventSource, CsvInputSource, CsvInputSourceKind};
#[cfg(feature = "pcap_interface")]
pub use pcap_input::{PCAPEventSource, PCAPInputSource};

pub use self::io_handler::OutputChannel;
pub(crate) use self::io_handler::{create_event_source, EventSource, OutputHandler};
