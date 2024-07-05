#![allow(clippy::mutex_atomic)]

use std::error::Error;
use std::fs::File;
use std::io::{stderr, stdout, BufWriter, Write};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rtlola_interpreter::monitor::{TotalIncremental, Tracer, TracingVerdict};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_io_plugins::outputs::VerdictsSink;

/// The possible targets at which the output of the interpreter can be directed.
#[derive(Debug, Clone, Default)]
pub enum OutputChannel {
    /// Write the output to Std-Out
    #[default]
    StdOut,
    /// Write the output to Std-Err
    StdErr,
    /// Write the output to a File
    File(PathBuf),
}

impl From<OutputChannel> for Box<dyn Write + Send> {
    fn from(channel: OutputChannel) -> Self {
        match channel {
            OutputChannel::StdOut => Box::new(stdout()),
            OutputChannel::StdErr => Box::new(stderr()),
            OutputChannel::File(f) => {
                let file = File::create(f.as_path()).expect("Could not open output file!");
                Box::new(BufWriter::new(file))
            },
        }
    }
}

/// Manages the output of the interpreter.
pub struct OutputHandler<
    OutputTime: OutputTimeRepresentation,
    Sink: VerdictsSink<TotalIncremental, OutputTime, Error = SinkError, Return = ()>,
    SinkError: Error,
> {
    // statistics: Option<Statistics>,
    sink: Sink,
    output_time: PhantomData<OutputTime>,
}

impl<
        OutputTime: OutputTimeRepresentation,
        Sink: VerdictsSink<TotalIncremental, OutputTime, Error = SinkError, Return = ()>,
        SinkError: Error + 'static,
    > OutputHandler<OutputTime, Sink, SinkError>
{
    /// Creates a new Output Handler. If None is given as 'start_time', then the first event determines it.
    pub(crate) fn new(stats: crate::config::Statistics, sink: Sink) -> OutputHandler<OutputTime, Sink, SinkError> {
        // let statistics = match stats {
        //     crate::Statistics::None => None,
        //     crate::Statistics::All => todo!(), //Some(Statistics::new(ir.triggers.len())),
        // };

        OutputHandler {
            // statistics,
            output_time: PhantomData,
            sink,
        }
    }

    pub(crate) fn run(
        mut self,
        input: Receiver<QueuedVerdict<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>>,
    ) {
        while let Ok(queue_verdict) = input.recv() {
            let TracingVerdict { tracer, verdict } = queue_verdict.verdict;

            self.sink.sink_verdict(queue_verdict.ts, verdict).unwrap();
        }
    }
}

/// This tracer provides the time given as a duration the evaluation cycle took.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EvalTimeTracer {
    parse_start: Option<Instant>,
    parse_end: Option<Instant>,

    eval_start: Option<Instant>,
    eval_end: Option<Instant>,
}

impl EvalTimeTracer {
    /// Returns the duration the traced evaluation cycle took.
    pub fn eval_duration(&self) -> Duration {
        self.eval_end.unwrap().duration_since(self.eval_start.unwrap())
    }

    /// Returns the duration the traced evaluation cycle took.
    pub fn parse_duration(&self) -> Option<Duration> {
        self.parse_end
            .and_then(|end| self.parse_start.map(|start| end.duration_since(start)))
    }
}

impl Tracer for EvalTimeTracer {
    fn parse_start(&mut self) {
        self.parse_start.replace(Instant::now());
    }

    fn parse_end(&mut self) {
        self.parse_end.replace(Instant::now());
    }

    fn eval_start(&mut self) {
        self.eval_start.replace(Instant::now());
    }

    fn eval_end(&mut self) {
        self.eval_end.replace(Instant::now());
    }
}
