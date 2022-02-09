use crate::basics::{CsvInputSource, CsvInputSourceKind, OutputChannel, OutputHandler};
use crate::config::ExecutionMode;

#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPInputSource;

use crate::config::{
    AbsoluteTimeFormat, Config, EventSourceConfig, RelativeTimeFormat, Statistics, TimeRepresentation, Verbosity,
};
use crate::coordination::Controller;
use crate::monitor::{Input, VerdictRepresentation};
use crate::Monitor;
use rtlola_frontend::mir::RtLolaMir;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/* Type state of shared config */
/// Represents a state of the [ConfigBuilder]
/// Used to ensure that only valid configurations can be created
pub trait ConfigState {}

/// The config state in which the specification has yet to be configured
#[derive(Debug, Clone, Default, Copy)]
pub struct ConfigureIR {}
impl ConfigState for ConfigureIR {}

/// The config state in which the specification is configured
#[derive(Debug, Clone)]
pub struct IrConfigured {
    ir: RtLolaMir,
}
impl ConfigState for IrConfigured {}

/// A trait to capture a sub configuration.
/// I.e. either for running the interpreter or for using the monitor API
pub trait SubConfig {}

/* Type state for execution config */
/// A trait to represent an executable configuration state
pub trait ExecConfigState {}

/// An executable configuration state in which the execution mode yet has to be configured.
#[derive(Debug, Clone, Default, Copy)]
pub struct ConfigureMode {}
impl ExecConfigState for ConfigureMode {}

/// An executable configuration state in which the execution mode is configured but the event source is not.
#[derive(Debug, Clone, Copy)]
pub struct ConfigureSource {
    mode: ExecutionMode,
}
impl ExecConfigState for ConfigureSource {}

/// An execution configuration state in which both the mode and the event source has been configured.
#[derive(Debug, Clone)]
pub struct SourceConfigured {
    mode: ExecutionMode,
    source: EventSourceConfig,
}
impl ExecConfigState for SourceConfigured {}

/* Type state for api config */
/// A sub-configuration for the API
#[derive(Debug, Clone, Default, Copy)]
pub struct ApiConfig<S: ApiConfigState> {
    state: S,
}
impl<S: ApiConfigState> SubConfig for ApiConfig<S> {}

/// A trait to capture an API configuration state
pub trait ApiConfigState {}

/// An API configuration state in which the input time format has yet to be configured
#[derive(Debug, Clone, Default, Copy)]
pub struct ConfigureTime {}
impl ApiConfigState for ConfigureTime {}

/// An API configuration state in which the input time format is configured
#[derive(Debug, Clone, Copy)]
pub struct TimeConfigured {
    input_time_repr: TimeRepresentation,
}
impl ApiConfigState for TimeConfigured {}

/// The executable monitor configuration
#[derive(Debug, Clone, Default)]
pub struct ExecConfig<S: ExecConfigState> {
    /// A statistics module
    statistics: Option<Statistics>,
    /// The verbosity to use
    verbosity: Option<Verbosity>,
    /// Where the output should go
    output_channel: Option<OutputChannel>,

    state: S,
}
impl<S: ExecConfigState> SubConfig for ExecConfig<S> {}

/// The main entry point of the application.
/// Use the various methods to construct a configuration either for running the interpreter directly or to use the [Monitor] API interface.
///
/// An example construction of the API:
/// ```
/// use rtlola_interpreter::ConfigBuilder;
/// use rtlola_interpreter::config::RelativeTimeFormat;
/// use rtlola_interpreter::monitor::{EventInput, Incremental};
/// use rtlola_interpreter::{Monitor, Value};
///
/// let monitor: Monitor<EventInput<Vec<Value>>, Incremental> =
///     ConfigBuilder::api().relative_input_time(RelativeTimeFormat::FloatSecs).spec_str("input i: Int64").monitor();
/// ````
///
/// An example configuration to run the interpreter:
/// ```
/// use rtlola_interpreter::ConfigBuilder;
/// use rtlola_interpreter::config::{RelativeTimeFormat};
/// use rtlola_interpreter::monitor::{EventInput, Incremental};
/// use rtlola_interpreter::{Monitor, Value};
/// use rtlola_interpreter::config::Verbosity;
/// use std::path::PathBuf;
///
///  ConfigBuilder::runnable()
///     .offline_relative(RelativeTimeFormat::FloatSecs)
///     .spec_str("input a: Int64")
///     .verbosity(Verbosity::Silent)
///     .csv_file_input(PathBuf::from("traces/tests/count_1_2.csv"), None, None)
///     .run();
/// ````
#[derive(Debug, Clone)]
pub struct ConfigBuilder<C: SubConfig, S: ConfigState> {
    /// Which format to use to output time
    output_time_representation: Option<TimeRepresentation>,
    /// The start time to assume
    start_time: Option<SystemTime>,
    /// The configuration for the chosen domain
    sub_config: C,
    /// The current state of the config
    state: S,
}

