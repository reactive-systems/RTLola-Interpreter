#![allow(clippy::mutex_atomic)]

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{stderr, stdout, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use crossterm::cursor::MoveUp;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use rtlola_frontend::mir::{OutputReference, RtLolaMir, TriggerReference};
use rtlola_interpreter::monitor::{Change, Incremental, Input, Parameters, Record, TotalIncremental};
use rtlola_interpreter::queued::{QueuedVerdict, Receiver, VerdictKind};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeRepresentation};
use rtlola_interpreter::{Time, Value};

use crate::config::{Config, EventSourceConfig, Verbosity};
use crate::io::CsvEventSource;
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
#[derive(Debug)]
pub struct OutputHandler<OutputTime: OutputTimeRepresentation> {
    pub(crate) verbosity: Verbosity,
    channel: OutputChannel,
    file: Option<File>,
    pub(crate) statistics: Option<Statistics>,
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
        let statistics = if verbosity == Verbosity::Progress {
            let stats = Statistics::new(ir.triggers.len());
            stats.start_print_progress();
            Some(stats)
        } else if stats == crate::config::Statistics::Debug {
            Some(Statistics::new(ir.triggers.len()))
        } else {
            None
        };

        let or_to_tr = ir.triggers.iter().map(|trigger| (trigger.reference.out_ix(), trigger.trigger_reference)).collect();

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
            format!("({})", paras.iter().map(|p| p.to_string()).collect::<Vec<String>>().join(", "))
        } else {
            String::new()
        }
    }

    pub(crate) fn run(self, input: Receiver<QueuedVerdict<TotalIncremental, OutputTime>>) -> () {
        loop {
            let verdict = match input.recv() {
                Ok(v) => v,
                Err(_) => {
                    self.terminate();
                    return;
                },
            };
            self.new_event();

            let ts = self.output_time.to_string(verdict.ts);
            let TotalIncremental{ inputs, outputs, trigger } = verdict.verdict;
            match verdict.kind {
                VerdictKind::Timed => {
                    self.debug(|| "Deadline reached", &ts);
                },
                VerdictKind::Event => {
                    self.debug(|| "Processing new event", &ts);
                    for (idx, val) in inputs {
                        let name = &self.ir.inputs[idx].name;
                        self.stream(move || format!("[Input][{}][Value] = {}", name, val), &ts);
                    }
                }
            }

            for (out, changes) in outputs {
                let name = &self.ir.outputs[out].name;
                for change in changes {
                    match change {
                        Change::Spawn(parameter) => {
                            self.debug(move || format!("[Output][{}][Spawn] = {}", name, Self::display_parameter(Some(parameter))), &ts);
                        }
                        Change::Value(parameter, val) => {
                            self.stream(move || format!("[Output][{}{}][Value] = {}", name, Self::display_parameter(parameter), val), &ts);
                        }
                        Change::Close(parameter) => {
                            self.debug(move || format!("[Output][{}][Close] = {}", name, Self::display_parameter(Some(parameter))), &ts);
                        }
                    }
                }
            }

            for (trg, msg) in trigger {
                let trigger_ref = self.or_to_tr[&trg];
                self.trigger(move || format!("[#{}] {}", trigger_ref, msg) , trigger_ref, &ts);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn trigger<F, T: Into<String>>(&self, msg: F, trigger_idx: usize, ts: &str)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Triggers, msg, ts);
        if let Some(statistics) = &self.statistics {
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
            self.print( format!("[{}][{}]{}", ts, kind, msg().into()));
        }
    }

    fn print(&self, msg: String) {
        let _ = match self.channel {
            OutputChannel::StdOut => stdout().write((msg + "\n").as_bytes()),
            OutputChannel::StdErr => stderr().write((msg + "\n").as_bytes()),
            OutputChannel::File(_) => self.file.as_ref().unwrap().write(msg.as_bytes()),
        }; // TODO: Decide how to handle the result.
    }

    pub(crate) fn new_event(&self) {
        if let Some(statistics) = &self.statistics {
            statistics.new_event();
        }
    }

    pub(crate) fn terminate(&self) {
        if let Some(statistics) = &self.statistics {
            if self.verbosity == Verbosity::Progress {
                statistics.terminate();
            }
        }
    }
}

