//! Module that contains the implementation of the [EventSource] and [VerdictsSink] for bytes
use std::convert::Infallible;
use std::error::Error;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::time::Duration;

use byteorder::ByteOrder;
use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::monitor::{TriggerMessages, VerdictRepresentation};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeRepresentation};
use time_converter::TimeConverter;

use crate::{EventSource, VerdictFactory, VerdictsSink};

pub mod time_converter;
pub mod upd;

/// Receives and parses events from a [EventByteFactory] and a [ByteSource].
#[derive(Debug)]
pub struct ByteEventSource<
    Source: ByteSource,
    Order: ByteOrder,
    Factory: EventByteFactory<Order>,
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
        Source: ByteSource,
        Order: ByteOrder,
        Factory: EventByteFactory<Order>,
        InputTime: TimeRepresentation,
        const BUFFERSIZE: usize,
    > ByteEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
where
    <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [ByteEventSource] out of a (conncetion)[ByteSource] and a (parser)[EventByteFactory].
    pub fn new(source: Source, factory: Factory) -> Self {
        Self {
            factory,
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

/// Enum to collect the errors with for a [ByteEventSource].
#[derive(Debug)]
pub enum ByteEventSourceError<EventFactory: Error + Debug, Source: Error + Debug, ByteFactory: Error + Debug> {
    /// Error while receiving a bytestream.
    Source(Source),
    /// Error while parsing a bytestream.
    EventFactory(EventFactory),
    /// Error while generating the time out of the bytestream
    ByteFactory(ByteFactory),
}

impl<EventFactory: Error + Debug, Source: Error + Debug, ByteFactory: Error + Debug> std::fmt::Display
    for ByteEventSourceError<EventFactory, Source, ByteFactory>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteEventSourceError::Source(e) => write!(f, "{e}"),
            ByteEventSourceError::EventFactory(e) => write!(f, "{e}"),
            ByteEventSourceError::ByteFactory(e) => write!(f, "{e}"),
        }
    }
}

impl<EventFactory: Error + Debug + 'static, Source: Error + Debug + 'static, ByteFactory: Error + Debug + 'static> Error
    for ByteEventSourceError<EventFactory, Source, ByteFactory>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ByteEventSourceError::Source(e) => Some(e),
            ByteEventSourceError::EventFactory(e) => Some(e),
            ByteEventSourceError::ByteFactory(e) => Some(e),
        }
    }
}

impl<
        InputTime: TimeRepresentation,
        Factory: EventByteFactory<Order, Factory: AssociatedFactory<Factory: EventFactory<CreationData = ()>>>,
        Source: ByteSource + Debug,
        Order: ByteOrder,
        const BUFFERSIZE: usize,
    > EventSource<InputTime> for ByteEventSource<Source, Order, Factory, InputTime, BUFFERSIZE>
where
    <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error: Error,
    Factory::Factory: TimeConverter<InputTime>,
{
    type Error = ByteEventSourceError<
        Factory::Error,
        Source::Error,
        <<Factory::Factory as AssociatedFactory>::Factory as EventFactory>::Error,
    >;
    type Factory = Factory::Factory;

    fn init_data(
        &self,
    ) -> Result<<<Self::Factory as AssociatedFactory>::Factory as EventFactory>::CreationData, Self::Error> {
        Ok(())
    }

    fn next_event(
        &mut self,
    ) -> crate::EventResult<Self::Factory, <InputTime as TimeRepresentation>::InnerTime, Self::Error> {
        loop {
            let event = self.factory.from_bytes(&self.buffer).map(|(event, package_size)| {
                let ts =
                    <<Factory as EventByteFactory<Order>>::Factory as TimeConverter<InputTime>>::convert_time(&event)
                        .map_err(ByteEventSourceError::ByteFactory)?;
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
                        .map_err(ByteEventSourceError::Source)?;
                    match package_size {
                        None | Some(0) => break Ok(None),
                        Some(package_size) => self.buffer.extend_from_slice(&temp_buffer[0..package_size]),
                    }
                },
                Err(ByteParsingError::Inner(e)) => break Err(ByteEventSourceError::EventFactory(e)),
            }
        }
    }
}

