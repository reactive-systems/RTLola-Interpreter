#![allow(clippy::mutex_atomic)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{stderr, stdout, BufWriter, Write};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveToPreviousLine;
use crossterm::execute;
use crossterm::style::{Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use rtlola_interpreter::monitor::{Change, Parameters, TotalIncremental, Tracer, TracingVerdict};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver, RecvTimeoutError, VerdictKind};
use rtlola_interpreter::rtlola_frontend::mir::{OutputReference, RtLolaMir, TriggerReference};
use rtlola_interpreter::rtlola_mir::OutputKind;
use rtlola_interpreter::time::OutputTimeRepresentation;

use crate::config::Verbosity;

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

impl From<OutputChannel> for Box<dyn Write> {
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
pub struct OutputHandler<OutputTime: OutputTimeRepresentation> {
    verbosity: Verbosity,
    statistics: Option<Statistics>,
    output_time: OutputTime,

    ir: RtLolaMir,
    or_to_tr: HashMap<OutputReference, TriggerReference>,
}

impl<OutputTime: OutputTimeRepresentation> OutputHandler<OutputTime> {
    /// Creates a new Output Handler. If None is given as 'start_time', then the first event determines it.
    pub(crate) fn new(
        ir: &RtLolaMir,
        verbosity: Verbosity,
        stats: crate::config::Statistics,
    ) -> OutputHandler<OutputTime> {
        let statistics = match stats {
            crate::Statistics::None => None,
            crate::Statistics::All => Some(Statistics::new(ir.triggers.len())),
        };

        let or_to_tr = ir
            .triggers
            .iter()
            .map(|trigger| (trigger.output_reference.out_ix(), trigger.trigger_reference))
            .collect();

        OutputHandler {
            verbosity,
            statistics,
            output_time: OutputTime::default(),
            ir: ir.clone(),
            or_to_tr,
        }
    }

    fn display_parameter(paras: Parameters) -> String {
        if let Some(paras) = paras {
            format!(
                "({})",
                paras.iter().map(|p| p.to_string()).collect::<Vec<String>>().join(", ")
            )
        } else {
            String::new()
        }
    }

    pub(crate) fn run(
        mut self,
        mut output: impl Write,
        input: Receiver<QueuedVerdict<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>>,
    ) {
        let mut last_stat_print = Instant::now();
        let mut first_loop = true;
        'outer: loop {
            if let Some(stats) = self.statistics.as_mut() {
                if self.verbosity != Verbosity::Silent {
                    // If we dont print anything, better clear later directly before reprinting for smoother look
                    stats.clear_progress(&mut output);
                }
            }
            loop {
                if self.statistics.is_some() && last_stat_print.elapsed() > Duration::from_millis(20) {
                    break;
                }
                let queue_verdict = match input.recv_timeout(Duration::from_millis(20)) {
                    Ok(v) => v,
                    Err(RecvTimeoutError::Disconnected) => break 'outer, // Channel closed
                    Err(RecvTimeoutError::Timeout) => break,
                };

                let TracingVerdict { tracer, verdict } = queue_verdict.verdict;
                let ts = self.output_time.to_string(queue_verdict.ts);

                if let Some(stats) = self.statistics.as_mut() {
                    stats.new_cycle(tracer);
                }

                let TotalIncremental {
                    inputs,
                    outputs,
                    trigger: _,
                } = verdict;
                match queue_verdict.kind {
                    VerdictKind::Timed => {
                        self.debug(&mut output, || "Deadline reached", &ts);
                    },
                    VerdictKind::Event => {
                        self.debug(&mut output, || "Processing new event", &ts);
                        for (idx, val) in inputs {
                            let name = &self.ir.inputs[idx].name;
                            self.debug(&mut output, move || format!("[Input][{}][Value] = {}", name, val), &ts);
                        }
                    },
                }

                for (out, changes) in outputs {
                    let kind = self.ir.outputs[out].kind;
                    let (kind_str, name) = match kind {
                        OutputKind::NamedOutput => ("Output", self.ir.outputs[out].name.clone()),
                        OutputKind::Trigger => ("Trigger", format!("#{}", self.or_to_tr[&out])),
                    };
                    let name = &name;
                    for change in changes {
                        match change {
                            Change::Spawn(parameter) => {
                                self.debug(
                                    &mut output,
                                    || {
                                        format!(
                                            "[{}][{}][Spawn] = {}",
                                            kind_str,
                                            name,
                                            Self::display_parameter(Some(parameter))
                                        )
                                    },
                                    &ts,
                                );
                            },
                            Change::Value(parameter, val) => {
                                let msg = move || {
                                    format!(
                                        "[{}][{}{}][Value] = {}",
                                        kind_str,
                                        name,
                                        Self::display_parameter(parameter),
                                        val
                                    )
                                };
                                let is_trigger = matches!(kind, OutputKind::Trigger);
                                self.stream(&mut output, msg, &ts, is_trigger);
                                if is_trigger {
                                    if let Some(statistics) = self.statistics.as_mut() {
                                        statistics.trigger(self.or_to_tr[&out]);
                                    }
                                }
                            },
                            Change::Close(parameter) => {
                                self.debug(
                                    &mut output,
                                    move || {
                                        format!(
                                            "[{}][{}][Close] = {}",
                                            kind_str,
                                            name,
                                            Self::display_parameter(Some(parameter))
                                        )
                                    },
                                    &ts,
                                );
                            },
                        }
                    }
                }
            }
            if let Some(stats) = self.statistics.as_mut() {
                if self.verbosity == Verbosity::Silent && !first_loop {
                    // If we dont print anything, better clear later directly before reprinting for smoother look
                    stats.clear_progress(&mut output);
                }
                stats.print_progress(&mut output);
                thread::sleep(Duration::from_millis(120));
                last_stat_print = Instant::now();
            }
            first_loop = false;
        }
        self.terminate(&mut output);
    }

