//! This module contains all configuration related structures.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{stderr, stdout, BufWriter};
use std::marker::PhantomData;
use std::thread;
use std::time::SystemTime;

use clap::ValueEnum;
use crossterm::style::Color;
use rtlola_frontend::RtLolaMir;
use rtlola_input_plugins::csv_plugin::CsvInputSourceKind;
#[cfg(feature = "pcap_interface")]
use rtlola_input_plugins::pcap_plugin::PcapInputSource;
use rtlola_input_plugins::EventSource;
use rtlola_interpreter::config::ExecutionMode;
use rtlola_interpreter::monitor::{RecordInput, TotalIncremental, TracingVerdict};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeRepresentation};
use rtlola_interpreter::QueuedMonitor;

use crate::output::{EvalTimeTracer, OutputChannel, OutputHandler};

/**
`Config` combines an RTLola specification in [RtLolaMir] form with various configuration parameters for the interpreter.

The configuration describes how the specification should be executed.
The `Config` can then be turned into a monitor for use via the API or simply executed.
 */
pub(crate) struct Config<
    Source: EventSource<InputTime>,
    InputTime: TimeRepresentation,
    OutputTime: OutputTimeRepresentation,
> {
    /// The representation of the specification
    pub(crate) ir: RtLolaMir,
    /// The source of events
    pub(crate) source: Source,
    /// A statistics module
    pub(crate) statistics: Statistics,
    /// The verbosity to use
    pub(crate) verbosity: Verbosity,
    /// Where the output should go
    pub(crate) output_channel: OutputChannel,
    /// In which mode the evaluator is executed
    pub(crate) mode: ExecutionMode,
    /// Which format the time is given to the monitor
    pub(crate) input_time_representation: InputTime,
    /// Which format to use to output time
    pub(crate) output_time_representation: PhantomData<OutputTime>,
    /// The start time to assume
    pub(crate) start_time: Option<SystemTime>,
}

/// Used to define the level of statistics that should be computed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, ValueEnum)]
pub(crate) enum Statistics {
    /// No statistics will be computed
    None,
    /// All statistics will be computed
    All,
}

impl Default for Statistics {
    fn default() -> Self {
        Statistics::None
    }
}

/// The different verbosities supported by the interpreter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, ValueEnum)]
pub enum Verbosity {
    /// Suppresses any kind of logging.
    Silent,
    /// Prints only triggers and runtime warnings.
    Trigger,
    /// Prints new stream values for every stream.
    Streams,
    /// Prints fine-grained debug information. Not suitable for production.
    Debug,
}

impl Display for Verbosity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Verbosity::Silent => write!(f, "Silent"),
            Verbosity::Trigger => write!(f, "Trigger"),
            Verbosity::Streams => write!(f, "Stream"),
            Verbosity::Debug => write!(f, "Debug"),
        }
    }
}

impl Default for Verbosity {
    fn default() -> Self {
        Verbosity::Trigger
    }
}

impl From<Verbosity> for Color {
    fn from(v: Verbosity) -> Self {
        match v {
            Verbosity::Silent => Color::White,
            Verbosity::Trigger => Color::DarkRed,
            Verbosity::Streams => Color::DarkGrey,
            Verbosity::Debug => Color::Grey,
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

impl<Source: EventSource<InputTime> + 'static, InputTime: TimeRepresentation, OutputTime: OutputTimeRepresentation>
    Config<Source, InputTime, OutputTime>
{
    pub(crate) fn run(self) -> Result<(), Box<dyn Error>> {
        // Convert config
        use rtlola_interpreter::config::Config as InterpreterConfig;
        let Config {
            ir,
            mut source,
            statistics,
            verbosity,
            output_channel,
            mode,
            input_time_representation,
            output_time_representation,
            start_time,
        } = self;

        let output: OutputHandler<OutputTime> = OutputHandler::new(&ir, verbosity, statistics);

        let cfg = InterpreterConfig {
            ir,
            mode,
            input_time_representation,
            output_time_representation,
            start_time,
        };

        // init monitor
        let mut monitor: QueuedMonitor<
            RecordInput<Source::Rec>,
            InputTime,
            TracingVerdict<EvalTimeTracer, TotalIncremental>,
            OutputTime,
        > = QueuedMonitor::setup(cfg, source.init_data());

        let queue = monitor.output_queue();
        let output_handler = match output_channel {
            OutputChannel::StdOut => thread::spawn(move || output.run(stdout(), queue)),
            OutputChannel::StdErr => thread::spawn(move || output.run(stderr(), queue)),
            OutputChannel::File(f) => {
                let file = File::create(f.as_path()).expect("Could not open output file!");
                thread::spawn(move || output.run(BufWriter::new(file), queue))
            },
        };

        // start evaluation
        monitor.start();
        while let Some((ev, ts)) = source.next_event() {
            monitor.accept_event(ev, ts);
        }
        // Wait for all events to be processed
        monitor.end();
        // Wait for the output queue to empty up.
        output_handler.join().expect("Failed to join on output handler");
        Ok(())
    }
}