/// Trait to collect all connections for the [ByteEventSource]
pub trait ByteSource {
    /// Error when receiving the bytestream
    type Error: Error + 'static;
    /// Function to receive the bytestream and returns the number of parsed bytes
    fn read(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;
}

impl<T: std::io::Read> ByteSource for T {
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
pub trait EventByteFactory<B: ByteOrder> {
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
pub struct StatelessEventByteFactory<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> {
    phantom: PhantomData<(B, Order)>,
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> Default for StatelessEventByteFactory<B, Order> {
    fn default() -> Self {
        Self {
            phantom: Default::default(),
        }
    }
}

impl<B: FromBytes<Order> + AssociatedFactory, Order: ByteOrder> EventByteFactory<Order>
    for StatelessEventByteFactory<B, Order>
{
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
        Source: ByteSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + AssociatedFactory,
        const BUFFERSIZE: usize,
    > ByteEventSource<Source, Order, StatelessEventByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [ByteEventSource] given a [ByteSource].
    pub fn from_source(source: Source) -> Self {
        Self {
            factory: StatelessEventByteFactory::default(),
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

impl<
        Source: ByteSource,
        Order: ByteOrder,
        InputTime: TimeRepresentation,
        B: FromBytes<Order> + AssociatedFactory,
        const BUFFERSIZE: usize,
    > From<Source> for ByteEventSource<Source, Order, StatelessEventByteFactory<B, Order>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedFactory>::Factory as EventFactory>::Error: Error,
{
    fn from(value: Source) -> Self {
        Self::from_source(value)
    }
}

/// Parses and send bytes with a [VerdictByteFactory] and a [ByteTarget].
#[derive(Debug)]
pub struct ByteVerdictSink<
    V: VerdictRepresentation,
    T: OutputTimeRepresentation,
    B: ByteOrder,
    Factory: VerdictByteFactory<B, V, T>,
    Target: ByteTarget,
> {
    factory: Factory,
    target: Target,
    phantom: PhantomData<(V, T, B)>,
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        B: ByteOrder,
        Factory: VerdictByteFactory<B, V, T>,
        Target: ByteTarget,
    > ByteVerdictSink<V, T, B, Factory, Target>
{
    /// Creates a new [ByteVerdictSink] out of a (target)[ByteTarget] and a (parser)[VerdictByteFactory].
    pub fn new(target: Target, factory: Factory) -> Self {
        Self {
            factory,
            target,
            phantom: PhantomData,
        }
    }
}

impl<
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        B: ByteOrder,
        Factory: VerdictByteFactory<B, V, T>,
        Target: ByteTarget,
    > VerdictsSink<V, T> for ByteVerdictSink<V, T, B, Factory, Target>
{
    type Error = ByteVerdictSinkError<<Factory as VerdictByteFactory<B, V, T>>::Error, Target::Error>;
    type Factory = Factory;
    type Return = ();

    fn sink(&mut self, verdict: <Self::Factory as VerdictFactory<V, T>>::Verdict) -> Result<Self::Return, Self::Error> {
        let buffer = self
            .factory
            .into_bytes(verdict)
            .map_err(ByteVerdictSinkError::Factory)?;
        self.target.write(&buffer).map_err(ByteVerdictSinkError::Target)
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// Enum to collect the errors with for a [ByteVerdictSink].
#[derive(Debug)]
pub enum ByteVerdictSinkError<Factory: Error + 'static, Target: Error + 'static> {
    #[allow(missing_docs)]
    Factory(Factory),
    #[allow(missing_docs)]
    Target(Target),
}

impl<Factory: Error + 'static, Target: Error + 'static> std::fmt::Display for ByteVerdictSinkError<Factory, Target> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteVerdictSinkError::Factory(e) => writeln!(f, "{e}"),
            ByteVerdictSinkError::Target(e) => writeln!(f, "{e}"),
        }
    }
}

impl<Factory: Error + 'static, Target: Error + 'static> Error for ByteVerdictSinkError<Factory, Target> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ByteVerdictSinkError::Factory(e) => Some(e),
            ByteVerdictSinkError::Target(e) => Some(e),
        }
    }
}

/// Trait to collect all connections for the [ByteEventSource]
pub trait ByteTarget {
    /// Error when receiving the bytestream
    type Error: Error + 'static;
    /// Function to receive the bytestream and returns the number of parsed bytes
    fn write(&mut self, buffer: &[u8]) -> Result<(), Self::Error>;
}

impl<T: std::io::Write> ByteTarget for T {
    type Error = std::io::Error;

