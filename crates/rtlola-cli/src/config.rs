//! This module contains all configuration related structures.

use std::error::Error;
use std::marker::PhantomData;
use std::thread;
use std::time::SystemTime;

use clap::ValueEnum;
use rtlola_interpreter::config::{ExecutionMode, OfflineMode, OnlineMode};
use rtlola_interpreter::input::{AssociatedFactory, EventFactory, InputMap, MappedFactory};
use rtlola_interpreter::monitor::{TotalIncremental, TracingVerdict};
use rtlola_interpreter::rtlola_mir::RtLolaMir;
use rtlola_interpreter::time::{OutputTimeRepresentation, RealTime, TimeRepresentation};
use rtlola_interpreter::QueuedMonitor;
use rtlola_io_plugins::inputs::csv_plugin::CsvInputSourceKind;
use rtlola_io_plugins::inputs::EventSource;
use rtlola_io_plugins::outputs::csv_plugin::CsvVerbosity;
use rtlola_io_plugins::outputs::json_plugin::JsonVerbosity;
use rtlola_io_plugins::outputs::{log_printer, VerdictsSink};
#[cfg(feature = "pcap_interface")]
use rtlola_io_plugins::pcap_plugin::PcapInputSource;

use crate::output::{EvalTimeTracer, OutputHandler};

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
pub(crate) struct Config<
    Source: EventSource<InputTime>,
    Mode: ExecutionMode<SourceTime = InputTime>,
    InputTime: TimeRepresentation,
    OutputTime: OutputTimeRepresentation,
    Sink: VerdictsSink<TotalIncremental, OutputTime>,
> {
    /// The representation of the specification
    pub(crate) ir: RtLolaMir,
    /// The source of events
    pub(crate) source: Source,
    /// A statistics module
    pub(crate) statistics: Statistics,
    /// In which mode the evaluator is executed
    pub(crate) mode: Mode,
    /// Which format to use to output time
    pub(crate) output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    pub(crate) start_time: Option<SystemTime>,
    pub(crate) sink: Sink,
}

/// Used to define the level of statistics that should be computed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, ValueEnum, Default)]
pub(crate) enum Statistics {
    /// No statistics will be computed
    #[default]
    None,
    /// All statistics will be computed
    All,
}

/// The different verbosities supported by the interpreter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, ValueEnum, Default)]
pub enum Verbosity {
    /// Suppresses any kind of logging.
    Silent,
    /// Prints only triggers and runtime warnings.
    #[default]
    Trigger,
    /// Prints new stream values for outputs (and triggers).
    Outputs,
    /// Prints new stream values for every stream.
    Streams,
    /// Prints fine-grained debug information. Not suitable for production.
    Debug,
}

impl TryFrom<Verbosity> for CsvVerbosity {
    type Error = ();

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Err(()),
            Verbosity::Trigger => Ok(CsvVerbosity::Trigger),
            Verbosity::Outputs => Ok(CsvVerbosity::Outputs),
            Verbosity::Streams => Ok(CsvVerbosity::Streams),
            Verbosity::Debug => Err(()),
        }
    }
}

impl TryFrom<Verbosity> for JsonVerbosity {
    type Error = ();

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Err(()),
            Verbosity::Trigger => Ok(JsonVerbosity::Trigger),
            Verbosity::Outputs => Ok(JsonVerbosity::Outputs),
            Verbosity::Streams => Ok(JsonVerbosity::Streams),
            Verbosity::Debug => Ok(JsonVerbosity::Debug),
        }
    }
}

impl TryFrom<Verbosity> for log_printer::Verbosity {
    type Error = ();

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Err(()),
            Verbosity::Trigger => Ok(log_printer::Verbosity::Trigger),
            Verbosity::Outputs => Ok(log_printer::Verbosity::Outputs),
            Verbosity::Streams => Ok(log_printer::Verbosity::Streams),
            Verbosity::Debug => Ok(log_printer::Verbosity::Debug),
        }
    }
}

/// The different supported input sources of the interpreter.
#[derive(Debug, Clone)]
pub enum EventSourceConfig {
    /// Parse events in CSV format
    Csv {
        /// The index of column in which the time information is given
        /// If none the column named 'time' is chosen.
        time_col: Option<usize>,
        /// Specifies the input channel of the source.
        kind: CsvInputSourceKind,
    },

