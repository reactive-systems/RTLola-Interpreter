#![allow(clippy::mutex_atomic)]

use std::error::Error;
use std::io::{stderr, Write};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use rtlola_interpreter::monitor::{TotalIncremental, TracingVerdict};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_io_plugins::outputs::statistics_plugin::EvalTimeTracer;
use rtlola_io_plugins::outputs::VerdictsSink;

use crate::StatsSink;

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

/// Manages the output of the interpreter.
pub struct OutputHandler<
    OutputTime: OutputTimeRepresentation,
    W: Write,
    VerdictSink: VerdictsSink<TotalIncremental, OutputTime, Error: Error + 'static, Return = ()>,
> {
    stats_sink: Option<StatsSink<W, OutputTime>>,
    verdict_sink: VerdictSink,
    output_time: PhantomData<OutputTime>,
}

impl<
        OutputTime: OutputTimeRepresentation,
        W: Write,
        VerdictSink: VerdictsSink<TotalIncremental, OutputTime, Error: Error + 'static, Return = ()>,
    > OutputHandler<OutputTime, W, VerdictSink>
{
    /// Creates a new Output Handler. If None is given as 'start_time', then the first event determines it.
    pub(crate) fn new(
        verdict_sink: VerdictSink,
        stats_sink: Option<StatsSink<W, OutputTime>>,
    ) -> OutputHandler<OutputTime, W, VerdictSink> {
        OutputHandler {
            output_time: PhantomData,
            verdict_sink,
            stats_sink,
        }
    }

    pub(crate) fn run(
        mut self,
        input: Receiver<QueuedVerdict<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>>,
    ) {
        loop {
            let res = input.recv_timeout(Duration::from_millis(250));
            let queue_verdict = match res {
                Ok(q) => q,
                Err(_) => {
                    if let Some(sink) = &mut self.stats_sink {
                        sink.factory().print_progress(&mut stderr());
                    }
                    continue;
                },
            };
            if let Some(sink) = &mut self.stats_sink {
                sink.sink_verdict(queue_verdict.ts.clone(), queue_verdict.verdict.clone())
                    .unwrap();
            }
            self.verdict_sink
                .sink_verdict(queue_verdict.ts, queue_verdict.verdict.verdict)
                .unwrap();
        }
    }
}
