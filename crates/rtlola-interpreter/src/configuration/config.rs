//! This module contains all configuration related structures.

use std::marker::PhantomData;
use std::time::SystemTime;

use rtlola_frontend::RtLolaMir;

use crate::api::monitor::{Incremental, Input, VerdictRepresentation};
use crate::time::{OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::Monitor;
#[cfg(feature = "queued-api")]
use crate::QueuedMonitor;

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
#[derive(Clone, Debug)]
pub struct Config<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> {
    /// The representation of the specification
    pub ir: RtLolaMir,
    /// In which mode the evaluator is executed
    pub mode: ExecutionMode,
    /// Which format the time is given to the monitor
    pub input_time_representation: InputTime,
    /// Which format to use to output time
    pub output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    pub start_time: Option<SystemTime>,
}

/// A configuration struct containing all information (including type information) to initialize a Monitor.
#[derive(Debug, Clone)]
pub struct MonitorConfig<Source, SourceTime, Verdict = Incremental, VerdictTime = RelativeFloat>
where
    Source: Input,
    SourceTime: TimeRepresentation,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    config: Config<SourceTime, VerdictTime>,
    input: PhantomData<Source>,
    verdict: PhantomData<Verdict>,
}

impl<
        Source: Input + 'static,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > MonitorConfig<Source, SourceTime, Verdict, VerdictTime>
{
    /// Creates a new monitor config from a config
    pub fn new(config: Config<SourceTime, VerdictTime>) -> Self {
        Self {
            config,
            input: PhantomData::default(),
            verdict: PhantomData::default(),
        }
    }

    /// Returns the underlying configuration
    pub fn inner(&self) -> &Config<SourceTime, VerdictTime> {
        &self.config
    }

    /// Transforms the configuration into a [Monitor] using the provided data to setup the input source.
    pub fn monitor_with_data(self, data: Source::CreationData) -> Monitor<Source, SourceTime, Verdict, VerdictTime> {
        Monitor::setup(self.config, data)
    }

    /// Transforms the configuration into a [Monitor]
    pub fn monitor(self) -> Monitor<Source, SourceTime, Verdict, VerdictTime>
    where
        Source: Input<CreationData = ()>,
    {
        Monitor::setup(self.config, ())
    }

    #[cfg(feature = "queued-api")]
    /// Transforms the configuration into a [QueuedMonitor] using the provided data to setup the input source.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime> {
        QueuedMonitor::setup(self.config, data)
    }

    #[cfg(feature = "queued-api")]
    /// Transforms the configuration into a [QueuedMonitor]
    pub fn queued_monitor(self) -> QueuedMonitor<Source, SourceTime, Verdict, VerdictTime>
    where
        Source: Input<CreationData = ()>,
    {
        QueuedMonitor::setup(self.config, ())
    }
}

/// The execution mode of the interpreter. See the `README` for more details.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutionMode {
    /// Time provided by input source
    Offline,
    /// Time taken by evaluator
    Online,
}

impl Config<RelativeFloat, RelativeFloat> {
    /// Creates a new Debug config.
    pub fn debug(ir: RtLolaMir) -> Self {
        Config {
            ir,
            mode: ExecutionMode::Offline,
            input_time_representation: RelativeFloat::default(),
            output_time_representation: PhantomData::<RelativeFloat>::default(),
            start_time: None,
        }
    }
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> Config<InputTime, OutputTime> {
    /// Creates a new API config
    pub fn api(ir: RtLolaMir) -> Self {
        Config {
            ir,
            mode: ExecutionMode::Offline,
            input_time_representation: InputTime::default(),
            output_time_representation: PhantomData::default(),
            start_time: None,
        }
    }

    /// Turn the configuration into the [Monitor] API.
    pub fn monitor<Source: Input, Verdict: VerdictRepresentation>(
        self,
        data: Source::CreationData,
    ) -> Monitor<Source, InputTime, Verdict, OutputTime> {
        Monitor::setup(self, data)
    }
}
