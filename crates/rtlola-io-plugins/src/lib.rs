//! This module exposes functionality to handle the input and output methods of the CLI.
#![forbid(unused_must_use)] // disallow discarding errors
#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;

#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin;

#[cfg(feature = "network_plugin")]
pub mod network_plugin;

use std::error::Error;

use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::time::TimeRepresentation;

type EventResult<MappedEvent, Time, Error> = Result<Option<(MappedEvent, Time)>, Error>;

/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    /// Type of the Event given to the monitor
    type Factory: AssociatedFactory;
    /// Error type when buildin the next Event
    type Error: Error;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(
        &self,
    ) -> Result<<<Self::Factory as AssociatedFactory>::Factory as EventFactory>::CreationData, Self::Error>;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> EventResult<Self::Factory, InputTime::InnerTime, Self::Error>;
}
