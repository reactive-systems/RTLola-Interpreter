//! This module contains all configuration related structures.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use clap::ArgEnum;
use rtlola_frontend::RtLolaMir;

#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPInputSource;
use crate::basics::{CsvInputSource, OutputChannel, OutputHandler};
use crate::coordination::Controller;
use crate::monitor::{Input, VerdictRepresentation};
use crate::Monitor;
use crate::configuration::time::{TimeRepresentation, RelativeFloat, AbsoluteRfc};
use std::marker::PhantomData;

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
#[derive(Clone, Debug)]
pub struct Config<IT: TimeRepresentation, OT: TimeRepresentation> {
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
    pub mode: ExecutionMode<IT>,
    /// Which format to use to output time
    pub output_time_representation: PhantomData<OT>,
    /// The start time to assume
    pub start_time: Option<SystemTime>,
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
pub enum ExecutionMode<T: TimeRepresentation> {
    /// Time provided by input source
    Offline(PhantomData<*const T>),
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

impl<IT: TimeRepresentation, OT: TimeRepresentation> Config<IT, OT> {
    /// Creates a new Debug config.
    pub fn debug(ir: RtLolaMir) -> Config<RelativeFloat, AbsoluteRfc> {
        let mode = ExecutionMode::Offline(PhantomData::default());
        Config {
            ir,
            source: EventSourceConfig::Csv { src: CsvInputSource::stdin(None, mode) },
            statistics: Statistics::Debug,
            verbosity: Verbosity::Debug,
            output_channel: OutputChannel::StdOut,
            mode,
            output_time_representation: PhantomData::<AbsoluteRfc>::default(),
            start_time: None,
        }
    }

    /// Creates a new release config.
    pub fn release(
        ir: RtLolaMir,
        csv_path: String,
        output: OutputChannel,
        mode: ExecutionMode<IT>,
        start_time: Option<SystemTime>,
    ) -> Self {
        Config {
            ir,
            source: EventSourceConfig::Csv { src: CsvInputSource::file(PathBuf::from(csv_path), None, None, mode) },
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: output,
            mode,
            output_time_representation: PhantomData::<OT>::default(),
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
            mode: ExecutionMode::Offline(PhantomData::<IT>::default()),
            output_time_representation: PhantomData::<OT>::default(),
            start_time: None,
        }
    }

    /// Run the interpreter on this configuration.
    pub fn run(self) -> Result<Arc<OutputHandler>, Box<dyn std::error::Error>> {
        Controller::new(self).start()
    }

    /// Turn the configuration into the [Monitor] API.
    pub fn monitor<I: Input, V: VerdictRepresentation>(self, data: I::CreationData) -> Monitor<I, IT, V, OT> {
        Monitor::setup(self, data)
    }
}