    /// Parse events from network packets
    #[cfg(feature = "pcap_interface")]
    Pcap(PcapInputSource),
}

impl<
        Source: EventSource<InputTime> + 'static,
        InputTime: TimeRepresentation,
        OutputTime: OutputTimeRepresentation,
        Sink: VerdictsSink<TotalIncremental, OutputTime, Return = (), Error = SinkError> + Send + 'static,
        SinkError: Error + 'static,
    > Config<Source, OfflineMode<InputTime>, InputTime, OutputTime, Sink>
where
    Source::Factory:
        InputMap<CreationData = <<Source::Factory as AssociatedFactory>::Factory as EventFactory>::CreationData>,
{
    pub(crate) fn run(self) -> Result<(), Box<dyn Error>> {
        // Convert config
        use rtlola_interpreter::config::Config as InterpreterConfig;
        let Config {
            ir,
            mut source,
            statistics,
            mode,
            output_time_representation,
            start_time,
            sink,
        } = self;

        let output: OutputHandler<_, _, _> = OutputHandler::new(statistics, sink);

        let cfg = InterpreterConfig {
            ir,
            mode,
            output_time_representation,
            start_time,
        };

        // init monitor
        let mut monitor: QueuedMonitor<
            MappedFactory<Source::Factory>,
            OfflineMode<InputTime>,
            TracingVerdict<EvalTimeTracer, TotalIncremental>,
            OutputTime,
        > = <QueuedMonitor<
            MappedFactory<Source::Factory>,
            OfflineMode<InputTime>,
            TracingVerdict<EvalTimeTracer, TotalIncremental>,
            OutputTime,
        >>::setup(cfg, source.init_data()?);

        let queue = monitor.output_queue();
        let output_handler = thread::spawn(move || output.run(queue));

        // start evaluation
        monitor.start()?;
        while let Some((ev, ts)) = source.next_event()? {
            monitor.accept_event(ev, ts)?;
        }
        // Wait for all events to be processed
        monitor.end()?;
        // Wait for the output queue to empty up.
        output_handler.join().expect("Failed to join on output handler");
        Ok(())
    }
}

impl<
        Source: EventSource<RealTime> + 'static,
        OutputTime: OutputTimeRepresentation,
        Sink: VerdictsSink<TotalIncremental, OutputTime, Return = (), Error = SinkError> + Send + 'static,
        SinkError: Error + 'static,
    > Config<Source, OnlineMode, RealTime, OutputTime, Sink>
where
    Source::Factory:
        InputMap<CreationData = <<Source::Factory as AssociatedFactory>::Factory as EventFactory>::CreationData>,
{
    pub(crate) fn run(self) -> Result<(), Box<dyn Error>> {
        // Convert config
        use rtlola_interpreter::config::Config as InterpreterConfig;
        let Config {
            ir,
            mut source,
            statistics,
            mode,
            output_time_representation,
            start_time,
            sink,
        } = self;

        let output: OutputHandler<_, _, _> = OutputHandler::new(statistics, sink);

        let cfg = InterpreterConfig {
            ir,
            mode,
            output_time_representation,
            start_time,
        };

        // init monitor
        let mut monitor: QueuedMonitor<
            MappedFactory<Source::Factory>,
            OnlineMode,
            TracingVerdict<EvalTimeTracer, TotalIncremental>,
            OutputTime,
        > = <QueuedMonitor<
            MappedFactory<Source::Factory>,
            OnlineMode,
            TracingVerdict<EvalTimeTracer, TotalIncremental>,
            OutputTime,
        >>::setup(cfg, source.init_data()?);

        let queue = monitor.output_queue();
        let output_handler = thread::spawn(move || output.run(queue));

        // start evaluation
        monitor.start()?;
        while let Some((ev, ts)) = source.next_event()? {
            monitor.accept_event(ev, ts)?;
        }
        // Wait for all events to be processed
        monitor.end()?;
        // Wait for the output queue to empty up.
        output_handler.join().expect("Failed to join on output handler");
        Ok(())
    }
}
