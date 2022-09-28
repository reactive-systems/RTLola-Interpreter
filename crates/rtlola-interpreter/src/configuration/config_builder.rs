use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::SystemTime;

use rtlola_frontend::mir::RtLolaMir;

use crate::api::monitor::{Event, EventInput, Input, Record, RecordInput, VerdictRepresentation};
use crate::config::{Config, ExecutionMode, MonitorConfig};
use crate::configuration::time::{OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::monitor::{EvalTimeTracer, NoTracer, TracingVerdict};
use crate::time::RealTime;
use crate::Monitor;
#[cfg(feature = "queued-api")]
use crate::QueuedMonitor;

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
pub struct ModeConfigured<InputTime: TimeRepresentation> {
    ir: RtLolaMir,
    input_time_representation: InputTime,
    mode: ExecutionMode,
}
impl<InputTime: TimeRepresentation> ConfigState for ModeConfigured<InputTime> {}

/// An API configuration state in which the input source is configured but the input time is not.
#[derive(Debug, Clone)]
pub struct InputConfigured<InputTime: TimeRepresentation, Source: Input> {
    ir: RtLolaMir,
    input_time_representation: InputTime,
    mode: ExecutionMode,
    source: PhantomData<Source>,
}
impl<Source: Input, InputTime: TimeRepresentation> ConfigState for InputConfigured<InputTime, Source> {}

/// An API configuration state in which the input source is configured but the input time is not.
#[derive(Debug, Clone)]
pub struct VerdictConfigured<InputTime: TimeRepresentation, Source: Input, Verdict: VerdictRepresentation> {
    ir: RtLolaMir,
    input_time_representation: InputTime,
    mode: ExecutionMode,
    source: PhantomData<Source>,
    verdict: PhantomData<Verdict>,
}
impl<Source: Input, Verdict: VerdictRepresentation, InputTime: TimeRepresentation> ConfigState
    for VerdictConfigured<InputTime, Source, Verdict>
{
}

/// The main entry point of the application.
/// Use the various methods to construct a configuration either for running the interpreter directly or to use the [Monitor] API interface.
///
/// An example construction of the API:
/// ````
/// use rtlola_interpreter::monitor::{EventInput, Incremental};
/// use rtlola_interpreter::time::RelativeFloat;
/// use rtlola_interpreter::{ConfigBuilder, Monitor, Value};
///
/// let monitor: Monitor<_, _, Incremental, _> = ConfigBuilder::new()
///     .spec_str("input i: Int64")
///     .offline::<RelativeFloat>()
///     .event_input::<Vec<Value>>()
///     .with_verdict::<Incremental>()
///     .monitor();
/// ````
#[derive(Debug, Clone)]
pub struct ConfigBuilder<S: ConfigState, OutputTime: OutputTimeRepresentation> {
    /// Which format to use to output time
    output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    start_time: Option<SystemTime>,
    /// The current state of the config
    state: S,
}

impl ConfigBuilder<ConfigureIR, RelativeFloat> {
    /// Creates a new configuration to be used with the API.
    pub fn new() -> Self {
        ConfigBuilder {
            output_time_representation: PhantomData::default(),
            start_time: None,
            state: ConfigureIR {},
        }
    }
}

impl Default for ConfigBuilder<ConfigureIR, RelativeFloat> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ConfigState, OutputTime: OutputTimeRepresentation> ConfigBuilder<S, OutputTime> {
    /// Sets the format in which time is returned.
    /// See the README for more details on time formats.
    /// For possible formats see the [OutputTimeRepresentation](crate::time::OutputTimeRepresentation) trait.
    pub fn output_time<T: OutputTimeRepresentation>(self) -> ConfigBuilder<S, T> {
        let ConfigBuilder {
            output_time_representation: _,
            start_time,
            state,
        } = self;
        ConfigBuilder {
            output_time_representation: PhantomData::default(),
            start_time,
            state,
        }
    }

    /// Sets the start time of the execution.
    pub fn start_time(mut self, time: SystemTime) -> Self {
        self.start_time = Some(time);
        self
    }
}

impl<OutputTime: OutputTimeRepresentation> ConfigBuilder<ConfigureIR, OutputTime> {
    /// Use an existing ir with the configuration
    pub fn with_ir(self, ir: RtLolaMir) -> ConfigBuilder<IrConfigured, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: _,
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: IrConfigured { ir },
        }
    }

    /// Read the specification from a file at the given path.
    pub fn spec_file(self, path: PathBuf) -> ConfigBuilder<IrConfigured, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: _,
        } = self;
        let config = rtlola_frontend::ParserConfig::from_path(path).unwrap_or_else(|e| {
            eprintln!("{}", e);
            std::process::exit(1)
        });
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: IrConfigured { ir },
        }
    }

    /// Read the specification from the given string.
    pub fn spec_str(self, spec: &str) -> ConfigBuilder<IrConfigured, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: _,
        } = self;
        let config = rtlola_frontend::ParserConfig::for_string(spec.to_string());
        let handler = rtlola_frontend::Handler::from(config.clone());
        let ir = rtlola_frontend::parse(config).unwrap_or_else(|e| {
            handler.emit_error(&e);
            std::process::exit(1);
        });
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: IrConfigured { ir },
        }
    }
}

