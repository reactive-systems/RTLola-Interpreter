//! This module exposes functionality to handle the input and output methods of the CLI.
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;

#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin;

#[cfg(feature = "network_plugin")]
pub mod network_plugin;

use std::error::Error;

use rtlola_interpreter::input::InputMap;
use rtlola_interpreter::time::TimeRepresentation;

type EventResult<MappedEvent, Time, Error> = Result<Option<(MappedEvent, Time)>, Error>;

/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    type MappedEvent: InputMap;
    type Error: Error;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(&self) -> Result<<Self::MappedEvent as InputMap>::CreationData, Self::Error>;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> EventResult<Self::MappedEvent, InputTime::InnerTime, Self::Error>;
}
