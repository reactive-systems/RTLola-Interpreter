//! This module contains all configuration related structures.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use clap::ArgEnum;
use rtlola_frontend::RtLolaMir;

#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPInputSource;
use crate::basics::{CsvInputSource, CsvInputSourceKind, OutputChannel, OutputHandler};
use crate::coordination::Controller;
use crate::monitor::{Incremental, Input, VerdictRepresentation};
use crate::time::{OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::Monitor;
use std::marker::PhantomData;

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
#[derive(Clone, Debug)]
pub struct Config<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> {
    /// The representation of the specification
    pub ir: RtLolaMir,
    /// The source of events
    pub source: EventSourceConfig,
    /// A statistics module
    pub statistics: Statistics,
    /// The verbosity to use
    pub verbosity: Verbosity,
    /// Where the output should go
    pub output_channel: OutputChannel,
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
        Source: Input,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        VerdictTime: OutputTimeRepresentation,
    > MonitorConfig<Source, SourceTime, Verdict, VerdictTime>
{
    /// Creates a new monitor config from a config
    pub fn new(config: Config<SourceTime, VerdictTime>) -> Self {
        Self { config, input: PhantomData::default(), verdict: PhantomData::default() }
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
}

/// Used to define the level of statistics that should be computed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Statistics {
    /// No statistics will be computed
    None,
    /// All statistics will be computed
    Debug,
}

impl Default for Statistics {
    fn default() -> Self {
        Statistics::None
    }
}

impl From<Verbosity> for Statistics {
    fn from(v: Verbosity) -> Self {
        match v {
            Verbosity::Progress | Verbosity::Debug => Statistics::Debug,
            Verbosity::Silent | Verbosity::WarningsOnly | Verbosity::Triggers | Verbosity::Outputs => Statistics::None,
        }
    }
}

/// The different verbosities supported by the interpreter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, ArgEnum)]
pub enum Verbosity {
    /// Suppresses any kind of logging.
    Silent,
    /// Prints statistical information like number of events, triggers, etc.
    Progress,
    /// Prints nothing but runtime warnings about potentially critical states, e.g. dropped
    /// evaluation cycles.
    WarningsOnly,
    /// Prints only triggers and runtime warnings.
    Triggers,
    /// Prints information about all or a subset of output streams whenever they produce a new
    /// value.
    Outputs,
    /// Prints fine-grained debug information. Not suitable for production.
    Debug,
}

impl Default for Verbosity {
    fn default() -> Self {
        Verbosity::Triggers
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

/// The different supported input sources of the interpreter.
#[derive(Debug, Clone)]
pub enum EventSourceConfig {
    /// Parse events in CSV format
    Csv {
        /// The source of the CSV data.
        src: CsvInputSource,
    },

    /// Parse events from network packets
    #[cfg(feature = "pcap_interface")]
    PCAP {
        /// The source of the network packets.
        src: PCAPInputSource,
    },

    /// The API handles the input.
    Api,
}

impl Config<RelativeFloat, RelativeFloat> {
    /// Creates a new Debug config.
    pub fn debug(ir: RtLolaMir) -> Self {
        let mode = ExecutionMode::Offline;
        Config {
            ir,
            source: EventSourceConfig::Csv { src: CsvInputSource { time_col: None, kind: CsvInputSourceKind::StdIn } },
            statistics: Statistics::Debug,
            verbosity: Verbosity::Debug,
            output_channel: OutputChannel::StdOut,
            mode,
            input_time_representation: RelativeFloat::default(),
            output_time_representation: PhantomData::<RelativeFloat>::default(),
            start_time: None,
        }
    }
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation> Config<InputTime, OutputTime> {
    /// Creates a new release config.
    pub fn release(
        ir: RtLolaMir,
        csv_path: String,
        output: OutputChannel,
        mode: ExecutionMode,
        start_time: Option<SystemTime>,
    ) -> Self {
        Config {
            ir,
            source: EventSourceConfig::Csv {
                src: CsvInputSource { time_col: None, kind: CsvInputSourceKind::File(PathBuf::from(csv_path)) },
            },
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: output,
            mode,
            input_time_representation: InputTime::default(),
            output_time_representation: PhantomData::<OutputTime>::default(),
            start_time,
        }
    }

    /// Creates a new API config
    pub fn api(ir: RtLolaMir) -> Self {
        Config {
            ir,
            source: EventSourceConfig::Api,
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: OutputChannel::None,
            mode: ExecutionMode::Offline,
            input_time_representation: InputTime::default(),
            output_time_representation: PhantomData::default(),
            start_time: None,
        }
    }

    /// Run the interpreter on this configuration.
    pub fn run(self) -> Result<Arc<OutputHandler<OutputTime>>, Box<dyn std::error::Error>> {
        Controller::new(self).start()
    }

    /// Turn the configuration into the [Monitor] API.
    pub fn monitor<Source: Input, Verdict: VerdictRepresentation>(
        self,
        data: Source::CreationData,
    ) -> Monitor<Source, InputTime, Verdict, OutputTime> {
        Monitor::setup(self, data)
    }
}
