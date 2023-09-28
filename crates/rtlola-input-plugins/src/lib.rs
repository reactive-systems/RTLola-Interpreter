//! This module exposes functionality to handle the input and output methods of the CLI.
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;

#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin;

use std::error::Error;

use rtlola_interpreter::input::InputMap;
use rtlola_interpreter::time::TimeRepresentation;

type EventResult<Rec, Time, Error> = Result<Option<(Rec, Time)>, Error>;

/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    type Rec: InputMap;
    type Error: Error;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(&self) -> Result<<Self::Rec as InputMap>::CreationData, Self::Error>;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> EventResult<Self::Rec, InputTime::InnerTime, Self::Error>;
}