#[derive(Debug)]
struct StatisticsData {
    start: SystemTime,
    num_events: AtomicU64,
    num_triggers: Vec<AtomicU64>,
    done: Mutex<bool>,
}

impl StatisticsData {
    fn new(num_trigger: usize) -> Self {
        Self {
            start: SystemTime::now(),
            num_events: AtomicU64::new(0),
            num_triggers: (0..num_trigger).map(|_| AtomicU64::new(0)).collect(),
            done: Mutex::new(false),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Statistics {
    data: Arc<StatisticsData>,
}

impl Statistics {
    fn new(num_trigger: usize) -> Self {
        let data = Arc::new(StatisticsData::new(num_trigger));
        Statistics { data }
    }

    fn start_print_progress(&self) {
        // print intitial info
        Self::print_progress_info(&self.data, ' ');
        let copy = self.data.clone();
        thread::spawn(move || {
            // this thread is responsible for displaying progress information
            let mut spinner = "⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠈⠈ ".chars().cycle();
            loop {
                thread::sleep(Duration::from_millis(100));
                #[allow(clippy::mutex_atomic)]
                let done = copy.done.lock().unwrap();
                if *done {
                    return;
                }
                Self::clear_progress_info();
                Self::print_progress_info(&copy, spinner.next().unwrap());
            }
        });
    }

    fn new_event(&self) {
        self.data.num_events.fetch_add(1, Ordering::Relaxed);
    }

    fn trigger(&self, trigger_idx: usize) {
        self.data.num_triggers[trigger_idx].fetch_add(1, Ordering::Relaxed);
    }

    #[allow(clippy::mutex_atomic)]
    pub(crate) fn terminate(&self) {
        let mut done = self.data.done.lock().unwrap();
        Self::clear_progress_info();
        Self::print_progress_info(&self.data, ' ');
        *done = true;
    }

    fn print_progress_info(data: &Arc<StatisticsData>, spin_char: char) {
        let mut out = stderr();

        // write event statistics
        let now = SystemTime::now();
        let elapsed_total = now.duration_since(data.start).unwrap().as_nanos();
        let num_events: u128 = data.num_events.load(Ordering::Relaxed).into();
        if num_events > 0 {
            let events_per_second = (num_events * Duration::from_secs(1).as_nanos()) / elapsed_total;
            let nanos_per_event = elapsed_total / num_events;
            writeln!(
                out,
                "{} {} events, {} events per second, {} nsec per event",
                spin_char, num_events, events_per_second, nanos_per_event
            )
            .unwrap_or_else(|_| {});
        } else {
            writeln!(out, "{} {} events", spin_char, num_events).unwrap_or_else(|_| {});
        }

        // write trigger statistics
        let num_triggers = data
            .num_triggers
            .iter()
            .fold(0, |val, num_trigger| val + num_trigger.load(Ordering::Relaxed));
        writeln!(out, "  {} triggers", num_triggers).unwrap_or_else(|_| {});
    }

    fn clear_progress_info() {
        let mut stdout = stdout();
        // clear screen as much as written in `print_progress_info`
        execute!(stdout, MoveUp(1u16), Clear(ClearType::CurrentLine)).unwrap_or_else(|_| {});
        execute!(stdout, MoveUp(1u16), Clear(ClearType::CurrentLine)).unwrap_or_else(|_| {});
    }

    #[cfg(test)]
    pub(crate) fn get_num_trigger(&self, trigger_idx: usize) -> u64 {
        self.data.num_triggers[trigger_idx].fetch_add(1, Ordering::Relaxed)
    }
}