impl ConfigBuilder<ExecConfig<ConfigureMode>, ConfigureIR> {
    /// Creates a new executable configuration.
    pub fn runnable() -> Self {
        ConfigBuilder {
            output_time_representation: None,
            start_time: None,
            sub_config: ExecConfig::default(),
            state: ConfigureIR {},
        }
    }
}

impl ConfigBuilder<ApiConfig<ConfigureTime>, ConfigureIR> {
    /// Creates a new configuration to be used with the API.
    pub fn api() -> Self {
        ConfigBuilder {
            output_time_representation: None,
            start_time: None,
            sub_config: ApiConfig { state: ConfigureTime {} },
            state: ConfigureIR {},
        }
    }
}

impl<C: SubConfig, S: ConfigState> ConfigBuilder<C, S> {
    /// Sets the time in the output of the monitor to be absolute.
    /// See the README for further details.
    pub fn absolute_output_time(mut self, format: AbsoluteTimeFormat) -> Self {
        self.output_time_representation = Some(TimeRepresentation::Absolute(format));
        self
    }

    /// Sets the time in the output of the monitor to be relative.
    /// See the README for further details.
    pub fn relative_output_time(mut self, format: RelativeTimeFormat) -> Self {
        self.output_time_representation = Some(TimeRepresentation::Relative(format));
        self
    }

    /// Sets the start time of the execution.
    pub fn start_time(mut self, time: SystemTime) -> Self {
        self.start_time = Some(time);
        self
    }
}

impl<C: SubConfig> ConfigBuilder<C, ConfigureIR> {
    /// Read the specification from a file at the given path.
    pub fn spec_file(self, path: PathBuf) -> ConfigBuilder<C, IrConfigured> {
        let ConfigBuilder { output_time_representation, start_time, sub_config, state: _ } = self;
        let config = rtlola_frontend::ParserConfig::from_path(path).unwrap_or_else(|e| {
            eprintln!("{}", e);
            std::process::exit(1)
        });
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        ConfigBuilder { output_time_representation, start_time, sub_config, state: IrConfigured { ir } }
    }

    /// Read the specification from the given string.
    pub fn spec_str(self, spec: &str) -> ConfigBuilder<C, IrConfigured> {
        let ConfigBuilder { output_time_representation, start_time, sub_config, state: _ } = self;
        let config = rtlola_frontend::ParserConfig::for_string(spec.to_string());
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        ConfigBuilder { output_time_representation, start_time, sub_config, state: IrConfigured { ir } }
    }
}

impl<S: ConfigState> ConfigBuilder<ExecConfig<ConfigureMode>, S> {
    /// Sets the execute mode to be offline, i.e. takes the time of events from the input source.
    /// The time should be relative in the given format.
    /// See the README for further details.
    pub fn offline_relative(self, format: RelativeTimeFormat) -> ConfigBuilder<ExecConfig<ConfigureSource>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: _ },
            state: cs,
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig {
                statistics,
                verbosity,
                output_channel,
                state: ConfigureSource { mode: ExecutionMode::Offline(TimeRepresentation::Relative(format)) },
            },
            state: cs,
        }
    }

    /// Sets the execute mode to be offline, i.e. takes the time of events from the input source.
    /// The time should be absolute in the given format.
    /// See the README for further details.
    pub fn offline_absolute(self, format: AbsoluteTimeFormat) -> ConfigBuilder<ExecConfig<ConfigureSource>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: _ },
            state: cs,
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig {
                statistics,
                verbosity,
                output_channel,
                state: ConfigureSource { mode: ExecutionMode::Offline(TimeRepresentation::Absolute(format)) },
            },
            state: cs,
        }
    }

    /// Sets the execute mode to be online, i.e. the time of events is taken by the interpreter.
    pub fn online(self) -> ConfigBuilder<ExecConfig<ConfigureSource>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: _ },
            state: cs,
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig {
                statistics,
                verbosity,
                output_channel,
                state: ConfigureSource { mode: ExecutionMode::Online },
            },
            state: cs,
        }
    }
}

impl<S: ConfigState> ConfigBuilder<ExecConfig<ConfigureSource>, S> {
    /// Take the events from a given CSV file at 'path'.
    /// A delay can be specified to ignore the given timestamps in the file and take the delay as the time between the events.
    /// Optionally, the time column in the input can be specified.
    pub fn csv_file_input(
        self,
        path: PathBuf,
        delay: Option<Duration>,
        time_col: Option<usize>,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        let source = EventSourceConfig::Csv {
            src: CsvInputSource { exec_mode: mode, time_col, kind: CsvInputSourceKind::File { path, delay } },
        };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }

