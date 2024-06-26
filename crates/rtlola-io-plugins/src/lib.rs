//! This module exposes functionality to handle the input and output methods of the CLI.
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
    type MappedEvent: AssociatedFactory;
    type Error: Error;

    /// Return the data needed by the monitor to initialize the input source.
    fn init_data(
        &self,
    ) -> Result<<<Self::MappedEvent as AssociatedFactory>::Factory as EventFactory>::CreationData, Self::Error>;

    /// Queries the event source for a new Record(Event) in a blocking fashion.
    /// If there are no more records, None is returned.
    fn next_event(&mut self) -> EventResult<Self::MappedEvent, InputTime::InnerTime, Self::Error>;
}

// use std::time::Duration;

// use byteorder::ByteOrder;
// use rtlola_interpreter_macros::{CompositFactory, ValueFactory};

// #[derive(Debug, Clone, CompositFactory)]
// pub(crate) struct TestInputWithMacros {
//     header: Header,
//     d: Message,
// }

// #[derive(Debug, Clone, ValueFactory)]
// pub(crate) struct Header {
//     timestamp: f64,
//     a: f64,
// }

// impl TestInputWithMacros {
//     pub(crate) fn new(header: Header, d: Message) -> Self {
//         Self { header, d }
//     }
// }

// #[derive(Debug, Clone, CompositFactory)]
// pub(crate) enum Message {
//     M0(Message0),
//     M1(Message1),
// }

// #[derive(Debug, Clone, ValueFactory)]
// pub(crate) struct Message0 {
//     b: f64,
// }

// #[derive(Debug, Clone, ValueFactory)]
// pub(crate) struct Message1 {
//     c: u64,
// }
