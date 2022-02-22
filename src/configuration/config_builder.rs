use crate::basics::{CsvInputSource, CsvInputSourceKind, OutputChannel, OutputHandler};
use crate::config::ExecutionMode;

#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPInputSource;

use crate::config::{Config, EventSourceConfig, Statistics, Verbosity};
use crate::configuration::time::{AbsoluteFloat, DelayTime, RealTime, RelativeFloat, TimeRepresentation};
use crate::coordination::Controller;
use crate::monitor::{Event, EventInput, Input, Record, RecordInput, VerdictRepresentation};
use crate::Monitor;
use rtlola_frontend::mir::RtLolaMir;
use std::marker::PhantomData;
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

/// The config state in which the specification is configured
#[derive(Debug, Clone)]
pub struct TimeConfigured<IT: TimeRepresentation> {
    ir: RtLolaMir,
    input_time_representation: IT,
}
impl<IT: TimeRepresentation> ConfigState for TimeConfigured<IT> {}

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
    state: PhantomData<S>,
}
impl<S: ApiConfigState> SubConfig for ApiConfig<S> {}

/// A trait to capture an API configuration state
pub trait ApiConfigState {}

/// An API configuration state in which the input source still has to be configured.
#[derive(Debug, Clone, Copy)]
pub struct ConfigureInput {}
impl ApiConfigState for ConfigureInput {}

/// An API configuration state in which the input source is configured but the input time is not.
#[derive(Debug, Clone, Default, Copy)]
pub struct InputConfigured<I: Input> {
    source: PhantomData<I>,
}
impl<I: Input> ApiConfigState for InputConfigured<I> {}

// /// An API configuration state in which the event input and the input time is configured but not its format
// #[derive(Debug, Clone, Copy, Default)]
// pub struct TimeConfigured<I: Input, IT: TimeRepresentation> {
//     source: PhantomData<I>,
//     input_time: PhantomData<IT>,
// }
// impl<I: Input, IT: TimeRepresentation> ApiConfigState for TimeConfigured<I, IT> {}

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
///     ConfigBuilder::api().relative_input_time(RelativeTimeFormat::FloatSecs).spec_str("input i: Int64").monitor(());
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
pub struct ConfigBuilder<C: SubConfig, S: ConfigState, OT: TimeRepresentation> {
    /// Which format to use to output time
    output_time_representation: PhantomData<OT>,
    /// The start time to assume
    start_time: Option<SystemTime>,
    /// The configuration for the chosen domain
    sub_config: C,
    /// The current state of the config
    state: S,
}

impl ConfigBuilder<ExecConfig<ConfigureMode>, ConfigureIR, RelativeFloat> {
    /// Creates a new executable configuration.
    pub fn runnable() -> Self {
        ConfigBuilder {
            output_time_representation: PhantomData::default(),
            start_time: None,
            sub_config: ExecConfig::default(),
            state: ConfigureIR {},
        }
    }
}

impl ConfigBuilder<ApiConfig<ConfigureInput>, ConfigureIR, RelativeFloat> {
    /// Creates a new configuration to be used with the API.
    pub fn api() -> Self {
        ConfigBuilder {
            output_time_representation: PhantomData::default(),
            start_time: None,
            sub_config: ApiConfig { state: PhantomData::default() },
            state: ConfigureIR {},
        }
    }
}

impl<C: SubConfig, S: ConfigState, OT: TimeRepresentation> ConfigBuilder<C, S, OT> {
    /// Sets the format in which time is returned.
    /// See the README for more details on time formats.
    /// For possible formats see the [Time](crate::config::time) module.
    pub fn output_time<T: TimeRepresentation>(self) -> ConfigBuilder<C, S, T> {
        let ConfigBuilder { output_time_representation: _, start_time, sub_config, state } = self;
        ConfigBuilder { output_time_representation: PhantomData::default(), start_time, sub_config, state }
    }

    /// Sets the start time of the execution.
    pub fn start_time(mut self, time: SystemTime) -> Self {
        self.start_time = Some(time);
        self
    }
}

impl<C: SubConfig, OT: TimeRepresentation> ConfigBuilder<C, ConfigureIR, OT> {
    /// Use an existing ir with the configuration
    pub fn with_ir(self, ir: RtLolaMir) -> ConfigBuilder<C, IrConfigured, OT> {
        let ConfigBuilder { output_time_representation, start_time, sub_config, state: _ } = self;
        ConfigBuilder { output_time_representation, start_time, sub_config, state: IrConfigured { ir } }
    }

