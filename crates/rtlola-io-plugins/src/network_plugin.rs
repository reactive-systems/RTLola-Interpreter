//! Module that contains the implementation of the [EventSource] for Networks represented by the [NetworkEventSource] struct
use std::error::Error;
use std::fmt::Debug;
use std::marker::PhantomData;

use byteorder::ByteOrder;
use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::time::TimeRepresentation;
use time_converter::TimeConverter;

use crate::EventSource;

pub mod time_converter;
pub mod upd;

/// Parses events from a [network connection](NetworkSource) and a [parser](ByteFactory).
#[derive(Debug)]
pub struct NetworkEventSource<
    Source: NetworkSource,
    Order: ByteOrder,
    Factory: ByteFactory<Order>,
    InputTime: TimeRepresentation,
    const BUFFERSIZE: usize,
> where
    <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    /// The factory to parse the monitor input given as bytearray
    factory: Factory,
    /// The connection that is used to receive data
    source: Source,
    /// The buffer that is used to store incoming data
    buffer: Vec<u8>,
    /// PhantomData that is used to propagate types
    timer: PhantomData<(InputTime, Order)>,
}

impl<
        Source: NetworkSource,
        Order: ByteOrder,
        Factory: ByteFactory<Order>,
        InputTime: TimeRepresentation,
        const BUFFERSIZE: usize,
    > NetworkEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
where
    <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [NetworkEventSource] out of a (conncetion)[NetworkSource] and a (parser)[ByteFactory].
    pub fn new(source: Source, factory: Factory) -> Self {
        Self {
            factory,
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

/// Enum to collect the errors with for a [NetworkEventSource].
#[derive(Debug)]
pub enum NetworkEventSourceError<Factory: Error + Debug, Source: Error + Debug, InputMap: Error + Debug> {
    /// Error while receiving a bytestream.
    Source(Source),
    /// Error while parsing a bytestream.
    EventFactory(Factory),
    /// Error while generating the time out of the bytestream
    ByteFactory(InputMap),
}

impl<EventFactory: Error + Debug, Source: Error + Debug, ByteFactory: Error + Debug> std::fmt::Display
    for NetworkEventSourceError<EventFactory, Source, ByteFactory>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkEventSourceError::Source(e) => write!(f, "{e}"),
            NetworkEventSourceError::EventFactory(e) => write!(f, "{e}"),
            NetworkEventSourceError::ByteFactory(e) => write!(f, "{e}"),
        }
    }
}

impl<EventFactory: Error + Debug + 'static, Source: Error + Debug + 'static, ByteFactory: Error + Debug + 'static> Error
    for NetworkEventSourceError<EventFactory, Source, ByteFactory>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            NetworkEventSourceError::Source(e) => Some(e),
            NetworkEventSourceError::EventFactory(e) => Some(e),
            NetworkEventSourceError::ByteFactory(e) => Some(e),
        }
    }
}

impl<
        InputTime: TimeRepresentation,
        Factory: ByteFactory<Order>,
        Source: NetworkSource + Debug,
        Order: ByteOrder,
        const BUFFERSIZE: usize,
    > EventSource<InputTime> for NetworkEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
where
    <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error: Error,
    Factory::Factory: TimeConverter<InputTime>,
{
    type Error = NetworkEventSourceError<
        Factory::Error,
        Source::Error,
        <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error,
    >;
    type Factory = Factory::Factory;

    fn init_data(
        &self,
    ) -> Result<<<Self::Factory as AssociatedFactory>::Factory as EventFactory>::CreationData, Self::Error> {
        todo!()
    }

    fn next_event(
        &mut self,
    ) -> crate::EventResult<Self::Factory, <InputTime as TimeRepresentation>::InnerTime, Self::Error> {
        loop {
            let event = self.factory.from_bytes(&self.buffer).map(|(event, package_size)| {
                let ts = <<Factory as ByteFactory<Order>>::Factory as TimeConverter<InputTime>>::convert_time(&event)
                    .map_err(NetworkEventSourceError::ByteFactory)?;
                let slice = self.buffer.drain(0..package_size);
                debug_assert_eq!(slice.len(), package_size);
                Ok((event, ts))
            });
            match event {
                Ok(res) => break Ok(Some(res?)),
                Err(ByteParsingError::Incomplete) => {
                    let mut temp_buffer = [0_u8; BUFFERSIZE];
                    let package_size = self
                        .source
                        .read(&mut temp_buffer)
                        .map_err(NetworkEventSourceError::Source)?;
                    match package_size {
                        None | Some(0) => break Ok(None),
                        Some(package_size) => self.buffer.extend_from_slice(&temp_buffer[0..package_size]),
                    }
                },
                Err(ByteParsingError::Inner(e)) => break Err(NetworkEventSourceError::EventFactory(e)),
            }
        }
    }
}

/// Trait to collect all connections for the [NetworkEventSource]
pub trait NetworkSource {
    /// Error when receiving the bytestream
    type Error: Error + 'static;
    /// Function to receive the bytestream and returns the number of parsed bytes
    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;
}

impl<T: std::io::Read> NetworkSource for T {
    type Error = std::io::Error;

    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        match std::io::Read::read(self, buffer) {
            Ok(size) => Ok(Some(size)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// This trait defines a factory to parse a bytestream to an event given to the monitor.
/// It contains one function to creates a [AssociatedFactory] and the number of parsed bytes form a bytestream.
pub trait ByteFactory<B: ByteOrder> {
    /// Error when parsing the bytestream
    type Error: Error + 'static;
    /// Event given to the monitor
    type Factory: AssociatedFactory;
    #[allow(clippy::wrong_self_convention)]
    /// Function to parse the bytestream
    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Factory, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized;
}

/// The error returned if anything goes wrong when parsing the bytestream
#[derive(Debug)]
pub enum ByteParsingError<Inner: Error> {
    /// Parsing Error
    Inner(Inner),
    /// Error to inducate that the number of bytes is insuffienct to parse the event
    Incomplete,
}

/// This trait defines a factory to parse a bytestream to an event given to the monitor that is stateless.
pub trait FromBytes<Inner: ByteOrder> {
    /// Error when parsing the bytestream
    type Error: Error + 'static;
    /// Function to parse the bytestream
    fn from_bytes(data: &[u8]) -> Result<(Self, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized;
}

/// A struct to create a stateless parser that is build out of the [FromBytes] trait.
#[derive(Debug, Clone, Copy)]
pub struct StatelessByteFactory<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> {
    phantom: PhantomData<(B, Order)>,
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> Default for StatelessByteFactory<B, Order> {
    fn default() -> Self {
        Self {
            phantom: Default::default(),
        }
    }
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> ByteFactory<Order> for StatelessByteFactory<B, Order> {
    type Error = <B as FromBytes<Order>>::Error;
    type Factory = B;

    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Factory, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized,
    {
        B::from_bytes(data)
    }
}

impl<
        Source: NetworkSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + AssociatedFactory,
        const BUFFERSIZE: usize,
    > NetworkEventSource<Source, Order, StatelessByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [NetworkEventSource] given a [NetworkSource].
    pub fn from_source(source: Source) -> Self {
        Self {
            factory: StatelessByteFactory::default(),
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

impl<
        Source: NetworkSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + AssociatedFactory,
        const BUFFERSIZE: usize,
    > From<Source> for NetworkEventSource<Source, Order, StatelessByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    fn from(value: Source) -> Self {
        Self::from_source(value)
    }
}
