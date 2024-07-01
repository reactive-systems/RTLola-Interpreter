use std::error::Error;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::SystemTime;

use rtlola_frontend::mir::RtLolaMir;

use crate::config::{Config, ExecutionMode, MonitorConfig, OfflineMode, OnlineMode};
use crate::configuration::time::{OutputTimeRepresentation, RelativeFloat, TimeRepresentation};
use crate::input::{ArrayFactory, EventFactory, EventFactoryError, InputMap, MappedFactory};
use crate::monitor::{NoTracer, Tracer, TracingVerdict, VerdictRepresentation};
#[cfg(feature = "queued-api")]
use crate::QueuedMonitor;
use crate::{CondDeserialize, CondSerialize, Monitor, Value};

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
pub struct ModeConfigured<Mode: ExecutionMode> {
    ir: RtLolaMir,
    mode: Mode,
}
impl<Mode: ExecutionMode> ConfigState for ModeConfigured<Mode> {}

/// An API configuration state in which the input source is configured but the input time is not.
#[derive(Debug, Clone)]
pub struct InputConfigured<Mode: ExecutionMode, Source: EventFactory> {
    ir: RtLolaMir,
    mode: Mode,
    source: PhantomData<Source>,
}
impl<Source: EventFactory, Mode: ExecutionMode> ConfigState for InputConfigured<Mode, Source> {}

/// An API configuration state in which the input source is configured but the input time is not.
#[derive(Debug, Clone)]
pub struct VerdictConfigured<Mode: ExecutionMode, Source: EventFactory, Verdict: VerdictRepresentation> {
    ir: RtLolaMir,
    mode: Mode,
    source: PhantomData<Source>,
    verdict: PhantomData<Verdict>,
}
impl<Source: EventFactory, Verdict: VerdictRepresentation, Mode: ExecutionMode> ConfigState
    for VerdictConfigured<Mode, Source, Verdict>
{
}

/// The main entry point of the application.
/// Use the various methods to construct a configuration either for running the interpreter directly or to use the [Monitor] API interface.
///
/// An example construction of the API:
/// ````
/// use std::convert::Infallible;
/// use rtlola_interpreter::monitor::Incremental;
/// use rtlola_interpreter::input::ArrayFactory;
/// use rtlola_interpreter::time::RelativeFloat;
/// use rtlola_interpreter::{ConfigBuilder, Monitor, Value};
///
/// let monitor: Monitor<_, _, Incremental, _> = ConfigBuilder::new()
///     .spec_str("input i: Int64")
///     .offline::<RelativeFloat>()
///     .with_array_events::<1, Infallible, [Value; 1]>()
///     .with_verdict::<Incremental>()
///     .monitor().expect("Failed to create monitor.");
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
            output_time_representation: PhantomData,
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
            output_time_representation: PhantomData,
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
        let handler = rtlola_frontend::Handler::from(&config);
        let ir = rtlola_frontend::parse(&config).unwrap_or_else(|e| {
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
        let handler = rtlola_frontend::Handler::from(&config);
        let ir = rtlola_frontend::parse(&config).unwrap_or_else(|e| {
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
    pub fn online(self) -> ConfigBuilder<ModeConfigured<OnlineMode>, OutputTime> {
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
                mode: OnlineMode::default(),
            },
        }
    }

    /// Sets the execute mode to be offline, i.e. takes the time of events from the input source.
    /// How the input timestamps are interpreted is defined by the type parameter.
    /// See the README for further details on timestamp representations.
    /// For possible [TimeRepresentation]s see the [Time](crate::time) Module.
    pub fn offline<InputTime: TimeRepresentation>(
        self,
    ) -> ConfigBuilder<ModeConfigured<OfflineMode<InputTime>>, OutputTime> {
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
                mode: OfflineMode::default(),
            },
        }
    }
}

impl<Mode: ExecutionMode, OutputTime: OutputTimeRepresentation> ConfigBuilder<ModeConfigured<Mode>, OutputTime> {
    /// Use the predefined [ArrayFactory] method to provide inputs to the API.
    pub fn with_array_events<
        const N: usize,
        I: Error + Send + 'static,
        E: TryInto<[Value; N], Error = I> + CondSerialize + CondDeserialize + Send,
    >(
        self,
    ) -> ConfigBuilder<InputConfigured<Mode, ArrayFactory<N, I, E>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: ModeConfigured { ir, mode },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                mode,
                source: PhantomData,
            },
        }
    }

    /// Use the predefined [MappedFactory] method to provide inputs to the API.
    /// Requires implementing [InputMap] for your type defining how the values of input streams are extracted from it.
    pub fn with_mapped_events<Inner: InputMap>(
        self,
    ) -> ConfigBuilder<InputConfigured<Mode, MappedFactory<Inner>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: ModeConfigured { ir, mode },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                mode,
                source: PhantomData,
            },
        }
    }

    /// Use a custom input method to provide inputs to the API.
    pub fn with_event_factory<Source: EventFactory>(self) -> ConfigBuilder<InputConfigured<Mode, Source>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: ModeConfigured { ir, mode },
        } = self;

        ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured {
                ir,
                mode,
                source: PhantomData,
            },
        }
    }
}

