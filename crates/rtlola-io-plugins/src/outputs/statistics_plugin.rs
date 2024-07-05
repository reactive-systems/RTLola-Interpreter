//! Module that implements [VerdictsSink] to print statistics about the monitoring run
use std::convert::Infallible;
use std::io::Write;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToPreviousLine;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use rtlola_interpreter::monitor::{TotalIncremental, Tracer, TracingVerdict};
use rtlola_interpreter::time::OutputTimeRepresentation;

use super::{StringSink, VerdictFactory};

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

/// VerdictFactory that produces statistical output about the monitoring run
#[derive(Debug, Clone)]
pub struct StatisticsFactory {
    num_cycles: u64,
    num_events: u64,
    elapsed_eval: Duration,
    elapsed_parse: Duration,

    num_triggers: Vec<u64>,

    spinner: [char; 4],
    current_char: usize,
    term_width: u16,
    last_update: Instant,

    // cache last stats
    cached_cycles_per_second: u128,
    cached_nanos_per_cycle: u128,
    cached_num_cycles: u64,
}

impl<OutputTime: OutputTimeRepresentation> VerdictFactory<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>
    for StatisticsFactory
{
    type Error = Infallible;
    type Verdict = String;

    fn get_verdict(
        &mut self,
        res: TracingVerdict<EvalTimeTracer, TotalIncremental>,
        _ts: OutputTime::InnerTime,
    ) -> Result<Self::Verdict, Self::Error> {
        let TracingVerdict { tracer, verdict } = res;
        self.new_cycle(tracer);
        for trigger in verdict.trigger {
            self.trigger(trigger);
        }

        let mut output = Vec::new();
        self.print_progress(&mut output);
        Ok(String::from_utf8(output).unwrap())
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
            spinner: ['▌', '▀', '▐', '▄'],
            current_char: 0,
            term_width: crossterm::terminal::size().map(|v| v.0).unwrap_or(70),
            last_update: Instant::now(),

            cached_cycles_per_second: 0,
            cached_nanos_per_cycle: 0,
            cached_num_cycles: 0,
        }
    }

    /// Turn the StatisticsFactory into a sink writing into the provided writer
    pub fn sink<W: Write, OutputTime: OutputTimeRepresentation>(
        self,
        writer: W,
    ) -> StringSink<W, Self, TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime> {
        StringSink::new(writer, self)
    }

    fn next_spinner_char(&mut self) -> char {
        self.current_char = (self.current_char + 1) % self.spinner.len();
        self.spinner[self.current_char]
    }

    fn spinner_char(&self) -> char {
        self.spinner[self.current_char]
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

    /// Return the current progress text as a string
    pub fn progress(&mut self) -> String {
        let mut res = Vec::new();
        self.print_progress(&mut res);
        String::from_utf8(res).unwrap()
    }

    fn update_cache(&mut self) {
        self.next_spinner_char();
        if self.num_cycles > 0 {
            self.cached_cycles_per_second =
                (self.num_cycles as u128 * Duration::from_secs(1).as_nanos()) / self.elapsed_eval.as_nanos();
            self.cached_nanos_per_cycle = self.elapsed_eval.as_nanos() / self.num_cycles as u128;
            self.cached_num_cycles = self.num_cycles;
            self.last_update = Instant::now();
        }
    }

    fn print_progress(&mut self, out: &mut impl Write) {
        if self.last_update.elapsed() >= Duration::from_millis(250) {
            self.update_cache();
        }
        writeln!(out, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(out, self.spinner_char());
        self.trigger_stats(out, true);
    }

    /// print additional statistics at the end of the monitoring
    pub fn final_progress(&mut self) -> String {
        let mut res = Vec::new();
        self.print_final(&mut res);
        String::from_utf8(res).unwrap()
    }

    fn print_final(&mut self, out: &mut impl Write) {
        self.update_cache();
        writeln!(out, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(out, ' ');
        self.event_stats(out);
        self.trigger_stats(out, false);
    }

    fn cycle_stats(&self, out: &mut impl Write, spin_char: char) {
        // write event statistics
        if self.num_cycles > 0 {
            writeln!(
                out,
                "{} {} cycles, {} cycles per second, {} nsec per cycles",
                spin_char, self.cached_num_cycles, self.cached_cycles_per_second, self.cached_nanos_per_cycle
            )
            .unwrap_or_else(|_| {});
        } else {
            writeln!(out, "{} {} events", spin_char, self.num_cycles).unwrap_or_else(|_| {});
        }
    }

    fn event_stats(&self, out: &mut impl Write) {
        if self.num_events > 0 {
            let seconds_per_cycle = self.elapsed_parse.as_nanos() / self.num_events as u128;
            writeln!(
                out,
                "  {} input events parsed in {} secs; {} nsec per event on average",
                self.num_events,
                self.elapsed_parse.as_secs_f32(),
                seconds_per_cycle
            )
            .unwrap_or_else(|_| {});
        }
    }

    fn trigger_stats(&self, out: &mut impl Write, short: bool) {
        let num_triggers: u64 = self.num_triggers.iter().sum();
        if short {
            writeln!(out, "  {} trigger", num_triggers).unwrap_or_else(|_| {});
        } else {
            writeln!(out, "  {} trigger in total", num_triggers).unwrap_or_else(|_| {});
            writeln!(out, "  Trigger details:").unwrap_or_else(|_| {});
            for (idx, trigger) in self.num_triggers.iter().enumerate() {
                writeln!(out, "   [#{}]: {}", idx, trigger).unwrap_or_else(|_| {});
            }
        }
    }

    /// clear screen as much as written in `progress`
    pub fn clear_progress_string(&self) -> String {
        let mut res = Vec::new();
        self.clear_progress(&mut res);
        String::from_utf8(res).unwrap()
    }

    fn clear_progress(&self, out: &mut impl Write) {
        execute!(out, MoveToPreviousLine(3u16), Clear(ClearType::FromCursorDown)).unwrap_or_else(|_| {});
    }
}
