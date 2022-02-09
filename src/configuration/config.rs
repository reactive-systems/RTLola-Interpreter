//! This module contains all configuration related structures.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::ArgEnum;
use rtlola_frontend::RtLolaMir;

#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPInputSource;
use crate::basics::{CsvInputSource, OutputChannel, OutputHandler, RawTime};
use crate::coordination::Controller;
use crate::monitor::{Input, VerdictRepresentation};
use crate::Monitor;

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
#[derive(Clone, Debug)]
pub struct Config {
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
    /// Which format to use to output time
    pub output_time_representation: TimeRepresentation,
    /// The start time to assume
    pub start_time: Option<SystemTime>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Statistics {
    None,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutionMode {
    /// Time provided by input source
    Offline(TimeRepresentation),
    /// Time taken by evaluator
    Online,
}

#[derive(Debug, Clone)]
pub enum EventSourceConfig {
    Csv {
        src: CsvInputSource,
    },
    #[cfg(feature = "pcap_interface")]
    PCAP {
        src: PCAPInputSource,
    },
    Api,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeRepresentation {
    /// Relative to fixed start time
    /// Start time defaults to start time of the monitor
    Relative(RelativeTimeFormat),
    /// Relative time as a delta between two events
    /// Start time defaults to start time of the monitor
    Incremental(RelativeTimeFormat),
    /// Time provided as absolute timestamps
    /// Start time required. defaults to time of first event
    Absolute(AbsoluteTimeFormat),
}

impl TimeRepresentation {
    pub(crate) fn parse(&self, s: &str) -> Result<RawTime, String> {
        match self {
            TimeRepresentation::Relative(format) | TimeRepresentation::Incremental(format) => {
                format.parse_str(s).map(RawTime::Relative)
            }
            TimeRepresentation::Absolute(format) => format.parse_str(s).map(RawTime::Absolute),
        }
    }
}

impl std::convert::TryFrom<&str> for TimeRepresentation {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use TimeRepresentation::*;
        Ok(match value {
            "relative" | "relative_secs" | "relative_float_secs" => Relative(RelativeTimeFormat::FloatSecs),
            "relative_nanos" | "relative_uint_nanos" => Relative(RelativeTimeFormat::UIntNanos),
            "incremental" | "incremental_secs" | "incremental_float_secs" => Incremental(RelativeTimeFormat::FloatSecs),
            "incremental_nanos" | "incremental_uint_nanos" => Incremental(RelativeTimeFormat::UIntNanos),
            "absolute" | "absolute_rfc" | "absolute_rfc_3339" => Absolute(AbsoluteTimeFormat::Rfc3339),
            "absolute_unix_float" => Absolute(AbsoluteTimeFormat::UnixTimeFloat),
            _ => return Err(()),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelativeTimeFormat {
    /// Time delta given as nano seconds
    UIntNanos,
    /// Time delta given as seconds and sub-seconds
    FloatSecs,
}

impl RelativeTimeFormat {
    pub fn parse_str(&self, s: &str) -> Result<Duration, String> {
        match self {
            RelativeTimeFormat::UIntNanos => {
                let nanos = u64::from_str(s).map_err(|e| e.to_string())?;
                Ok(Duration::from_nanos(nanos))
            }
            RelativeTimeFormat::FloatSecs => match s.split_once('.') {
                Some((secs, nanos)) => {
                    let secs = u64::from_str(secs).map_err(|e| e.to_string())?;
                    let nanos = nanos
                        .char_indices()
                        .fold(Some(0u32), |val, (pos, c)| {
                            val.and_then(|val| c.to_digit(10).map(|c| val + c * (10u32.pow(8 - pos as u32))))
                        })
                        .ok_or("invalid character in number literal")?;
                    Ok(Duration::new(secs, nanos))
                }
                None => {
                    let secs = u64::from_str(s).map_err(|e| e.to_string())?;
                    Ok(Duration::from_secs(secs))
                }
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbsoluteTimeFormat {
    /// Time given as a string in the Rfc3339 format
    Rfc3339,
    /// Time given as a float representing the unix timestamp and a fraction of seconds
    UnixTimeFloat,
}

impl AbsoluteTimeFormat {
    pub(crate) fn parse_str(&self, s: &str) -> Result<SystemTime, String> {
        match self {
            AbsoluteTimeFormat::Rfc3339 => humantime::parse_rfc3339(s).map_err(|e| e.to_string()),
            AbsoluteTimeFormat::UnixTimeFloat => RelativeTimeFormat::FloatSecs.parse_str(s).map(|dur| UNIX_EPOCH + dur),
        }
    }
}

impl Config {
    pub fn debug(ir: RtLolaMir) -> Self {
        let mode = ExecutionMode::Offline(TimeRepresentation::Relative(RelativeTimeFormat::FloatSecs));
        Config {
            ir,
            source: EventSourceConfig::Csv { src: CsvInputSource::stdin(None, mode) },
            statistics: Statistics::Debug,
            verbosity: Verbosity::Debug,
            output_channel: OutputChannel::StdOut,
            mode,
            output_time_representation: TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339),
            start_time: None,
        }
    }

    pub fn release(
        ir: RtLolaMir,
        csv_path: String,
        output: OutputChannel,
        mode: ExecutionMode,
        output_time_representation: TimeRepresentation,
        start_time: Option<SystemTime>,
    ) -> Self {
        Config {
            ir,
            source: EventSourceConfig::Csv { src: CsvInputSource::file(PathBuf::from(csv_path), None, None, mode) },
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: output,
            mode,
            output_time_representation,
            start_time,
        }
    }

    pub fn api(ir: RtLolaMir, time_representation: TimeRepresentation) -> Self {
        Config {
            ir,
            source: EventSourceConfig::Api,
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: OutputChannel::None,
            mode: ExecutionMode::Offline(time_representation),
            output_time_representation: time_representation,
            start_time: None,
        }
    }

    pub fn run(self) -> Result<Arc<OutputHandler>, Box<dyn std::error::Error>> {
        Controller::new(self).start()
    }

    pub fn monitor<S: Input, V: VerdictRepresentation>(self) -> Monitor<S, V> {
        Monitor::setup(self)
    }
}
