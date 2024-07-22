#[cfg(feature = "byte_plugin")]
pub mod byte_plugin;
#[cfg(feature = "csv_plugin")]
pub mod csv_plugin;
#[cfg(feature = "json_plugin")]
pub mod json_plugin;
#[cfg(feature = "log_printer")]
pub mod log_printer;
#[cfg(feature = "statistics_plugin")]
pub mod statistics_plugin;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::error::Error;
use std::io::Write;
use std::marker::PhantomData;

use rtlola_interpreter::monitor::{VerdictRepresentation, Verdicts};
use rtlola_interpreter::output::{NewVerdictFactory, VerdictFactory};
use rtlola_interpreter::rtlola_frontend::tag_parser::verbosity_parser::{
    DebugParser, StreamVerbosity, VerbosityParser,
};
use rtlola_interpreter::rtlola_frontend::tag_parser::TagValidator;
use rtlola_interpreter::rtlola_frontend::{RtLolaError, RtLolaMir};
use rtlola_interpreter::rtlola_mir::{OutputKind, StreamReference};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeRepresentation};

/// Struct for a generic factory returning the monitor output
#[derive(Debug)]
pub struct VerdictRepresentationFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    phantom: PhantomData<(MonitorOutput, OutputTime)>,
}

impl<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> Default
    for VerdictRepresentationFactory<MonitorOutput, OutputTime>
{
    fn default() -> Self {
        Self {
            phantom: Default::default(),
        }
    }
}

impl<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation>
    VerdictFactory<MonitorOutput, OutputTime> for VerdictRepresentationFactory<MonitorOutput, OutputTime>
{
    type Error = Infallible;
    type Verdict = (MonitorOutput, OutputTime::InnerTime);

    fn get_verdict(&mut self, rec: MonitorOutput, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error> {
        Ok((rec, ts))
    }
}

impl<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation>
    NewVerdictFactory<MonitorOutput, OutputTime> for VerdictRepresentationFactory<MonitorOutput, OutputTime>
{
    type CreationData = ();

    fn new(_ir: &RtLolaMir, _data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self {
            phantom: Default::default(),
        })
    }
}

/// The main trait that has to be implemented by an output plugin
pub trait VerdictsSink<V: VerdictRepresentation, T: OutputTimeRepresentation> {
    /// Error Type of a [VerdictsSink] implementation
    type Error: Error + 'static;
    /// Return Type of a [VerdictsSink] implementation
    type Return;
    /// Factory Type to convert the monitor output to the required representation
    type Factory: VerdictFactory<V, T>;

    /// Defines how the verdicts of the monitor needs to be handled
    fn sink_verdicts(
        &mut self,
        verdicts: Verdicts<V, T>,
    ) -> Result<Vec<Self::Return>, VerdictSinkError<Self::Error, <Self::Factory as VerdictFactory<V, T>>::Error>> {
        let Verdicts { timed, event, ts } = verdicts;
        timed
            .into_iter()
            .chain(vec![(ts, event)])
            .map(|(ts, verdict)| self.sink_verdict(ts, verdict))
            .collect::<Result<Vec<_>, _>>()
    }
    /// Defines how one verdict of the monitor needs to be handled, timed and event-based
    fn sink_verdict(
        &mut self,
        ts: <T as TimeRepresentation>::InnerTime,
        verdict: V,
    ) -> Result<Self::Return, VerdictSinkError<Self::Error, <Self::Factory as VerdictFactory<V, T>>::Error>> {
        let verdict = self
            .factory()
            .get_verdict(verdict, ts)
            .map_err(VerdictSinkError::Factory)?;
        self.sink(verdict).map_err(VerdictSinkError::Sink)
    }

    /// Function to dispatch the converted verdict to the sink
    fn sink(&mut self, verdict: <Self::Factory as VerdictFactory<V, T>>::Verdict) -> Result<Self::Return, Self::Error>;

    /// Function to return a reference to the Verdictfactory
    fn factory(&mut self) -> &mut Self::Factory;
}

#[derive(Debug)]
/// A generic Error to be used by [VerdictFactory]s
pub enum VerdictSinkError<SinkError: Error + 'static, FactoryError: Error + 'static> {
    #[allow(missing_docs)]
    Sink(SinkError),
    #[allow(missing_docs)]
    Factory(FactoryError),
}

impl<SinkError: Error, FactoryError: Error> std::fmt::Display for VerdictSinkError<SinkError, FactoryError> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerdictSinkError::Sink(e) => write!(f, "{}", e),
            VerdictSinkError::Factory(e) => write!(f, "{}", e),
        }
    }
}

impl<SinkError: Error, FactoryError: Error> Error for VerdictSinkError<SinkError, FactoryError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            VerdictSinkError::Sink(e) => Some(e),
            VerdictSinkError::Factory(e) => Some(e),
        }
    }
}

/// Generic VerdictSink that accepts verdicts as bytes and writes them to a Writer.
#[derive(Debug)]
pub struct ByteSink<
    W: Write,
    Factory: VerdictFactory<MonitorOutput, OutputTime>,
    MonitorOutput: VerdictRepresentation,
    OutputTime: OutputTimeRepresentation,
