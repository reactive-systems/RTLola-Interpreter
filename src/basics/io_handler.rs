#![allow(clippy::mutex_atomic)]

use super::Time;
use crate::basics::CsvEventSource;
#[cfg(feature = "pcap_interface")]
use crate::basics::PCAPEventSource;
use crate::config::{
    AbsoluteTimeFormat, EvalConfig, EventSourceConfig, RelativeTimeFormat, TimeRepresentation, Verbosity,
};
use crate::storage::Value;
use crossterm::{
    cursor::MoveUp,
    execute,
    terminal::{Clear, ClearType},
};
use rtlola_frontend::mir::RtLolaMir;
use std::error::Error;
use std::fs::File;
use std::io::{stderr, stdout, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

//Input Handling

/// Time as read from the input source
/// Must be converted to the internal time format before use
#[derive(Debug, Clone, Copy)]
pub(crate) enum RawTime {
    Relative(Duration),
    Absolute(SystemTime),
}

impl RawTime {
    pub(crate) fn relative(&self) -> Duration {
        match self {
            RawTime::Relative(d) => *d,
            _ => panic!("Expected relative time"),
        }
    }

    pub(crate) fn absolute(&self) -> SystemTime {
        match self {
            RawTime::Absolute(t) => *t,
            _ => panic!("Expected absolute time"),
        }
    }
}

/// A trait that represents the functionality needed for an event source.
/// The order in which the functions are called is:
/// has_event -> get_event -> read_time
pub(crate) trait EventSource {
    /// Returns true if another event can be obtained from the Event Source
    fn has_event(&mut self) -> bool;

    /// Returns an Event consisting of a vector of Values for the input streams and the time passed since the start of the evaluation
    fn get_event(&mut self) -> (Vec<Value>, RawTime);
}

pub(crate) fn create_event_source(
    config: EventSourceConfig,
    ir: &RtLolaMir,
) -> Result<Box<dyn EventSource>, Box<dyn Error>> {
    use EventSourceConfig::*;
    match config {
        Csv { src } => CsvEventSource::setup(&src, ir),
        #[cfg(feature = "pcap_interface")]
        PCAP { src } => PCAPEventSource::setup(&src, ir),
        Api => unreachable!("Currently, there is no need to create an event source for the API."),
    }
}

// Output Handling

#[derive(Debug, Clone)]
pub enum OutputChannel {
    StdOut,
    StdErr,
    File(PathBuf),
    None,
}

#[derive(Debug)]
pub struct OutputHandler {
    pub(crate) verbosity: Verbosity,
    channel: OutputChannel,
    file: Option<File>,
    pub(crate) statistics: Option<Statistics>,
    start_time: RwLock<Option<SystemTime>>,
    time_representation: TimeRepresentation,
    // Incremental time handling
    last_event: RwLock<Time>,
    is_incremental: bool,
}

impl OutputHandler {
    /// Creates a new Output Handler. If None is given as 'start_time', then the first event determines it.
    pub(crate) fn new(config: &EvalConfig, num_trigger: usize) -> OutputHandler {
        let statistics = if config.verbosity == Verbosity::Progress {
            let stats = Statistics::new(num_trigger);
            stats.start_print_progress();
            Some(stats)
        } else if config.statistics == crate::config::Statistics::Debug {
            Some(Statistics::new(num_trigger))
        } else {
            None
        };
        OutputHandler {
            verbosity: config.verbosity,
            channel: config.output_channel.clone(),
            file: None,
            statistics,
            start_time: RwLock::new(None),
            time_representation: config.output_time_representation,
            last_event: Default::default(),
            is_incremental: matches!(config.output_time_representation, TimeRepresentation::Incremental(_)),
        }
    }

    pub(crate) fn set_start_time(&self, time: SystemTime) {
        *self.start_time.write().unwrap() = Some(time);
    }

    pub(crate) fn runtime_warning<F, T: Into<String>>(&self, msg: F)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::WarningsOnly, msg);
    }

    fn time_info(&self, time: Time) -> String {
        use AbsoluteTimeFormat::*;
        use RelativeTimeFormat::*;
        use TimeRepresentation::*;

        let start = self.start_time.read().unwrap().expect("Start-time not correctly initialized");

        match self.time_representation {
            Relative(format) => match format {
                UIntNanos => format!("{}", time.as_nanos()),
                FloatSecs => format!("{}.{:09}", time.as_secs(), time.subsec_nanos()),
            },
            Incremental(format) => {
                let diff = time - *self.last_event.read().unwrap();
                match format {
                    UIntNanos => format!("{}", diff.as_nanos()),
                    FloatSecs => format!("{}.{:09}", diff.as_secs(), diff.subsec_nanos()),
                }
            }
            Absolute(format) => {
                let time = start + time;
                // Convert to unix timestamp
                let absolute = time.duration_since(UNIX_EPOCH).expect("Computation of duration failed!");
                match format {
                    UnixTimeFloat => format!("{}.{:09}", absolute.as_secs(), absolute.subsec_nanos()),
                    Rfc3339 => format!("{}", humantime::format_rfc3339(time)),
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn trigger<F, T: Into<String>>(&self, msg: F, trigger_idx: usize, time: Time)
    where
        F: FnOnce() -> T,
    {
        let msg = || format!("{}: {}", self.time_info(time), msg().into());
        self.emit(Verbosity::Triggers, msg);
        if let Some(statistics) = &self.statistics {
            statistics.trigger(trigger_idx);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn debug<F, T: Into<String>>(&self, msg: F)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Debug, msg);
    }

    #[allow(dead_code)]
    pub(crate) fn output<F, T: Into<String>>(&self, msg: F)
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Outputs, msg);
    }

    /// Accepts a message and forwards it to the appropriate output channel.
    /// If the configuration prohibits printing the message, `msg` is never called.
    fn emit<F, T: Into<String>>(&self, kind: Verbosity, msg: F)
    where
        F: FnOnce() -> T,
    {
        if kind <= self.verbosity {
            self.print(msg().into());
        }
    }

    fn print(&self, msg: String) {
        let _ = match self.channel {
            OutputChannel::StdOut => stdout().write((msg + "\n").as_bytes()),
            OutputChannel::StdErr => stderr().write((msg + "\n").as_bytes()),
            OutputChannel::File(_) => self.file.as_ref().unwrap().write(msg.as_bytes()),
            OutputChannel::None => Ok(0),
        }; // TODO: Decide how to handle the result.
    }

    pub(crate) fn new_input(&self, ts: Time) {
        if self.is_incremental {
            *self.last_event.write().unwrap() = ts;
        }
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
        let num_triggers =
            data.num_triggers.iter().fold(0, |val, num_trigger| val + num_trigger.load(Ordering::Relaxed));
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