impl<OutputTime: OutputTimeRepresentation> ConfigBuilder<IrConfigured, OutputTime> {
    /// Sets the execute mode to be online, i.e. the time of events is taken by the interpreter.
    pub fn online(self) -> ConfigBuilder<ModeConfigured<RealTime>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: IrConfigured { ir },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: ModeConfigured {
                ir,
                input_time_representation: RealTime::default(),
                mode: ExecutionMode::Online,
            },
        }
    }

    /// Sets the execute mode to be offline, i.e. takes the time of events from the input source.
    /// How the input timestamps are interpreted is defined by the type parameter.
    /// See the README for further details on timestamp representations.
    /// For possible [TimeRepresentation]s see the [Time](crate::time) Module.
    pub fn offline<InputTime: TimeRepresentation>(self) -> ConfigBuilder<ModeConfigured<InputTime>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: IrConfigured { ir },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: ModeConfigured {
                ir,
                input_time_representation: InputTime::default(),
                mode: ExecutionMode::Offline,
            },
        }
    }
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation>
    ConfigBuilder<ModeConfigured<InputTime>, OutputTime>
{
    /// Use the predefined [EventInput] method to provide inputs to the API.
    pub fn event_input<E: Into<Event> + Send>(
        self,
    ) -> ConfigBuilder<InputConfigured<InputTime, EventInput<E>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                ModeConfigured {
                    ir,
                    input_time_representation,
                    mode,
                },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                input_time_representation,
                mode,
                source: PhantomData::default(),
            },
        }
    }

    /// Use the predefined [RecordInput] method to provide inputs to the API.
    pub fn record_input<Inner: Record>(
        self,
    ) -> ConfigBuilder<InputConfigured<InputTime, RecordInput<Inner>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                ModeConfigured {
                    ir,
                    input_time_representation,
                    mode,
                },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                input_time_representation,
                mode,
                source: PhantomData::default(),
            },
        }
    }

    /// Use a custom input method to provide inputs to the API.
    pub fn custom_input<Source: Input>(self) -> ConfigBuilder<InputConfigured<InputTime, Source>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                ModeConfigured {
                    ir,
                    input_time_representation,
                    mode,
                },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                input_time_representation,
                mode,
                source: PhantomData::default(),
            },
        }
    }
}

impl<InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation, Source: Input>
    ConfigBuilder<InputConfigured<InputTime, Source>, OutputTime>
{
    /// Sets the [VerdictRepresentation] for the monitor
    pub fn with_verdict<Verdict: VerdictRepresentation>(
        self,
    ) -> ConfigBuilder<VerdictConfigured<InputTime, Source, Verdict>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                InputConfigured {
                    ir,
                    input_time_representation,
                    mode,
                    source,
                },
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: VerdictConfigured {
                ir,
                input_time_representation,
                mode,
                source,
                verdict: Default::default(),
            },
        }
    }
}

impl<
        Source: Input + 'static,
        InputTime: TimeRepresentation,
        Verdict: VerdictRepresentation<Tracing = NoTracer>,
        OutputTime: OutputTimeRepresentation,
    > ConfigBuilder<VerdictConfigured<InputTime, Source, Verdict>, OutputTime>
{
    /// Additionally collects timing information of the evaluator
    pub fn with_eval_time(
        self,
    ) -> ConfigBuilder<VerdictConfigured<InputTime, Source, TracingVerdict<EvalTimeTracer, Verdict>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                VerdictConfigured {
                    ir,
                    input_time_representation,
                    mode,
                    source,
                    verdict: _,
                },
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: VerdictConfigured {
                ir,
                input_time_representation,
                mode,
                source,
                verdict: Default::default(),
            },
        }
    }
}

impl<
        Source: Input + 'static,
        InputTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
    > ConfigBuilder<VerdictConfigured<InputTime, Source, Verdict>, OutputTime>
{
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> MonitorConfig<Source, InputTime, Verdict, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                VerdictConfigured {
                    ir,
                    input_time_representation,
                    mode,
                    ..
                },
        } = self;
        let config = Config {
            ir,
            mode,
            input_time_representation,
            output_time_representation,
            start_time,
        };
        MonitorConfig::new(config)
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::monitor::Input) source at creation.
    pub fn monitor_with_data(self, data: Source::CreationData) -> Monitor<Source, InputTime, Verdict, OutputTime> {
        self.build().monitor_with_data(data)
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API.
    pub fn monitor(self) -> Monitor<Source, InputTime, Verdict, OutputTime>
    where
        Source: Input<CreationData = ()> + 'static,
    {
        self.build().monitor()
    }

    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::monitor::Input) source at creation.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, InputTime, Verdict, OutputTime> {
        self.build().queued_monitor_with_data(data)
    }

    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API.
    pub fn queued_monitor(self) -> QueuedMonitor<Source, InputTime, Verdict, OutputTime>
    where
        Source: Input<CreationData = ()> + 'static,
    {
        self.build().queued_monitor()
    }
}