    /// Read the specification from a file at the given path.
    pub fn spec_file(self, path: PathBuf) -> ConfigBuilder<C, IrConfigured, OT> {
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
    pub fn spec_str(self, spec: &str) -> ConfigBuilder<C, IrConfigured, OT> {
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

impl<C: SubConfig, OT: TimeRepresentation> ConfigBuilder<C, IrConfigured, OT> {
    /// Use an existing ir with the configuration
    pub fn input_time<IT: TimeRepresentation>(self) -> ConfigBuilder<C, TimeConfigured<IT>, OT> {
        let ConfigBuilder { output_time_representation, start_time, sub_config, state: IrConfigured { ir } } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config,
            state: TimeConfigured { ir, input_time_representation: IT::default() },
        }
    }
}

impl<IT: TimeRepresentation, OT: TimeRepresentation> ConfigBuilder<ExecConfig<ConfigureMode>, TimeConfigured<IT>, OT> {
    /// Sets the execute mode to be offline, i.e. takes the time of events from the input source.
    /// The time representation is given as the type parameter.
    /// See the README for further details on time representations.
    /// For possible [TimeRepresentation]s see the [Time](crate::configure::time) Module.
    pub fn offline(self) -> ConfigBuilder<ExecConfig<ConfigureSource>, TimeConfigured<IT>, OT> {
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
                state: ConfigureSource { mode: ExecutionMode::Offline },
            },
            state: cs,
        }
    }

    /// Sets the execute mode to be online, i.e. the time of events is taken by the interpreter.
    pub fn online(self) -> ConfigBuilder<ExecConfig<ConfigureSource>, TimeConfigured<RealTime>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: _ },
            state: TimeConfigured { ir, .. },
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
            state: TimeConfigured { ir, input_time_representation: RealTime::default() },
        }
    }
}

impl<IT: TimeRepresentation, OT: TimeRepresentation>
    ConfigBuilder<ExecConfig<ConfigureSource>, TimeConfigured<IT>, OT>
{
    /// Take the events from a given CSV file at 'path'.
    /// Optionally, the time column in the input can be specified.
    pub fn csv_file_input(
        self,
        path: PathBuf,
        time_col: Option<usize>,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<IT>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        assert_ne!(mode, ExecutionMode::Online, "CSV File input is only supported in offline mode");
        let source = EventSourceConfig::Csv { src: CsvInputSource { time_col, kind: CsvInputSourceKind::File(path) } };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }

    /// Take the events in CSV format from stdin.
    /// Optionally, the time column in the input can be specified.
    pub fn csv_stdin_input(
        self,
        time_col: Option<usize>,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<IT>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        let source = EventSourceConfig::Csv { src: CsvInputSource { time_col, kind: CsvInputSourceKind::StdIn } };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }

    /// Use a PCAP file at 'path' as an input source.
    /// `local_network` sets the ip address range of your local network in CIDR format.
    #[cfg(feature = "pcap_interface")]
    pub fn pcap_input(
        self,
        path: PathBuf,
        local_network: String,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<AbsoluteFloat>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: TimeConfigured { ir, .. },
        } = self;
        let source = EventSourceConfig::PCAP { src: PCAPInputSource::File { path, local_network } };
        assert_ne!(mode, ExecutionMode::Online, "PCAP File input is only supported in offline mode");
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: TimeConfigured { ir, input_time_representation: AbsoluteFloat::default() },
        }
    }

    /// Use the network interface with the given name as input source for packets.
    /// `local_network` sets the ip address range of your local network in CIDR format.
    #[cfg(feature = "pcap_interface")]
    pub fn network_interface_input(
        self,
        name: String,
        local_network: String,
    ) -> ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<IT>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: ConfigureSource { mode } },
            state: cs,
        } = self;
        assert_ne!(mode, ExecutionMode::Offline, "Network interface input is only supported in online mode");
        let source = EventSourceConfig::PCAP { src: PCAPInputSource::Device { name, local_network } };
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: cs,
        }
    }
}

