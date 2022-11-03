//! This module exposes functionality to handle the input and output methods of the CLI.
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;

#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin;

use rtlola_interpreter::monitor::Record;
use rtlola_interpreter::time::TimeRepresentation;

/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    type Rec: Record;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(&self) -> <Self::Rec as Record>::CreationData;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> Option<(Self::Rec, InputTime::InnerTime)>;
}