impl<Mode: ExecutionMode, OutputTime: OutputTimeRepresentation, Source: EventFactory>
    ConfigBuilder<InputConfigured<Mode, Source>, OutputTime>
{
    /// Sets the [VerdictRepresentation] for the monitor
    pub fn with_verdict<Verdict: VerdictRepresentation>(
        self,
    ) -> ConfigBuilder<VerdictConfigured<Mode, Source, Verdict>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: InputConfigured { ir, mode, source },
        } = self;
        ConfigBuilder {
            output_time_representation,
            start_time,
            state: VerdictConfigured {
                ir,
                mode,
                source,
                verdict: Default::default(),
            },
        }
    }
}

impl<
        Source: EventFactory + 'static,
        Mode: ExecutionMode,
        Verdict: VerdictRepresentation<Tracing = NoTracer>,
        OutputTime: OutputTimeRepresentation,
    > ConfigBuilder<VerdictConfigured<Mode, Source, Verdict>, OutputTime>
{
    /// Adds tracing functionality to the evaluator
    pub fn with_tracer<T: Tracer>(
        self,
    ) -> ConfigBuilder<VerdictConfigured<Mode, Source, TracingVerdict<T, Verdict>>, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state:
                VerdictConfigured {
                    ir,
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
                mode,
                source,
                verdict: Default::default(),
            },
        }
    }
}

impl<
        Source: EventFactory + 'static,
        Mode: ExecutionMode,
        Verdict: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
    > ConfigBuilder<VerdictConfigured<Mode, Source, Verdict>, OutputTime>
{
    /// Finalize the configuration and generate a configuration.
    pub fn build(self) -> MonitorConfig<Source, Mode, Verdict, OutputTime> {
        let ConfigBuilder {
            output_time_representation,
            start_time,
            state: VerdictConfigured { ir, mode, .. },
        } = self;
        let config = Config {
            ir,
            mode,
            output_time_representation,
            start_time,
        };
        MonitorConfig::new(config)
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::input::EventFactory) source at creation.
    pub fn monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> Result<Monitor<Source, Mode, Verdict, OutputTime>, EventFactoryError> {
        self.build().monitor_with_data(data)
    }

    /// Create a [Monitor] from the configuration. The entrypoint of the API.
    pub fn monitor(self) -> Result<Monitor<Source, Mode, Verdict, OutputTime>, EventFactoryError>
    where
        Source: EventFactory<CreationData = ()> + 'static,
    {
        self.build().monitor()
    }
}

impl<
        Source: EventFactory + 'static,
        SourceTime: TimeRepresentation,
        Verdict: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
    > ConfigBuilder<VerdictConfigured<OfflineMode<SourceTime>, Source, Verdict>, OutputTime>
{
    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::input::EventFactory) source at creation.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, OutputTime> {
        self.build().queued_monitor_with_data(data)
    }

    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API.
    pub fn queued_monitor(self) -> QueuedMonitor<Source, OfflineMode<SourceTime>, Verdict, OutputTime>
    where
        Source: EventFactory<CreationData = ()> + 'static,
    {
        self.build().queued_monitor()
    }
}

impl<Source: EventFactory + 'static, Verdict: VerdictRepresentation, OutputTime: OutputTimeRepresentation>
    ConfigBuilder<VerdictConfigured<OnlineMode, Source, Verdict>, OutputTime>
{
    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API. The data is provided to the [Input](crate::input::EventFactory) source at creation.
    pub fn queued_monitor_with_data(
        self,
        data: Source::CreationData,
    ) -> QueuedMonitor<Source, OnlineMode, Verdict, OutputTime> {
        self.build().queued_monitor_with_data(data)
    }

    #[cfg(feature = "queued-api")]
    /// Create a [QueuedMonitor] from the configuration. The entrypoint of the API.
    pub fn queued_monitor(self) -> QueuedMonitor<Source, OnlineMode, Verdict, OutputTime>
    where
        Source: EventFactory<CreationData = ()> + 'static,
    {
        self.build().queued_monitor()
    }
}
