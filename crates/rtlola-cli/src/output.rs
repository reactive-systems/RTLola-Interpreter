#![allow(clippy::mutex_atomic)]

use std::convert::Infallible;
use std::error::Error;
use std::io::Write;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToPreviousLine;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use rtlola_interpreter::monitor::{TotalIncremental, TracingVerdict};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver, RecvTimeoutError};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_io_plugins::outputs::statistics_plugin::{EvalTimeTracer, StatisticsFactory, StatisticsVerdict};
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

/// Manages the output of the interpreter.
pub struct OutputHandler<
    OutputTime: OutputTimeRepresentation,
    W: Write,
    VerdictSink: VerdictsSink<TotalIncremental, OutputTime, Error: Error + 'static, Return = ()>,
> {
    stats_sink: Option<StatisticsVerdictSink<W, OutputTime>>,
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
        stats_sink: Option<StatisticsVerdictSink<W, OutputTime>>,
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
            let res = input.recv_timeout(Duration::from_millis(100));

            if let Some(sink) = &mut self.stats_sink {
                sink.clear_progress();
            }

            match res {
                Ok(queue_verdict) => {
                    self.verdict_sink
                        .sink_verdict(queue_verdict.ts.clone(), queue_verdict.verdict.verdict.clone())
                        .unwrap();
                    if let Some(sink) = &mut self.stats_sink {
                        sink.sink_verdict(queue_verdict.ts, queue_verdict.verdict).unwrap();
                    }
                },
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(sink) = &mut self.stats_sink {
                        sink.progress();
                    }
                },
                Err(RecvTimeoutError::Disconnected) => break,
            };
        }
        if let Some(sink) = &mut self.stats_sink {
            sink.print_final()
        }
    }
}

pub(crate) struct StatisticsVerdictSink<W: Write, O: OutputTimeRepresentation> {
    spinner_chars: [char; 4],
    current_char: usize,
    term_width: u16,
    last_update: Instant,
    write: W,
    factory: StatisticsFactory,
    current_verdict: StatisticsVerdict,
    cached_verdict: StatisticsVerdict,
    output_time_representation: PhantomData<O>,
}

impl<W: Write, O: OutputTimeRepresentation> VerdictsSink<TracingVerdict<EvalTimeTracer, TotalIncremental>, O>
    for StatisticsVerdictSink<W, O>
{
    type Error = Infallible;
    type Factory = StatisticsFactory;
    type Return = ();

    fn sink(&mut self, verdict: StatisticsVerdict) -> Result<Self::Return, Self::Error> {
        self.current_verdict = verdict;
        if self.last_update.elapsed() >= Duration::from_millis(250) {
            self.update_cache()
        }
        self.progress();
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

impl<W: Write, O: OutputTimeRepresentation> StatisticsVerdictSink<W, O> {
    pub(crate) fn new(num_triggers: usize, write: W) -> Self {
        let factory = StatisticsFactory::new(num_triggers);
        Self {
            spinner_chars: ['▌', '▀', '▐', '▄'],
            current_char: 0,
            term_width: crossterm::terminal::size().map(|v| v.0).unwrap_or(70),
            last_update: Instant::now(),
            write,
            current_verdict: factory.verdict(),
            cached_verdict: factory.verdict(),
            factory,
            output_time_representation: PhantomData,
        }
    }

    fn update_cache(&mut self) {
        self.cached_verdict = self.current_verdict.clone();
        self.current_char = (self.current_char + 1) % self.spinner_chars.len();
        self.last_update = Instant::now();
    }

    fn spinner_char(&self) -> char {
        self.spinner_chars[self.current_char]
    }

    fn progress(&mut self) {
        if self.last_update.elapsed() >= Duration::from_millis(250) {
            self.update_cache()
        }
        writeln!(self.write, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(self.spinner_char());
        self.trigger_stats(true);
    }

    fn print_final(&mut self) {
        self.cached_verdict = self.current_verdict.clone();
        writeln!(self.write, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(' ');
        self.event_stats();
        self.trigger_stats(false);
    }

    fn cycle_stats(&mut self, spinner_char: char) {
        if self.cached_verdict.num_cycles > 0 {
            writeln!(
                self.write,
                "{} {} cycles, {} cycles per second, {} nsec per cycles",
                spinner_char,
                self.cached_verdict.num_cycles,
                self.cached_verdict.cycles_per_second.unwrap(),
                self.cached_verdict.nanos_per_cycle.unwrap()
            )
            .unwrap_or_else(|_| {});
        } else {
            writeln!(self.write, "{} {} events", spinner_char, self.cached_verdict.num_cycles).unwrap_or_else(|_| {});
        }
    }

    fn event_stats(&mut self) {
        if self.cached_verdict.num_events > 0 {
            writeln!(
                self.write,
                "  {} input events parsed in {} secs; {} nsec per event on average",
                self.cached_verdict.num_events,
                self.cached_verdict.elapsed_parse.as_secs_f64(),
                self.cached_verdict.nanos_per_cycle.unwrap()
            )
            .unwrap_or_else(|_| {});
        }
    }

    fn trigger_stats(&mut self, short: bool) {
        let num_triggers: u64 = self.cached_verdict.num_triggers.iter().sum();
        if short {
            writeln!(self.write, "  {} trigger", num_triggers).unwrap_or_else(|_| {});
        } else {
            writeln!(self.write, "  {} trigger in total", num_triggers).unwrap_or_else(|_| {});
            writeln!(self.write, "  Trigger details:").unwrap_or_else(|_| {});
            for (idx, trigger) in self.cached_verdict.num_triggers.iter().enumerate() {
                writeln!(self.write, "   [#{}]: {}", idx, trigger).unwrap_or_else(|_| {});
            }
        }
    }

    fn clear_progress(&mut self) {
        execute!(self.write, MoveToPreviousLine(3u16), Clear(ClearType::FromCursorDown)).unwrap_or_else(|_| {});
    }
}
