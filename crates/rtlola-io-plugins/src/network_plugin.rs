use std::error::Error;
use std::fmt::Debug;
use std::marker::PhantomData;

use byteorder::ByteOrder;
use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::time::TimeRepresentation;
use time_converter::TimeConverter;

use crate::EventSource;

pub mod reader;
pub mod time_converter;
pub mod upd;

pub struct NetworkEventSource<
    Source: NetworkSource,
    Order: ByteOrder,
    Factory: ByteFactory<Order>,
    InputTime: TimeRepresentation,
    const BUFFERSIZE: usize,
> where
    <<Factory::Input as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    factory: Factory,
    source: Source,
    buffer: Vec<u8>,
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
    <<Factory::Input as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
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
    Factory(Factory),
    // Error while generating the time out of the bytestream
    InputMap(InputMap),
}

impl<Factory: Error + Debug, Source: Error + Debug, InputMap: Error + Debug> std::fmt::Display
    for NetworkEventSourceError<Factory, Source, InputMap>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkEventSourceError::Source(e) => write!(f, "{e}"),
            NetworkEventSourceError::Factory(e) => write!(f, "{e}"),
            NetworkEventSourceError::InputMap(e) => write!(f, "{e}"),
        }
    }
}

impl<Factory: Error + Debug + 'static, Source: Error + Debug + 'static, InputMap: Error + Debug + 'static>
    std::error::Error for NetworkEventSourceError<Factory, Source, InputMap>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            NetworkEventSourceError::Source(e) => Some(e),
            NetworkEventSourceError::Factory(e) => Some(e),
            NetworkEventSourceError::InputMap(e) => Some(e),
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
    <<Factory::Input as AssociatedFactory>::Factory as EventFactory>::Error: Error,
    Factory::Input: TimeConverter<InputTime>,
{
    type Error = NetworkEventSourceError<
        Factory::Error,
        Source::Error,
        <<Factory::Input as AssociatedFactory>::Factory as EventFactory>::Error,
    >;
    type Factory = Factory::Input;

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
                let ts = <<Factory as ByteFactory<Order>>::Input as TimeConverter<InputTime>>::convert_time(&event)
                    .map_err(NetworkEventSourceError::InputMap)?;
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
                        None => break Ok(None),
                        Some(package_size) => self.buffer.extend_from_slice(&temp_buffer[0..package_size]),
                    }
                },
                Err(ByteParsingError::Inner(e)) => break Err(NetworkEventSourceError::Factory(e)),
            }
        }
    }
}

pub trait NetworkSource {
    type Error: Error + 'static;
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

pub trait ByteFactory<B: ByteOrder> {
    type Error: Error + 'static;
    type Input: AssociatedFactory;
    #[allow(clippy::wrong_self_convention)]
    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Input, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized;
}

/// The error returned if anything goes wrong when parsing the bytestream
#[derive(Debug)]
pub enum ByteParsingError<R: Error> {
    /// Parsing Error
    Inner(R),
    /// Error to inducate that the number of bytes is insuffienct to parse the event
    Incomplete,
}

pub trait FromBytes<B: ByteOrder> {
    type Error: Error + 'static;
    fn from_bytes(data: &[u8]) -> Result<(Self, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized;
}

#[derive(Debug, Clone, Copy)]
pub struct EmptyByteFactory<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> {
    phantom: PhantomData<(B, Order)>,
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> std::default::Default for EmptyByteFactory<B, Order> {
    fn default() -> Self {
        Self {
            phantom: Default::default(),
        }
    }
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> ByteFactory<Order> for EmptyByteFactory<B, Order> {
    type Error = <B as FromBytes<Order>>::Error;
    type Input = B;

    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Input, usize), ByteParsingError<Self::Error>>
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
    > NetworkEventSource<Source, Order, EmptyByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: std::error::Error,
{
    pub fn from_source(source: Source) -> Self {
        Self {
            factory: EmptyByteFactory::default(),
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
    > From<Source> for NetworkEventSource<Source, Order, EmptyByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: std::error::Error,
{
    fn from(value: Source) -> Self {
        Self::from_source(value)
    }
}