    fn write(&mut self, buffer: &[u8]) -> Result<(), Self::Error> {
        std::io::Write::write_all(self, buffer)
    }
}

/// This trait defines a factory to create a bytestream from a verdict
pub trait VerdictByteFactory<B: ByteOrder, V: VerdictRepresentation, T: OutputTimeRepresentation>:
    VerdictFactory<V, T>
{
    /// Error when creating the bytestream
    type Error: Error + 'static;
    /// Function to create the bytestream
    fn into_bytes(
        &mut self,
        verdict: <Self as VerdictFactory<V, T>>::Verdict,
    ) -> Result<Vec<u8>, <Self as VerdictByteFactory<B, V, T>>::Error>;
}

/// This trait defines a factory to create a bytestream to an verdict that is stateless.
pub trait ToBytes<Inner: ByteOrder> {
    /// Error when parsing the bytestream
    type Error: Error + 'static;
    /// Function to create the bytestream
    fn to_bytes(self) -> Result<Vec<u8>, Self::Error>
    where
        Self: Sized;
}

/// A struct to create a stateless byte parser that is build out of the [ToBytes] and [VerdictFactory] trait.
#[derive(Debug, Clone, Copy)]
pub struct StatelessVerdictByteFactory<
    B: ByteOrder,
    V: VerdictRepresentation,
    T: OutputTimeRepresentation,
    Factory: VerdictFactory<V, T, Verdict: ToBytes<B>>,
> {
    factory: Factory,
    phantom: PhantomData<(B, V, T)>,
}

impl<
        B: ByteOrder,
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T, Verdict: ToBytes<B>>,
    > StatelessVerdictByteFactory<B, V, T, Factory>
{
    /// Create a new [StatelessVerdictByteFactory] from a [VerdictFactory]
    pub fn new(factory: Factory) -> Self {
        Self {
            factory,
            phantom: PhantomData,
        }
    }
}

impl<
        B: ByteOrder,
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T, Verdict: ToBytes<B>>,
    > VerdictFactory<V, T> for StatelessVerdictByteFactory<B, V, T, Factory>
{
    type Error = Factory::Error;
    type Verdict = Factory::Verdict;

    fn get_verdict(&mut self, rec: V, ts: <T>::InnerTime) -> Result<Self::Verdict, Self::Error> {
        self.factory.get_verdict(rec, ts)
    }
}

impl<
        B: ByteOrder,
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T, Verdict: ToBytes<B>>,
    > VerdictByteFactory<B, V, T> for StatelessVerdictByteFactory<B, V, T, Factory>
{
    type Error = <Factory::Verdict as ToBytes<B>>::Error;

    fn into_bytes(
        &mut self,
        verdict: <Self as VerdictFactory<V, T>>::Verdict,
    ) -> Result<Vec<u8>, <Self as VerdictByteFactory<B, V, T>>::Error> {
        verdict.to_bytes()
    }
}

impl<
        B: ByteOrder,
        V: VerdictRepresentation,
        T: OutputTimeRepresentation,
        Factory: VerdictFactory<V, T, Verdict: ToBytes<B>>,
    > From<Factory> for StatelessVerdictByteFactory<B, V, T, Factory>
{
    fn from(value: Factory) -> Self {
        StatelessVerdictByteFactory {
            factory: value,
            phantom: PhantomData,
        }
    }
}

impl<B: ByteOrder> ToBytes<B> for (TriggerMessages, Duration) {
    type Error = Infallible;

    fn to_bytes(self) -> Result<Vec<u8>, Self::Error>
    where
        Self: Sized,
    {
        Ok(self
            .0
            .into_iter()
            .map(|(_sr, _parameter, msg)| format!("{}\n", msg).as_bytes().to_vec())
            .flatten()
            .collect())
    }
}