    /// Take the events in CSV format from stdin.
    /// Optionally, the time column in the input can be specified.
    pub fn csv_stdin_input(self, time_col: Option<usize>) -> ConfigBuilder<ExecConfig<SourceConfigured>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        let source = EventSourceConfig::Csv {
            src: CsvInputSource { exec_mode: mode, time_col, kind: CsvInputSourceKind::StdIn },
        };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }

    /// Use a PCAP file at 'path' as an input source.
    /// A delay can be specified to ignore the given timestamps in the file and take the delay as the time between the events.
    /// `local_network` sets the ip address range of your local network in CIDR format.
    #[cfg(feature = "pcap_interface")]
    pub fn pcap_input(
        self,
        path: PathBuf,
        delay: Option<Duration>,
        local_network: String,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        let source = EventSourceConfig::PCAP { src: PCAPInputSource::File { path, delay, local_network } };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }

    /// Use the network interface with the given name as input source for packets.
    /// `local_network` sets the ip address range of your local network in CIDR format.
    #[cfg(feature = "pcap_interface")]
    pub fn network_interface_input(
        self,
        name: String,
        local_network: String,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, S> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        let source = EventSourceConfig::PCAP { src: PCAPInputSource::Device { name, local_network } };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }
}

impl<ES: ExecConfigState, S: ConfigState> ConfigBuilder<ExecConfig<ES>, S> {
    /// Enable the output of statistics like processes events per second.
    pub fn enable_statistics(mut self) -> Self {
        self.sub_config.statistics = Some(Statistics::Debug);
        self
    }

    /// Set the output verbosity
    pub fn verbosity(mut self, verbosity: Verbosity) -> Self {
        self.sub_config.verbosity = Some(verbosity);
        self
    }

    /// Print the output to a file at `path`
    pub fn output_to_file(mut self, path: PathBuf) -> Self {
        self.sub_config.output_channel = Some(OutputChannel::File(path));
        self
    }

    /// Print the output to stdout
    pub fn output_to_stdout(mut self) -> Self {
        self.sub_config.output_channel = Some(OutputChannel::StdOut);
        self
    }

    /// Print the output to stderr
    pub fn output_to_stderr(mut self) -> Self {
        self.sub_config.output_channel = Some(OutputChannel::StdErr);
        self
    }
}

impl<S: ConfigState> ConfigBuilder<ApiConfig<ConfigureTime>, S> {
    /// Sets the timestamps provided to the API to be relative.
    /// See the README for more details on the input time format.
    pub fn relative_input_time(self, format: RelativeTimeFormat) -> ConfigBuilder<ApiConfig<TimeConfigured>, S> {
        let ConfigBuilder { output_time_representation, start_time, sub_config: ApiConfig { state: _ }, state: s } =
            self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: TimeConfigured { input_time_repr: TimeRepresentation::Relative(format) } },
            state: s,
        }
    }

    /// Sets the timestamps provided to the API to be absolute.
    /// See the README for more details on the input time format.
    pub fn absolute_input_time(self, format: AbsoluteTimeFormat) -> ConfigBuilder<ApiConfig<TimeConfigured>, S> {
        assert_ne!(format, AbsoluteTimeFormat::Rfc3339, "Rfc3339 time input is currently not supported by the API");
        let ConfigBuilder { output_time_representation, start_time, sub_config: ApiConfig { state: _ }, state: s } =
            self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: TimeConfigured { input_time_repr: TimeRepresentation::Absolute(format) } },
            state: s,
        }
    }
}

impl ConfigBuilder<ApiConfig<TimeConfigured>, IrConfigured> {
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> Config {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: TimeConfigured { input_time_repr } },
            state: IrConfigured { ir },
        } = self;
        Config {
            ir,
            source: EventSourceConfig::Api,
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: OutputChannel::None,
            mode: ExecutionMode::Offline(input_time_repr),
            output_time_representation: output_time_representation
                .unwrap_or(TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339)),
            start_time,
        }
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API.
    pub fn monitor<S: Input, V: VerdictRepresentation>(self) -> Monitor<S, V> {
        Monitor::setup(self.build())
    }
}

impl ConfigBuilder<ExecConfig<SourceConfigured>, IrConfigured> {
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> Config {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: IrConfigured { ir },
        } = self;
        Config {
            ir,
            source,
            statistics: statistics.unwrap_or_default(),
            verbosity: verbosity.unwrap_or_default(),
            output_channel: output_channel.unwrap_or_default(),
            mode,
            output_time_representation: output_time_representation
                .unwrap_or(TimeRepresentation::Absolute(AbsoluteTimeFormat::Rfc3339)),
            start_time,
        }
    }

    /// Run the interpreter with the constructed configuration
    pub fn run(self) -> Result<Arc<OutputHandler>, Box<dyn std::error::Error>> {
        // TODO: Rather than returning OutputHandler publicly --- let alone an Arc ---, transform into more suitable format or make OutputHandler more accessible.
        Controller::new(self.build()).start()
    }
}
