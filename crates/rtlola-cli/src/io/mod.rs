//! This module exposes basic functionality that is useful when dealing with the interpreter or the Api.

mod csv_input;
mod output;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

pub(crate) use csv_input::{CsvEventSource, CsvInputSourceKind};
#[cfg(feature = "pcap_interface")]
pub use pcap_input::{PCAPEventSource, PCAPInputSource};
use rtlola_interpreter::monitor::Record;
use rtlola_interpreter::time::TimeRepresentation;

pub use self::output::OutputChannel;
pub(crate) use self::output::OutputHandler;

/// A trait that represents the functionality needed for an event source.

pub(crate) trait EventSource<Rec: Record, InputTime: TimeRepresentation> {
    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(&self) -> Rec::CreationData;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> Option<(Rec, InputTime::InnerTime)>;
}