    pub(crate) fn debug<F, T: Into<String>>(&self, out: &mut impl Write, msg: F, ts: &str)
    where
        F: FnOnce() -> T,
    {
        self.emit(out, Verbosity::Debug, msg, ts);
    }

    pub(crate) fn stream<F, T: Into<String>>(&self, out: &mut impl Write, msg: F, ts: &str, is_trigger: bool)
    where
        F: FnOnce() -> T,
    {
        if is_trigger {
            self.emit(out, Verbosity::Trigger, msg, ts);
        } else {
            self.emit(out, Verbosity::Streams, msg, ts);
        }
    }

    /// Accepts a message and forwards it to the appropriate output channel.
    /// If the configuration prohibits printing the message, `msg` is never called.
    fn emit<F, T: Into<String>>(&self, out: &mut impl Write, kind: Verbosity, msg: F, ts: &str)
    where
        F: FnOnce() -> T,
    {
        if kind <= self.verbosity {
            execute!(
                out,
                Print(format!("[{}]", ts)),
                SetForegroundColor(kind.into()),
                Print(format!("[{}]", kind)),
                ResetColor,
                Print(format!("{}\r\n", msg().into()))
            )
            .expect("Failed to write to output channel");
        }
    }

    pub(crate) fn terminate(&mut self, out: &mut impl Write) {
        if let Some(statistics) = self.statistics.as_mut() {
            statistics.print_final(out);
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

#[derive(Debug, Clone)]
pub(crate) struct Statistics {
    num_cycles: u64,
    num_events: u64,
    elapsed_eval: Duration,
    elapsed_parse: Duration,

    num_triggers: Vec<u64>,

    spinner: [char; 4],
    current_char: usize,
    term_width: u16,
}

impl Statistics {
    pub(crate) fn new(num_trigger: usize) -> Self {
        Statistics {
            num_cycles: 0,
            num_events: 0,
            elapsed_eval: Duration::default(),
            elapsed_parse: Duration::default(),
            num_triggers: vec![0; num_trigger],
            spinner: ['▌', '▀', '▐', '▄'],
            current_char: 0,
            term_width: crossterm::terminal::size().map(|(width, _)| width).unwrap_or(32),
        }
    }

    fn next_spinner_char(&mut self) -> char {
        self.current_char = (self.current_char + 1) % self.spinner.len();
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

    pub(crate) fn print_progress(&mut self, out: &mut impl Write) {
        let spinner_char = self.next_spinner_char();
        writeln!(out, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(out, spinner_char);
        self.trigger_stats(out, true);
    }

    pub(crate) fn print_final(&self, out: &mut impl Write) {
        self.clear_progress(out);
        writeln!(out, "{}", "=".repeat(self.term_width as usize)).unwrap_or_else(|_| {});
        self.cycle_stats(out, ' ');
        self.event_stats(out);
        self.trigger_stats(out, false);
    }

    fn cycle_stats(&self, out: &mut impl Write, spin_char: char) {
        // write event statistics
        if self.num_cycles > 0 {
            let cycles_per_second =
                (self.num_cycles as u128 * Duration::from_secs(1).as_nanos()) / self.elapsed_eval.as_nanos();
            let nanos_per_cycle = self.elapsed_eval.as_nanos() / self.num_cycles as u128;
            writeln!(
                out,
                "{} {} cycles, {} cycles per second, {} nsec per cycles",
                spin_char, self.num_cycles, cycles_per_second, nanos_per_cycle
            )
            .unwrap_or_else(|_| {});
        } else {
            writeln!(out, "{} {} events", spin_char, self.num_cycles).unwrap_or_else(|_| {});
        }
    }

    fn event_stats(&self, out: &mut impl Write) {
        if self.num_cycles > 0 {
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
    pub(crate) fn clear_progress(&self, out: &mut impl Write) {
        if self.num_cycles > 0 {
            execute!(out, MoveToPreviousLine(3u16), Clear(ClearType::FromCursorDown)).unwrap_or_else(|_| {});
        }
    }
}