> {
    factory: Factory,
    writer: W,
    output: PhantomData<MonitorOutput>,
    time: PhantomData<OutputTime>,
}
impl<
        W: Write,
        Factory: VerdictFactory<MonitorOutput, OutputTime, Verdict = Vec<u8>>,
        MonitorOutput: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
    > ByteSink<W, Factory, MonitorOutput, OutputTime>
{
    /// Create a new [ByteSink] that receives bytes and forwards them to a writer
    pub fn new(writer: W, factory: Factory) -> Self {
        Self {
            factory,
            writer,
            output: PhantomData,
            time: PhantomData,
        }
    }
}

impl<
        W: Write,
        Factory: VerdictFactory<MonitorOutput, OutputTime, Verdict = Vec<u8>>,
        MonitorOutput: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
    > VerdictsSink<MonitorOutput, OutputTime> for ByteSink<W, Factory, MonitorOutput, OutputTime>
{
    type Error = std::io::Error;
    type Factory = Factory;
    type Return = ();

    fn sink(&mut self, verdict: Vec<u8>) -> Result<Self::Return, Self::Error> {
        self.writer.write_all(&verdict[..])?;
        self.writer.flush()?;
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// A sink implementation that is completely discarding the verdicts
#[derive(Copy, Clone, Debug)]
pub struct DiscardSink<O: OutputTimeRepresentation, V: VerdictRepresentation, F: VerdictFactory<V, O, Verdict = ()>> {
    factory: F,
    verdict: PhantomData<(O, V)>,
}

impl<O: OutputTimeRepresentation, V: VerdictRepresentation, F: VerdictFactory<V, O, Verdict = ()>>
    DiscardSink<O, V, F>
{
    /// Creates a new [DiscardSink] for a factory that returns `()` as a verdict.
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            verdict: Default::default(),
        }
    }
}

impl<O: OutputTimeRepresentation, V: VerdictRepresentation> Default for DiscardSink<O, V, EmptyFactory> {
    fn default() -> Self {
        Self::new(EmptyFactory)
    }
}

impl<O: OutputTimeRepresentation, V: VerdictRepresentation, F: VerdictFactory<V, O, Verdict = ()>> VerdictsSink<V, O>
    for DiscardSink<O, V, F>
{
    type Error = Infallible;
    type Factory = F;
    type Return = ();

    fn sink(
        &mut self,
        _verdict: <Self::Factory as VerdictFactory<V, O>>::Verdict,
    ) -> Result<Self::Return, Self::Error> {
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// A factory implementation that does nothing
#[derive(Default, Copy, Clone, Debug)]
pub struct EmptyFactory;

impl<V: VerdictRepresentation, O: OutputTimeRepresentation> VerdictFactory<V, O> for EmptyFactory {
    type Error = Infallible;
    type Verdict = ();

    fn get_verdict(&mut self, _rec: V, _ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        Ok(())
    }
}

impl<V: VerdictRepresentation, O: OutputTimeRepresentation> NewVerdictFactory<V, O> for EmptyFactory {
    type CreationData = ();

    fn new(_ir: &RtLolaMir, _data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self)
    }
}

/// Represents the verbosity and debug configuration of the streams in the specification to be used with the output plugins.
pub struct CliAnnotations {
    stream_verbosity: HashMap<StreamReference, StreamVerbosity>,
    debug_streams: HashSet<StreamReference>,
}

impl CliAnnotations {
    /// Parses the annotated tags in the specification to build the [CliAnnotations]
    pub fn new(ir: &RtLolaMir) -> Result<CliAnnotations, RtLolaError> {
        let verbosity_parser = VerbosityParser;
        let debug_parser = DebugParser;

        let stream_verbosity = ir.parse_tags(verbosity_parser)?;
        let stream_verbosity = ir
            .all_streams()
            .map(|sr| {
                let verbosity = stream_verbosity
                    .local_tags(sr)
                    .unwrap()
                    .as_ref()
                    .copied()
                    .unwrap_or_else(|| {
                        match sr {
                            StreamReference::In(_) => StreamVerbosity::Streams,
                            StreamReference::Out(_) => {
                                match ir.output(sr).kind {
                                    OutputKind::NamedOutput(_) => StreamVerbosity::Outputs,
                                    OutputKind::Trigger(_) => StreamVerbosity::Warnings,
                                }
                            },
                        }
                    });
                (sr, verbosity)
            })
            .collect();
        let debug_tags = ir.parse_tags(debug_parser)?;
        let debug_streams = ir
            .all_streams()
            .filter(|sr| *debug_tags.local_tags(*sr).unwrap())
            .collect();

        Ok(Self {
            stream_verbosity,
            debug_streams,
        })
    }

    /// Returns the tag parsers applied by the [Self::new] call
    pub fn parsers<'a>() -> &'a [&'a dyn TagValidator] {
        &[&VerbosityParser, &DebugParser]
    }

    /// Mark a set of streams as additional debug streams
    pub fn add_debug_streams(mut self, stream: &[StreamReference]) -> Self {
        self.debug_streams.extend(stream);
        self
    }

    fn verbosity(&self, sr: StreamReference) -> StreamVerbosity {
        *self.stream_verbosity.get(&sr).unwrap()
    }

    fn debug(&self, sr: StreamReference) -> bool {
        self.debug_streams.contains(&sr)
    }

    fn has_debug(&self) -> bool {
        !self.debug_streams.is_empty()
    }
}
