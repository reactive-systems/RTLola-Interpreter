#![allow(clippy::mutex_atomic)]

use std::collections::HashMap;
use std::fs::File;
use std::io::{stderr, stdout, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::cursor::MoveUp;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use rtlola_frontend::mir::{OutputReference, RtLolaMir, TriggerReference};
use rtlola_interpreter::monitor::{Change, EvalTimeTracer, Parameters, TotalIncremental, TracingVerdict};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver, VerdictKind};
use rtlola_interpreter::time::OutputTimeRepresentation;

use crate::config::Verbosity;
#[cfg(feature = "pcap_interface")]
use crate::io::PCAPEventSource;

/// The possible targets at which the output of the interpreter can be directed.
#[derive(Debug, Clone)]
pub enum OutputChannel {
    /// Write the output to Std-Out
    StdOut,
    /// Write the output to Std-Err
    StdErr,
    /// Write the output to a File
    File(PathBuf),
}

impl Default for OutputChannel {
    fn default() -> Self {
        OutputChannel::StdOut
    }
}

/// Manages the output of the interpreter.
pub struct OutputHandler<OutputTime: OutputTimeRepresentation> {
    verbosity: Verbosity,
    channel: OutputChannel,
    file: Option<File>,
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
        channel: OutputChannel,
    ) -> OutputHandler<OutputTime> {
        let statistics = if stats == crate::config::Statistics::Debug {
            let stats = Statistics::new(ir.triggers.len());
            Some(stats)
        } else {
            None
        };

        let or_to_tr = ir
            .triggers
            .iter()
            .map(|trigger| (trigger.reference.out_ix(), trigger.trigger_reference))
            .collect();

        OutputHandler {
            verbosity,
            channel,
            file: None,
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
        input: Receiver<QueuedVerdict<TracingVerdict<EvalTimeTracer, TotalIncremental>, OutputTime>>,
    ) {
        for queue_verdict in input.iter() {
            let TracingVerdict { tracer, verdict } = queue_verdict.verdict;
            let ts = self.output_time.to_string(queue_verdict.ts);

            if let Some(stats) = self.statistics.as_mut() {
                stats.new_event(tracer.duration());
            }

            let TotalIncremental {
                inputs,
                outputs,
                trigger,
            } = verdict;
            match queue_verdict.kind {
                VerdictKind::Timed => {
                    self.debug(|| "Deadline reached", &ts);
                },
                VerdictKind::Event => {
                    self.debug(|| "Processing new event", &ts);
                    for (idx, val) in inputs {
                        let name = &self.ir.inputs[idx].name;
                        self.stream(move || format!("[Input][{}][Value] = {}", name, val), &ts);
                    }
                },
            }

            for (out, changes) in outputs {
                let name = &self.ir.outputs[out].name;
                for change in changes {
                    match change {
                        Change::Spawn(parameter) => {
                            self.debug(
                                move || {
                                    format!(
                                        "[Output][{}][Spawn] = {}",
                                        name,
                                        Self::display_parameter(Some(parameter))
                                    )
                                },
                                &ts,
                            );
                        },
                        Change::Value(parameter, val) => {
                            self.stream(
                                move || {
                                    format!(
                                        "[Output][{}{}][Value] = {}",
                                        name,
                                        Self::display_parameter(parameter),
                                        val
                                    )
                                },
                                &ts,
                            );
                        },
                        Change::Close(parameter) => {
                            self.debug(
                                move || {
                                    format!(
                                        "[Output][{}][Close] = {}",
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

            for (trg, msg) in trigger {
                let trigger_ref = self.or_to_tr[&trg];
                self.trigger(move || format!("[#{}] {}", trigger_ref, msg), trigger_ref, &ts);
            }

            if let Some(stats) = self.statistics.as_mut() {
                stats.print();
            }
        }
        self.terminate();
    }

    #[allow(dead_code)]
    pub(crate) fn trigger<F, T: Into<String>>(&mut self, msg: F, trigger_idx: usize, ts: &str)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Triggers, msg, ts);
        if let Some(statistics) = self.statistics.as_mut() {
            statistics.trigger(trigger_idx);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn debug<F, T: Into<String>>(&self, msg: F, ts: &str)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Debug, msg, ts);
    }

    #[allow(dead_code)]
    pub(crate) fn stream<F, T: Into<String>>(&self, msg: F, ts: &str)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Streams, msg, ts);
    }

    /// Accepts a message and forwards it to the appropriate output channel.
    /// If the configuration prohibits printing the message, `msg` is never called.
    fn emit<F, T: Into<String>>(&self, kind: Verbosity, msg: F, ts: &str)
    where
        F: FnOnce() -> T,
    {
        if kind <= self.verbosity {
            self.print(format!("[{}][{}]{}", ts, kind, msg().into()));
        }
    }

    fn print(&self, msg: String) {
        let _ = match self.channel {
            OutputChannel::StdOut => stdout().write((msg + "\n").as_bytes()),
            OutputChannel::StdErr => stderr().write((msg + "\n").as_bytes()),
            OutputChannel::File(_) => self.file.as_ref().unwrap().write(msg.as_bytes()),
        }; // TODO: Decide how to handle the result.
    }

    pub(crate) fn terminate(&self) {
        if let Some(statistics) = &self.statistics {
            statistics.terminate();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Statistics {
    num_events: u64,
    elapsed_total: Duration,
    num_triggers: Vec<u64>,
    last_print: Instant,

    spinner: [char; 4],
    current_char: usize,
}

impl Statistics {
    pub(crate) fn new(num_trigger: usize) -> Self {
        Statistics {
            num_events: 0,
            elapsed_total: Duration::default(),
            num_triggers: vec![0; num_trigger],
            last_print: Instant::now(),
            spinner: ['▌', '▀', '▐', '▄'],
            current_char: 0,
        }
    }

    fn next_spinner_char(&mut self) -> char {
        self.current_char = (self.current_char + 1) % self.spinner.len();
        self.spinner[self.current_char]
    }

    pub(crate) fn new_event(&mut self, time: Duration) {
        self.elapsed_total += time;
        self.num_events += 1;
    }

    fn trigger(&mut self, trigger_idx: usize) {
        self.num_triggers[trigger_idx] += 1;
    }

    pub(crate) fn print(&mut self) {
        if self.last_print.elapsed() >= Duration::from_millis(100) {
            self.last_print = Instant::now();
            Self::clear_progress_info();
            let spinner_char = self.next_spinner_char();
            self.print_progress_info(spinner_char);
        }
    }

    pub(crate) fn terminate(&self) {
        Self::clear_progress_info();
        self.print_progress_info(' ');
    }

    fn print_progress_info(&self, spin_char: char) {
        let mut out = stderr();

        // write event statistics
        if self.num_events > 0 {
            let events_per_second =
                (self.num_events as u128 * Duration::from_secs(1).as_nanos()) / self.elapsed_total.as_nanos();
            let nanos_per_event = self.elapsed_total.as_nanos() / self.num_events as u128;
            writeln!(
                out,
                "{} {} events, {} events per second, {} nsec per event",
                spin_char, self.num_events, events_per_second, nanos_per_event
            )
            .unwrap_or_else(|_| {});
        } else {
            writeln!(out, "{} {} events", spin_char, self.num_events).unwrap_or_else(|_| {});
        }

        let num_triggers = self.num_triggers.iter().fold(0, |val, num_trigger| val + num_trigger);
        writeln!(out, "  {} triggers", num_triggers).unwrap_or_else(|_| {});
    }

    fn clear_progress_info() {
        let mut stdout = stdout();
        // clear screen as much as written in `print_progress_info`
        execute!(stdout, MoveUp(1u16), Clear(ClearType::CurrentLine)).unwrap_or_else(|_| {});
        execute!(stdout, MoveUp(1u16), Clear(ClearType::CurrentLine)).unwrap_or_else(|_| {});
    }
}
