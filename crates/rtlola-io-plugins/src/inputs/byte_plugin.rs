//! Module that contains the implementation of the [EventSource] for bytes.
use std::error::Error;
use std::fmt::Debug;
use std::io::ErrorKind;
use std::marker::PhantomData;

use rtlola_interpreter::input::{AssociatedEventFactory, EventFactory};
use rtlola_interpreter::time::TimeRepresentation;
use serde::{Deserialize, Serialize};
use time_converter::TimeConverter;

use super::{EventResult, EventSource};

pub mod time_converter;
pub mod upd;

/// Receives and parses events from a [ByteParser] and a [ByteSource].
#[derive(Debug)]
pub struct ByteEventSource<
    Source: ByteSource,
    Parser: ByteParser<Output: AssociatedEventFactory>,
    InputTime: TimeRepresentation,
    const BUFFERSIZE: usize,
> where
    <<Parser::Output as AssociatedEventFactory>::Factory as EventFactory>::Error: Error,
{
    /// The factory to parse the monitor input given as bytearray
    parser: Parser,
    /// The connection that is used to receive data
    source: Source,
    /// The buffer that is used to store incoming data
    buffer: Vec<u8>,
    /// PhantomData that is used to propagate types
    timer: PhantomData<InputTime>,
}

impl<
        Source: ByteSource,
        Parser: ByteParser<Output: AssociatedEventFactory>,
        InputTime: TimeRepresentation,
        const BUFFERSIZE: usize,
    > ByteEventSource<Source, Parser, InputTime, BUFFERSIZE>
where
    <<Parser::Output as AssociatedEventFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [ByteEventSource] out of a (conncetion)[ByteSource] and a (parser)[ByteParser].
    pub fn new(source: Source, factory: Parser) -> Self {
        Self {
            parser: factory,
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

/// Enum to collect the errors with for a [ByteEventSource].
#[derive(Debug)]
pub enum ByteEventSourceError<Parse: Error + Debug, Source: Error + Debug, Time: Error + Debug> {
    /// Error while receiving a bytestream.
    Source(Source),
    /// Error while parsing a bytestream.
    Parse(Parse),
    /// Error while generating the time out of the bytestream
    TimeConversion(Time),
}

impl<Parse: Error + Debug, Source: Error + Debug, Time: Error + Debug> std::fmt::Display
    for ByteEventSourceError<Parse, Source, Time>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteEventSourceError::Source(e) => write!(f, "{e}"),
            ByteEventSourceError::Parse(e) => write!(f, "{e}"),
            ByteEventSourceError::TimeConversion(e) => write!(f, "{e}"),
        }
    }
}

impl<Parse: Error + Debug + 'static, Source: Error + Debug + 'static, Time: Error + Debug + 'static> Error
    for ByteEventSourceError<Parse, Source, Time>
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ByteEventSourceError::Source(e) => Some(e),
            ByteEventSourceError::Parse(e) => Some(e),
            ByteEventSourceError::TimeConversion(e) => Some(e),
        }
    }
}

impl<
        InputTime: TimeRepresentation,
        Parser: ByteParser<Output: AssociatedEventFactory<Factory: EventFactory<CreationData = ()>>>,
        Source: ByteSource + Debug,
        const BUFFERSIZE: usize,
    > EventSource<InputTime> for ByteEventSource<Source, Parser, InputTime, BUFFERSIZE>
where
    <<Parser::Output as AssociatedEventFactory>::Factory as EventFactory>::Error: Error,
    Parser::Output: TimeConverter<InputTime>,
{
    type Error = ByteEventSourceError<
        Parser::Error,
        Source::Error,
        <<Parser::Output as AssociatedEventFactory>::Factory as EventFactory>::Error,
    >;
    type Factory = Parser::Output;

    fn init_data(
        &self,
    ) -> Result<<<Self::Factory as AssociatedEventFactory>::Factory as EventFactory>::CreationData, Self::Error> {
        Ok(())
    }

    fn next_event(&mut self) -> EventResult<Self::Factory, <InputTime as TimeRepresentation>::InnerTime, Self::Error> {
        loop {
            let event = self.parser.from_bytes(&self.buffer).map(|(event, package_size)| {
                let ts = <<Parser as ByteParser>::Output as TimeConverter<InputTime>>::convert_time(&event)
                    .map_err(ByteEventSourceError::TimeConversion)?;
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
                Err(ByteParsingError::Inner(e)) => break Err(ByteEventSourceError::Parse(e)),
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
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// This trait defines a factory to parse a bytestream to an event given to the monitor.
/// It contains one function to creates an Output and the number of parsed bytes form a bytestream.
pub trait ByteParser {
    /// Error when parsing the bytestream
    type Error: Error + 'static;
    /// Event given to the monitor
    type Output;
    #[allow(clippy::wrong_self_convention)]
    /// Function to parse the bytestream
    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Output, usize), ByteParsingError<Self::Error>>
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

/// A struct to create a stateless parser that is build out of the [Serialize] and [Deserialize] trait.
#[derive(Debug, Clone, Copy)]
pub struct SerdeParser<B: Serialize + for<'a> Deserialize<'a> + AssociatedEventFactory> {
    phantom: PhantomData<B>,
}

impl<B: Serialize + for<'a> Deserialize<'a> + AssociatedEventFactory> Default for SerdeParser<B> {
    fn default() -> Self {
        Self {
            phantom: Default::default(),
        }
    }
}

impl<B: Serialize + for<'a> Deserialize<'a> + AssociatedEventFactory> ByteParser for SerdeParser<B> {
    type Error = bincode::Error;
    type Output = B;

    fn from_bytes(&mut self, data: &[u8]) -> Result<(Self::Output, usize), ByteParsingError<Self::Error>>
    where
        Self: Sized,
    {
        let res: B = bincode::deserialize(data).map_err(|e| {
            if matches!(ErrorKind::UnexpectedEof, _e) {
                ByteParsingError::Incomplete
            } else {
                ByteParsingError::Inner(e)
            }
        })?;
        let size = bincode::serialized_size(&res).map_err(ByteParsingError::Inner)? as usize;
        Ok((res, size))
    }
}

impl<
        Source: ByteSource,
        InputTime: TimeRepresentation,
        B: Serialize + for<'a> Deserialize<'a> + AssociatedEventFactory,
        const BUFFERSIZE: usize,
    > ByteEventSource<Source, SerdeParser<B>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedEventFactory>::Factory as EventFactory>::Error: Error,
{
    /// Creates a new [ByteEventSource] given a [ByteSource].
    pub fn from_source(source: Source) -> Self {
        Self {
            parser: SerdeParser::default(),
            source,
            buffer: Vec::new(),
            timer: PhantomData,
        }
    }
}

impl<
        Source: ByteSource,
        InputTime: TimeRepresentation,
        B: Serialize + for<'a> Deserialize<'a> + AssociatedEventFactory,
        const BUFFERSIZE: usize,
    > From<Source> for ByteEventSource<Source, SerdeParser<B>, InputTime, BUFFERSIZE>
where
    <<B as AssociatedEventFactory>::Factory as EventFactory>::Error: Error,
{
    fn from(value: Source) -> Self {
        Self::from_source(value)
    }
}
