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

#[cfg(feature = "byte_plugin")]
pub mod byte_plugin;

pub mod log_printer;

#[cfg(feature = "jsonl_plugin")]
pub mod jsonl_plugin;

use std::convert::Infallible;
use std::error::Error;
use std::marker::PhantomData;

use rtlola_interpreter::input::{AssociatedFactory, EventFactory};
use rtlola_interpreter::monitor::{VerdictRepresentation, Verdicts};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeRepresentation};

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

/// This trait provides the functionally to convert the monitor output.
/// You can either implement this trait for your own datatype or use one of the predefined output methods.
/// See [VerdictRepresentationFactory]
pub trait VerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    /// Type of the expected Output representation
    type Verdict;
    /// Error when converting the monitor output to the verdict
    type Error: Error + 'static;

    /// This function converts a monitor to a verdict.
    fn get_verdict(&mut self, rec: MonitorOutput, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error>;
}

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

/// The main trait that has to be implemented by an output plugin
pub trait VerdictsSink<V: VerdictRepresentation, T: OutputTimeRepresentation> {
    /// Error Type of an [VerdictsSink] implementation
    type Error: Error + 'static;
    /// Return Type of an [VerdictsSink] implementation
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
