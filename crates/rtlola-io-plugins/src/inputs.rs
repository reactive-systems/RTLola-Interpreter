#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;

#[cfg(feature = "pcap_plugin")]
pub mod pcap_plugin;

#[cfg(feature = "byte_plugin")]
pub mod byte_plugin;

use std::error::Error;

use rtlola_interpreter::input::{AssociatedEventFactory, EventFactory};
use rtlola_interpreter::time::TimeRepresentation;

type EventResult<MappedEvent, Time, Error> = Result<Option<(MappedEvent, Time)>, Error>;

/// The main trait that has to be implemented by an input plugin
pub trait EventSource<InputTime: TimeRepresentation> {
    /// Type of the Event given to the monitor
    type Record: AssociatedEventFactory;
    /// Error type when buildin the next Event
    type Error: Error;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(
        &self,
    ) -> Result<
        <<Self::Record as AssociatedEventFactory>::Factory as EventFactory>::CreationData,
        Self::Error,
    >;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> EventResult<Self::Record, InputTime::InnerTime, Self::Error>;
}