impl<IT: TimeRepresentation, OT: TimeRepresentation>
    ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<IT>, OT>
{
    /// A delay is set to ignore the given timestamps in the input and take the delay as the time between the events.
    pub fn delay(self, delay: Duration) -> ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<DelayTime>, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { source, mode } },
            state: TimeConfigured { ir, .. },
        } = self;
        match source {
            EventSourceConfig::Api
            | EventSourceConfig::Csv { src: CsvInputSource { kind: CsvInputSourceKind::StdIn { .. }, .. } } => {
                panic!("A delay is only supported for file based input.")
            }
            #[cfg(feature = "pcap_interface")]
            EventSourceConfig::PCAP { src: PCAPInputSource::Device { .. } } => {
                panic!("A delay is only supported for file based input.")
            }
            _ => {}
        }
        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: TimeConfigured { ir, input_time_representation: DelayTime::new(delay) },
        }
    }
}

impl<ES: ExecConfigState, S: ConfigState, OT: TimeRepresentation> ConfigBuilder<ExecConfig<ES>, S, OT> {
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

impl<S: ConfigState, OT: TimeRepresentation> ConfigBuilder<ApiConfig<ConfigureInput>, S, OT> {
    /// Use the predefined [EventInput] method to provide inputs to the API.
    pub fn event_input<E: Into<Event>>(self) -> ConfigBuilder<ApiConfig<InputConfigured<EventInput<E>>>, S, OT> {
        let ConfigBuilder { output_time_representation, start_time, sub_config: ApiConfig { state: _ }, state: s } =
            self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: PhantomData::default() },
            state: s,
        }
    }

    /// Use the predefined [RecordInput] method to provide inputs to the API.
    pub fn record_input<R: Record>(self) -> ConfigBuilder<ApiConfig<InputConfigured<RecordInput<R>>>, S, OT> {
        let ConfigBuilder { output_time_representation, start_time, sub_config: ApiConfig { state: _ }, state: s } =
            self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: PhantomData::default() },
            state: s,
        }
    }

    /// Use a custom input method to provide inputs to the API.
    pub fn custom_input<I: Input>(self) -> ConfigBuilder<ApiConfig<InputConfigured<I>>, S, OT> {
        let ConfigBuilder { output_time_representation, start_time, sub_config: ApiConfig { state: _ }, state: s } =
            self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: PhantomData::default() },
            state: s,
        }
    }
}

impl<I: Input, IT: TimeRepresentation, OT: TimeRepresentation>
    ConfigBuilder<ApiConfig<InputConfigured<I>>, TimeConfigured<IT>, OT>
{
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> Config<IT, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ApiConfig { state: _ },
            state: TimeConfigured { ir, input_time_representation },
        } = self;
        Config {
            ir,
            source: EventSourceConfig::Api,
            statistics: Statistics::None,
            verbosity: Verbosity::Triggers,
            output_channel: OutputChannel::None,
            mode: ExecutionMode::Offline,
            input_time_representation,
            output_time_representation,
            start_time,
        }
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::monitor::Input) source at creation.
    pub fn monitor_with_data<V: VerdictRepresentation>(self, data: I::CreationData) -> Monitor<I, IT, V, OT> {
        Monitor::setup(self.build(), data)
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API.
    pub fn monitor<V: VerdictRepresentation>(self) -> Monitor<I, IT, V, OT>
    where
        I: Input<CreationData = ()>,
    {
        Monitor::setup(self.build(), ())
    }
}

impl<IT: TimeRepresentation, OT: TimeRepresentation>
    ConfigBuilder<ExecConfig<SourceConfigured>, TimeConfigured<IT>, OT>
{
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> Config<IT, OT> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            sub_config: ExecConfig { statistics, verbosity, output_channel, state: SourceConfigured { mode, source } },
            state: TimeConfigured { ir, input_time_representation },
        } = self;
        Config {
            ir,
            source,
            statistics: statistics.unwrap_or_default(),
            verbosity: verbosity.unwrap_or_default(),
            output_channel: output_channel.unwrap_or_default(),
            mode,
            input_time_representation,
            output_time_representation,
            start_time,
        }
    }

    /// Run the interpreter with the constructed configuration
    pub fn run(self) -> Result<Arc<OutputHandler<OT>>, Box<dyn std::error::Error>> {
        // TODO: Rather than returning OutputHandler publicly --- let alone an Arc ---, transform into more suitable format or make OutputHandler more accessible.
        Controller::new(self.build()).start()
    }
}
