//! This module contains all configuration related structures.

use std::error::Error;
use std::io::Write;
use std::marker::PhantomData;
use std::thread;
use std::time::SystemTime;

use clap::ValueEnum;
use rtlola_interpreter::config::{ExecutionMode, OfflineMode, OnlineMode};
use rtlola_interpreter::input::{AssociatedEventFactory, EventFactory, InputMap, MappedFactory};
use rtlola_interpreter::monitor::{TotalIncremental, TracingVerdict};
use rtlola_interpreter::rtlola_mir::{RtLolaMir, StreamReference};
use rtlola_interpreter::time::{OutputTimeRepresentation, RealTime, TimeRepresentation};
use rtlola_interpreter::QueuedMonitor;
use rtlola_io_plugins::inputs::csv_plugin::CsvInputSourceKind;
#[cfg(feature = "pcap_interface")]
use rtlola_io_plugins::inputs::pcap_plugin::PcapInputSource;
use rtlola_io_plugins::inputs::EventSource;
use rtlola_io_plugins::outputs::csv_plugin::CsvVerbosity;
use rtlola_io_plugins::outputs::json_plugin::JsonVerbosity;
use rtlola_io_plugins::outputs::statistics_plugin::EvalTimeTracer;
use rtlola_io_plugins::outputs::{log_printer, VerdictsSink};

use crate::output::{OutputHandler, StatisticsVerdictSink};

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
    VerdictSink: VerdictsSink<TotalIncremental, OutputTime>,
    W: Write,
> {
    /// The representation of the specification
    pub(crate) ir: RtLolaMir,
    /// The source of events
    pub(crate) source: Source,
    /// In which mode the evaluator is executed
    pub(crate) mode: Mode,
    /// Which format to use to output time
    pub(crate) output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    pub(crate) start_time: Option<SystemTime>,
    pub(crate) verdict_sink: VerdictSink,
    pub(crate) stats_sink: Option<StatisticsVerdictSink<W, OutputTime>>,
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
    /// Only print trigger violations.
    #[clap(alias = "trigger")]
    Violations,
    /// Print trigger violations and warning trigger.
    Warnings,
    #[default]
    /// Print new stream values for public output streams.
    Public,
    /// Prints new stream values for outputs (and triggers).
    Outputs,
    /// Prints new stream values for every stream.
    Streams,
    /// Prints fine-grained debug information. Not suitable for production.
    Debug,
}

impl TryFrom<Verbosity> for CsvVerbosity {
    type Error = String;

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Err("Silent verbosity not supported with csv output format.".into()),
            Verbosity::Warnings => Ok(CsvVerbosity::Warnings),
            Verbosity::Violations => Ok(CsvVerbosity::Violations),
            Verbosity::Public => Ok(CsvVerbosity::Public),
            Verbosity::Outputs => Ok(CsvVerbosity::Outputs),
            Verbosity::Streams => Ok(CsvVerbosity::Streams),
            Verbosity::Debug => Err("Debug verbosity not supported with csv output format. Use json instead.".into()),
        }
    }
}

impl TryFrom<Verbosity> for JsonVerbosity {
    type Error = String;

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Err("Silent verbosity not supported with json output format.".into()),
            Verbosity::Warnings => Ok(JsonVerbosity::Warnings),
            Verbosity::Violations => Ok(JsonVerbosity::Violations),
            Verbosity::Public => Ok(JsonVerbosity::Public),
            Verbosity::Outputs => Ok(JsonVerbosity::Outputs),
            Verbosity::Streams => Ok(JsonVerbosity::Streams),
            Verbosity::Debug => Ok(JsonVerbosity::Debug),
        }
    }
}

impl TryFrom<Verbosity> for log_printer::Verbosity {
    type Error = String;

    fn try_from(value: Verbosity) -> Result<Self, Self::Error> {
        match value {
            Verbosity::Silent => Ok(log_printer::Verbosity::Silent),
            Verbosity::Warnings => Ok(log_printer::Verbosity::Warnings),
            Verbosity::Violations => Ok(log_printer::Verbosity::Violations),
            Verbosity::Public => Ok(log_printer::Verbosity::Public),
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
        VerdictSink: VerdictsSink<TotalIncremental, OutputTime, Return = (), Error: Error + 'static> + Send + 'static,
        W: Write + Send + 'static,
    > Config<Source, OfflineMode<InputTime>, InputTime, OutputTime, VerdictSink, W>
where
    Source::Factory:
        InputMap<CreationData = <<Source::Factory as AssociatedEventFactory>::Factory as EventFactory>::CreationData>,
{
    pub(crate) fn run(self) -> Result<(), Box<dyn Error>> {
        // Convert config
        use rtlola_interpreter::config::Config as InterpreterConfig;
        let Config {
            ir,
            mut source,
            mode,
            output_time_representation,
            start_time,
            verdict_sink,
            stats_sink,
        } = self;

        let output: OutputHandler<_, _, _> = OutputHandler::new(verdict_sink, stats_sink);

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
        VerdictSink: VerdictsSink<TotalIncremental, OutputTime, Return = (), Error: Error + 'static> + Send + 'static,
        W: Write + Send + 'static,
    > Config<Source, OnlineMode, RealTime, OutputTime, VerdictSink, W>
where
    Source::Factory:
        InputMap<CreationData = <<Source::Factory as AssociatedEventFactory>::Factory as EventFactory>::CreationData>,
{
    pub(crate) fn run(self) -> Result<(), Box<dyn Error>> {
        // Convert config
        use rtlola_interpreter::config::Config as InterpreterConfig;
        let Config {
            ir,
            mut source,
            mode,
            output_time_representation,
            start_time,
            verdict_sink,
            stats_sink,
        } = self;

        let output: OutputHandler<_, _, _> = OutputHandler::new(verdict_sink, stats_sink);

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

/// Adds the `debug` tag to all streams in `debug_streams`
pub(crate) fn annotate_debug_streams(mut mir: RtLolaMir, debug_streams: Vec<String>) -> Result<RtLolaMir, String> {
    let debug_streams = debug_streams
        .iter()
        .flat_map(|arg| arg.split(",").map(|s| s.trim()))
        .collect::<Vec<_>>();
    for stream in debug_streams {
        let Some(stream) = mir.get_stream_by_name(stream) else {
            return Err(format!(
                "The stream \"{}\" was specified to debug, but does not exist in the specification.",
                stream
            ));
        };
        let tags = match stream.as_stream_ref() {
            StreamReference::In(idx) => &mut mir.inputs[idx].tags,
            StreamReference::Out(idx) => &mut mir.outputs[idx].tags,
        };
        tags.insert("debug".into(), None);
    }
    Ok(mir)
}
