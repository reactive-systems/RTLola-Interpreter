//! Module that contains the implementation of the default [VerdictsSink] used by the CLI for printing log messages
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use rtlola_interpreter::monitor::{Change, Parameters, TotalIncremental};
use rtlola_interpreter::rtlola_mir::{OutputReference, RtLolaMir, StreamReference, TriggerReference};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_interpreter::Value;
use termcolor::{Color, ColorSpec, WriteColor};

use super::{DiscardSink, VerdictFactory};

#[derive(PartialEq, Ord, PartialOrd, Eq, Debug, Clone, Copy)]
/// The verbosity of the log printer output
pub enum Verbosity {
    /// only print the values of the trigger
    Trigger,
    /// print the values of the outputs (including trigger)
    Outputs,
    /// print the values of all streams (including inputs)
    Streams,
    /// also print the spawn and close of streams
    Debug,
}

impl Display for Verbosity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Verbosity::Trigger => write!(f, "Trigger"),
            Verbosity::Outputs => write!(f, "Outputs"),
            Verbosity::Streams => write!(f, "Stream"),
            Verbosity::Debug => write!(f, "Debug"),
        }
    }
}

impl From<Verbosity> for Color {
    fn from(v: Verbosity) -> Self {
        match v {
            Verbosity::Trigger => Color::Ansi256(1), //Dark Red
            Verbosity::Outputs => Color::Ansi256(4), //Dark Blue
            Verbosity::Streams => Color::Ansi256(5), //Dark Magenta
            Verbosity::Debug => Color::Ansi256(8),   //Dark Grey
        }
    }
}

/// A VerdictFactory turning the verdict into a line-based logging format
#[derive(Debug)]
pub struct LogPrinter<OutputTime: OutputTimeRepresentation, W: WriteColor> {
    output_time: OutputTime,
    verbosity: Verbosity,
    stream_names: HashMap<StreamReference, String>,
    trigger_ids: HashMap<OutputReference, TriggerReference>,
    writer: W,
}

impl<OutputTime: OutputTimeRepresentation, W: WriteColor> LogPrinter<OutputTime, W> {
    /// Construct a new LogPrinter based on the given verbosity
    pub fn new(verbosity: Verbosity, ir: &RtLolaMir, writer: W) -> Self {
        let stream_names = ir.all_streams().map(|s| (s, ir.stream(s).name().to_owned())).collect();
        let trigger_ids = ir
            .triggers
            .iter()
            .map(|trigger| (trigger.output_reference.out_ix(), trigger.trigger_reference))
            .collect();
        Self {
            output_time: OutputTime::default(),
            verbosity,
            stream_names,
            trigger_ids,
            writer,
        }
    }

    /// Turn the LogPrinter into a VerdictSink writing the logs into a writer
    pub fn sink(self) -> DiscardSink<OutputTime, TotalIncremental, Self> {
        DiscardSink::new(self)
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

    fn display_value(value: Value) -> String {
        match value {
            Value::Str(s) => format!("\"{s}\""),
            other => other.to_string(),
        }
    }
}

impl<OutputTime: OutputTimeRepresentation, W: WriteColor> VerdictFactory<TotalIncremental, OutputTime>
    for LogPrinter<OutputTime, W>
{
    type Error = std::io::Error;
    type Verdict = ();

    fn get_verdict(
        &mut self,
        verdict: TotalIncremental,
        ts: OutputTime::InnerTime,
    ) -> Result<Self::Verdict, Self::Error> {
        let TotalIncremental {
            inputs,
            outputs,
            trigger: _,
        } = verdict;

        let ts = self.output_time.to_string(ts);

        for (idx, val) in inputs {
            let name = self.stream_names[&StreamReference::In(idx)].clone();
            self.input(
                move || format!("[Input][{}][Value] = {}", name, Self::display_value(val)),
                &ts,
            )?;
        }

        for (out, changes) in outputs {
            let name = match self.trigger_ids.get(&out) {
                None => format!("[Output][{}", self.stream_names[&StreamReference::Out(out)]),
                Some(idx) => format!("[Trigger][#{idx}"),
            };
            let name = &name;
            for change in changes {
                match change {
                    Change::Spawn(parameter) => {
                        self.debug(
                            || format!("{}][Spawn] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                        )?;
                    },
                    Change::Value(parameter, val) => {
                        let msg = move || {
                            format!(
                                "{}{}][Value] = {}",
                                name,
                                Self::display_parameter(parameter),
                                Self::display_value(val)
                            )
                        };
                        let is_trigger = self.trigger_ids.contains_key(&out);
                        self.output(msg, &ts, is_trigger)?;
                    },
                    Change::Close(parameter) => {
                        self.debug(
                            move || format!("{}][Close] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                        )?;
                    },
                }
            }
        }
        Ok(())
    }
}

impl<O: OutputTimeRepresentation, W: WriteColor> LogPrinter<O, W> {
    /// Accepts a message and forwards it to the appropriate output channel.
    /// If the configuration prohibits printing the message, `msg` is never called.
    fn emit<F, T: Into<String>>(&mut self, kind: Verbosity, msg: F, ts: &str) -> std::io::Result<()>
    where
        F: FnOnce() -> T,
    {
        if kind <= self.verbosity {
            write!(&mut self.writer, "[{}]", ts)?;
            self.writer.set_color(ColorSpec::default().set_fg(Some(kind.into())))?;
            writeln!(&mut self.writer, "{}", msg().into())?;
            self.writer.reset()
        } else {
            Ok(())
        }
    }

    fn debug<F, T: Into<String>>(&mut self, msg: F, ts: &str) -> std::io::Result<()>
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Debug, msg, ts)
    }

    fn input<F, T: Into<String>>(&mut self, msg: F, ts: &str) -> std::io::Result<()>
    where
        F: FnOnce() -> T,
    {
        self.emit(Verbosity::Streams, msg, ts)
    }

    fn output<F, T: Into<String>>(&mut self, msg: F, ts: &str, is_trigger: bool) -> std::io::Result<()>
    where
        F: FnOnce() -> T,
    {
        if is_trigger {
            self.emit(Verbosity::Trigger, msg, ts)
        } else {
            self.emit(Verbosity::Outputs, msg, ts)
        }
    }
}
