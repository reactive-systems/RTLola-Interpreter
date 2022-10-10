//! This module exposes basic functionality that is useful when dealing with the interpreter or the Api.

mod csv_input;
mod output;
#[cfg(feature = "pcap_interface")]
mod pcap_input;

pub(crate) use csv_input::{CsvEventSource, CsvInputSourceKind};
#[cfg(feature = "pcap_interface")]
pub(crate) use pcap_input::{PcapEventSource, PcapInputSource};
use rtlola_interpreter::monitor::Record;
use rtlola_interpreter::time::TimeRepresentation;

pub(crate) use self::output::{EvalTimeTracer, OutputChannel, OutputHandler};

/// A trait that represents the functionality needed for an event source.
pub(crate) trait EventSource<InputTime: TimeRepresentation> {
    type Rec: Record;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(&self) -> <Self::Rec as Record>::CreationData;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> Option<(Self::Rec, InputTime::InnerTime)>;
}
