//! This module contains all configuration related structures.

use std::marker::PhantomData;
use std::time::SystemTime;

use rtlola_frontend::RtLolaMir;

use crate::input::{EventFactory, EventFactoryError};
use crate::monitor::{Incremental, VerdictRepresentation};
use crate::time::{OutputTimeRepresentation, RealTime, RelativeFloat, TimeRepresentation};
use crate::Monitor;
#[cfg(feature = "queued-api")]
use crate::QueuedMonitor;

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
#[derive(Clone, Debug)]
pub struct Config<Mode: ExecutionMode, OutputTime: OutputTimeRepresentation> {
    /// The representation of the specification
    pub ir: RtLolaMir,
    /// In which mode the evaluator is executed
    pub mode: Mode,
    /// Which format to use to output time
    pub output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    pub start_time: Option<SystemTime>,
}

/// The execution mode of the interpreter. See the `README` for more details.
pub trait ExecutionMode: Default {
    /// The TimeRepresentation used during execution.
    type SourceTime: TimeRepresentation;

    /// Creates a new ExecutionMode with initíalized Time.
    fn new(time: Self::SourceTime) -> Self;

    /// Returns the inner TimeRepresentation.
    fn time_representation(&self) -> Self::SourceTime;
}

/// Time is taken by the monitor.
#[derive(Debug, Copy, Clone, Default)]
pub struct OnlineMode {
    input_time_representation: RealTime,
}
impl ExecutionMode for OnlineMode {
    type SourceTime = RealTime;

    fn new(time: Self::SourceTime) -> Self {
        Self {
            input_time_representation: time,
        }
    }

    fn time_representation(&self) -> Self::SourceTime {
        self.input_time_representation
    }
}

/// Time is provided by the input source in the format defined by the [TimeRepresentation].
#[derive(Debug, Copy, Clone, Default)]
pub struct OfflineMode<InputTime: TimeRepresentation> {
    input_time_representation: InputTime,
}
impl<InputTime: TimeRepresentation> ExecutionMode for OfflineMode<InputTime> {
    type SourceTime = InputTime;

    fn new(time: Self::SourceTime) -> Self {
        Self {
            input_time_representation: time,
        }
    }

    fn time_representation(&self) -> Self::SourceTime {
        self.input_time_representation.clone()
    }
}

/// A configuration struct containing all information (including type information) to initialize a Monitor.
#[derive(Debug, Clone)]
pub struct MonitorConfig<Source, Mode, Verdict = Incremental, VerdictTime = RelativeFloat>
where
    Source: EventFactory,
    Mode: ExecutionMode,
    Verdict: VerdictRepresentation,
    VerdictTime: OutputTimeRepresentation,
{
    config: Config<Mode, VerdictTime>,
    input: PhantomData<Source>,
    verdict: PhantomData<Verdict>,
}

impl<
        Source: EventFactory + 'static,
        Mode: ExecutionMode,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > MonitorConfig<Source, Mode, Verdict, VerdictTime>
{
    /// Creates a new monitor config from a config
    pub fn new(config: Config<Mode, VerdictTime>) -> Self {
        Self {
            config,
            input: PhantomData,
            verdict: PhantomData,
        }
    }

    /// Returns the underlying configuration
    pub fn inner(&self) -> &Config<Mode, VerdictTime> {
        &self.config
    }

    /// Transforms the configuration into a [Monitor] using the provided data to setup the input source.
    pub fn monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> Result<Monitor<Source, Mode, Verdict, VerdictTime>, EventFactoryError> {
        Monitor::setup(self.config, data)
    }

    /// Transforms the configuration into a [Monitor]
    pub fn monitor(self) -> Result<Monitor<Source, Mode, Verdict, VerdictTime>, EventFactoryError>
    where
        Source: EventFactory<CreationData = ()>,
    {
        Monitor::setup(self.config, ())
    }
}

#[cfg(feature = "queued-api")]
impl<
        Source: EventFactory + 'static,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > MonitorConfig<Source, OfflineMode<SourceTime>, Verdict, VerdictTime>
{
    /// Transforms the configuration into a [QueuedMonitor] using the provided data to setup the input source.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, VerdictTime> {
        <QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, VerdictTime>>::setup(self.config, data)
    }

    /// Transforms the configuration into a [QueuedMonitor]
    pub fn queued_monitor(self) -> QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, VerdictTime>
    where
        Source: EventFactory<CreationData = ()>,
    {
        <QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, VerdictTime>>::setup(self.config, ())
    }
}

#[cfg(feature = "queued-api")]
impl<Source: EventFactory + 'static, Verdict: VerdictRepresentation, VerdictTime: OutputTimeRepresentation>
    MonitorConfig<Source, OnlineMode, Verdict, VerdictTime>
{
    /// Transforms the configuration into a [QueuedMonitor] using the provided data to setup the input source.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, OnlineMode, Verdict, VerdictTime> {
        <QueuedMonitor<Source, OnlineMode, Verdict, VerdictTime>>::setup(self.config, data)
    }

    /// Transforms the configuration into a [QueuedMonitor]
    pub fn queued_monitor(self) -> QueuedMonitor<Source, OnlineMode, Verdict, VerdictTime>
    where
        Source: EventFactory<CreationData = ()>,
    {
        <QueuedMonitor<Source, OnlineMode, Verdict, VerdictTime>>::setup(self.config, ())
    }
}

impl Config<OfflineMode<RelativeFloat>, RelativeFloat> {
    /// Creates a new Debug config.
    pub fn debug(ir: RtLolaMir) -> Self {
        Config {
            ir,
            mode: OfflineMode::default(),
            output_time_representation: PhantomData,
            start_time: None,
        }
    }
}

impl<Mode: ExecutionMode, OutputTime: OutputTimeRepresentation> Config<Mode, OutputTime> {
    /// Creates a new API config
    pub fn api(ir: RtLolaMir) -> Config<OfflineMode<RelativeFloat>, OutputTime> {
        Config {
            ir,
            mode: OfflineMode::default(),
            output_time_representation: PhantomData,
            start_time: None,
        }
    }

    /// Turn the configuration into the [Monitor] API.
    pub fn monitor<Source: EventFactory, Verdict: VerdictRepresentation>(
        self,
        data: Source::CreationData,
    ) -> Result<Monitor<Source, Mode, Verdict, OutputTime>, EventFactoryError> {
        Monitor::setup(self, data)
    }
}
