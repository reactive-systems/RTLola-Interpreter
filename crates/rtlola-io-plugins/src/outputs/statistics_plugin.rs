//! Module that implements [VerdictsSink](crate::outputs::VerdictsSink) to print statistics about the monitoring run
use std::convert::Infallible;
use std::io::Write;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToPreviousLine;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use rtlola_interpreter::monitor::{TotalIncremental, Tracer, TracingVerdict};
use rtlola_interpreter::output::NewVerdictFactory;
use rtlola_interpreter::rtlola_frontend::RtLolaMir;
use rtlola_interpreter::time::OutputTimeRepresentation;

use super::{VerdictFactory, VerdictsSink};

/// This tracer provides the time given as a duration the evaluation cycle took.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvalTimeTracer {
    parse_start: Option<Instant>,
    parse_end: Option<Instant>,

    eval_start: Option<Instant>,
    eval_end: Option<Instant>,
}

impl EvalTimeTracer {
    /// Returns the duration the traced evaluation cycle took.
    pub fn eval_duration(&self) -> Duration {
        self.eval_end
            .unwrap()
            .duration_since(self.eval_start.unwrap())
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

/// VerdictFactory that produces statistical output about the monitoring run
#[derive(Debug, Clone)]
pub struct StatisticsFactory {
    num_cycles: u128,
    num_events: u128,
    elapsed_eval: Duration,
    elapsed_parse: Duration,
    num_triggers: Vec<u64>,
}

/// The statistics calculated by the [StatisticsFactory]
#[derive(Clone, Debug)]
pub struct StatisticsVerdict {
    /// The average number of cycles per second
    pub cycles_per_second: Option<u128>,
    /// The average nanoseconds per cycle
    pub nanos_per_cycle: Option<u128>,
    /// The total amount of cycles so far
    pub num_cycles: u128,
    /// The total amount of accepted input events so far
    pub num_events: u128,
    /// For each trigger the total count the trigger activated so far
    pub num_triggers: Vec<u64>,
    /// The time duration spent on evaluation cycles
    pub elapsed_eval: Duration,
    /// The time duration spent on parsing input events
    pub elapsed_parse: Duration,
}

impl<OutputTime: OutputTimeRepresentation>
    VerdictFactory<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>
    for StatisticsFactory
{
    type Error = Infallible;
    type Record = StatisticsVerdict;

    fn get_verdict(
        &mut self,
        res: TracingVerdict<EvalTimeTracer, TotalIncremental>,
        _ts: OutputTime::InnerTime,
    ) -> Result<Self::Record, Self::Error> {
        let TracingVerdict { tracer, verdict } = res;
        self.new_cycle(tracer);
        for trigger in verdict.trigger {
            self.trigger(trigger);
        }
        Ok(self.verdict())
    }
}

impl<OutputTime: OutputTimeRepresentation>
    NewVerdictFactory<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>
    for StatisticsFactory
{
    type CreationData = usize;
    type CreationError = Infallible;

    fn new(_ir: &RtLolaMir, data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self::new(data))
    }
}

impl StatisticsFactory {
    /// Create a new StatisticsFactory
    pub fn new(num_trigger: usize) -> Self {
        Self {
            num_cycles: 0,
            num_events: 0,
            elapsed_eval: Duration::default(),
            elapsed_parse: Duration::default(),
            num_triggers: vec![0; num_trigger],
        }
    }

    /// Return the current statistics
    pub fn verdict(&self) -> StatisticsVerdict {
        let cycles_per_second = (self.num_cycles > 0).then(|| {
            (self.num_cycles * Duration::from_secs(1).as_nanos()) / self.elapsed_eval.as_nanos()
        });
        let nanos_per_cycle =
            (self.num_cycles > 0).then(|| self.elapsed_eval.as_nanos() / self.num_cycles);

        StatisticsVerdict {
            cycles_per_second,
            nanos_per_cycle,
            num_cycles: self.num_cycles,
            num_events: self.num_events,
            num_triggers: self.num_triggers.clone(),
            elapsed_eval: self.elapsed_eval,
            elapsed_parse: self.elapsed_parse,
        }
    }

    pub(crate) fn new_cycle(&mut self, trace: EvalTimeTracer) {
        self.elapsed_eval += trace.eval_duration();
        if let Some(parse_dur) = trace.parse_duration() {
            self.num_events += 1;
            self.elapsed_parse += parse_dur;
        }
        self.num_cycles += 1;
    }

    fn trigger(&mut self, trigger_idx: usize) {
        self.num_triggers[trigger_idx] += 1;
    }
}

#[derive(Debug)]
/// A [VerdictsSink] to print the [StatisticsVerdict] to the terminal.
/// It updates a statistics line instead of appending the output.
pub struct StatisticsVerdictSink<W: Write, O: OutputTimeRepresentation> {
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

impl<W: Write, O: OutputTimeRepresentation>
    VerdictsSink<TracingVerdict<EvalTimeTracer, TotalIncremental>, O>
    for StatisticsVerdictSink<W, O>
{
    type Error = Infallible;
    type Factory = StatisticsFactory;
    type Return = ();

    fn sink(&mut self, verdict: StatisticsVerdict) -> Result<Self::Return, Self::Error> {
        self.current_verdict = verdict;
        self.progress();
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

impl<W: Write, O: OutputTimeRepresentation> StatisticsVerdictSink<W, O> {
    /// Creates a new [StatisticsVerdictSink]
    pub fn new(num_triggers: usize, write: W) -> Self {
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

    /// Write the current statistics to the screen
    pub fn progress(&mut self) {
        if self.last_update.elapsed() >= Duration::from_millis(250) {
            self.update_cache()
        }
        writeln!(self.write, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(self.spinner_char());
        self.trigger_stats(true);
    }

    /// Prints the final statistics to the screen
    pub fn print_final(&mut self) {
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
            writeln!(
                self.write,
                "{} {} events",
                spinner_char, self.cached_verdict.num_cycles
            )
            .unwrap_or_else(|_| {});
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

    /// Clears the line on the screen
    pub fn clear_progress(&mut self) {
        execute!(
            self.write,
            MoveToPreviousLine(3u16),
            Clear(ClearType::FromCursorDown)
        )
        .unwrap_or_else(|_| {});
    }
}
