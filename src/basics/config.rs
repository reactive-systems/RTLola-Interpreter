use super::{EventSourceConfig, OutputChannel};
use crate::basics::io_handler::RawTime;
use crate::basics::CsvInputSource;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct EvalConfig {
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutionMode {
    /// Time provided by input source
    Offline(TimeRepresentation),
    /// Time taken by evaluator
    Online,
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
            "absolute_unix_nanos" | "absolute_unix_uint_nanos" => Absolute(AbsoluteTimeFormat::UnixTimeNanos),
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
    pub(crate) fn parse_str(&self, s: &str) -> Result<Duration, String> {
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
    /// Time given as a unsigned integer representing the unix timestamp in nano seconds
    UnixTimeNanos,
    /// Time given as a float representing the unix timestamp and a fraction of seconds
    UnixTimeFloat,
}

impl AbsoluteTimeFormat {
    pub(crate) fn parse_str(&self, s: &str) -> Result<SystemTime, String> {
        match self {
            AbsoluteTimeFormat::Rfc3339 => humantime::parse_rfc3339(s).map_err(|e| e.to_string()),
            AbsoluteTimeFormat::UnixTimeNanos => RelativeTimeFormat::UIntNanos.parse_str(s).map(|dur| UNIX_EPOCH + dur),
            AbsoluteTimeFormat::UnixTimeFloat => RelativeTimeFormat::FloatSecs.parse_str(s).map(|dur| UNIX_EPOCH + dur),
        }
    }
}

impl EvalConfig {
    pub fn new(
        source: EventSourceConfig,
        statistics: Statistics,
        verbosity: Verbosity,
        output: OutputChannel,
        mode: ExecutionMode,
        output_time_representation: TimeRepresentation,
        start_time: Option<SystemTime>,
    ) -> Self {
        EvalConfig {
            source,
            statistics,
            verbosity,
            output_channel: output,
            mode,
            output_time_representation,
            start_time,
        }
    }

    pub fn debug() -> Self {
        EvalConfig { statistics: Statistics::Debug, verbosity: Verbosity::Debug, ..Default::default() }
    }

    pub fn release(
        path: String,
        output: OutputChannel,
        mode: ExecutionMode,
        output_time_representation: TimeRepresentation,
        start_time: Option<SystemTime>,
    ) -> Self {
        EvalConfig::new(
            EventSourceConfig::Csv { src: CsvInputSource::file(path, None, None, mode) },
            Statistics::None,
            Verbosity::Triggers,
            output,
            mode,
            output_time_representation,
            start_time,
        )
    }

    pub fn api(time_representation: TimeRepresentation) -> Self {
        EvalConfig::new(
            EventSourceConfig::Api,
            Statistics::None,
            Verbosity::Triggers,
            OutputChannel::None,
            ExecutionMode::Offline(time_representation),
            time_representation,
            None,
        )
    }
}

impl Default for EvalConfig {
    fn default() -> EvalConfig {
        let mode = ExecutionMode::Offline(TimeRepresentation::Relative(RelativeTimeFormat::FloatSecs));
        EvalConfig {
            source: EventSourceConfig::Csv { src: CsvInputSource::stdin(None, mode) },
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: OutputChannel::StdOut,
            mode,
            output_time_representation: TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339),
            start_time: None,
        }
    }
}
