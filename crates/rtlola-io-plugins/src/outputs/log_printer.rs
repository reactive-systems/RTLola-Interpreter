//! Module that contains the implementation of the default [VerdictsSink](crate::outputs::VerdictsSink) used by the CLI for printing log messages
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::marker::PhantomData;

use rtlola_interpreter::monitor::{Change, Parameters, TotalIncremental};
use rtlola_interpreter::output::NewVerdictFactory;
use rtlola_interpreter::rtlola_frontend::tag_parser::verbosity_parser::StreamVerbosity;
use rtlola_interpreter::rtlola_mir::{OutputReference, RtLolaMir, StreamReference, TriggerReference};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_interpreter::Value;
use termcolor::{Ansi, Color, ColorSpec, NoColor, WriteColor};

use super::{ByteSink, CliAnnotations, VerdictFactory};

/// A trait that captures color writers that can forward the result to a regular `std::io::write` implementor.
pub trait IndirectWriteColor<W: Write>: WriteColor {
    /// Creates a new Indirect Writer given a writer.
    fn for_writer(write: W) -> Self;

    /// Deconstructs self into its inner writer.
    fn into_inner(self) -> W;
}

impl<W: Write> IndirectWriteColor<W> for Ansi<W> {
    fn for_writer(write: W) -> Self {
        Ansi::new(write)
    }

    fn into_inner(self) -> W {
        Ansi::into_inner(self)
    }
}

impl<W: Write> IndirectWriteColor<W> for NoColor<W> {
    fn for_writer(write: W) -> Self {
        NoColor::new(write)
    }

    fn into_inner(self) -> W {
        NoColor::into_inner(self)
    }
}

#[derive(PartialEq, Ord, PartialOrd, Eq, Debug, Clone, Copy)]
/// The verbosity of the log printer output
pub enum Verbosity {
    /// print nothing (besides explicitly debugged streams)
    Silent,
    /// only print trigger violations
    Violations,
    /// only print trigger violations and warnings
    Warnings,
    /// print public output streams
    Public,
    /// print the values of the outputs (including trigger)
    Outputs,
    /// print the values of all streams (including inputs)
    Streams,
    /// also print the spawn and close of streams
    Debug,
}

impl From<StreamVerbosity> for Verbosity {
    fn from(value: StreamVerbosity) -> Self {
        match value {
            StreamVerbosity::Streams => Verbosity::Streams,
            StreamVerbosity::Outputs => Verbosity::Outputs,
            StreamVerbosity::Public => Verbosity::Public,
            StreamVerbosity::Warnings => Verbosity::Warnings,
            StreamVerbosity::Violations => Verbosity::Violations,
        }
    }
}

impl Display for Verbosity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Verbosity::Silent => write!(f, "Silent"),
            Verbosity::Violations => write!(f, "Violation"),
            Verbosity::Warnings => write!(f, "Warning"),
            Verbosity::Public => write!(f, "Public"),
            Verbosity::Outputs => write!(f, "Outputs"),
            Verbosity::Streams => write!(f, "Stream"),
            Verbosity::Debug => write!(f, "Debug"),
        }
    }
}

impl From<Verbosity> for Color {
    fn from(v: Verbosity) -> Self {
        match v {
            Verbosity::Silent => unreachable!(),
            Verbosity::Violations => Color::Ansi256(1), //Dark Red
            Verbosity::Warnings => Color::Ansi256(3),   //Dark Yellow
            Verbosity::Public => Color::Ansi256(4),     //Dark Blue
            Verbosity::Outputs => Color::Ansi256(4),    //Dark Blue
            Verbosity::Streams => Color::Ansi256(5),    //Dark Magenta
            Verbosity::Debug => Color::Ansi256(8),      //Dark Grey
        }
    }
}

/// A VerdictFactory turning the verdict into a line-based logging format
#[derive(Debug)]
pub struct LogPrinter<OutputTime: OutputTimeRepresentation, W: IndirectWriteColor<Vec<u8>>> {
    output_time: OutputTime,
    verbosity: Verbosity,
    stream_names: HashMap<StreamReference, String>,
    trigger_ids: HashMap<OutputReference, TriggerReference>,
    stream_verbosity: HashMap<StreamReference, Verbosity>,
    debug_streams: HashSet<StreamReference>,
    writer: PhantomData<W>,
}

impl<OutputTime: OutputTimeRepresentation, W: IndirectWriteColor<Vec<u8>>> LogPrinter<OutputTime, W> {
    /// Construct a new LogPrinter based on the given verbosity
    pub fn new(verbosity: Verbosity, ir: &RtLolaMir, annotations: CliAnnotations) -> Result<Self, String> {
        let stream_names = ir.all_streams().map(|s| (s, ir.stream(s).name().to_owned())).collect();
        let trigger_ids = ir
            .triggers
            .iter()
            .map(|trigger| (trigger.output_reference.out_ix(), trigger.trigger_reference))
            .collect();
        let stream_verbosity = ir
            .all_streams()
            .map(|stream| Ok((stream, Verbosity::from(annotations.verbosity(stream)))))
            .collect::<Result<_, String>>()?;
        let debug_streams = ir.all_streams().filter(|sr| annotations.debug(*sr)).collect();
        Ok(Self {
            output_time: OutputTime::default(),
            verbosity,
            stream_names,
            trigger_ids,
            stream_verbosity,
            debug_streams,
            writer: PhantomData,
        })
    }

    /// Turn the LogPrinter into a VerdictSink writing the logs into a writer
    pub fn sink<OW: Write>(self, writer: OW) -> ByteSink<OW, Self, TotalIncremental, OutputTime> {
        ByteSink::new(writer, self)
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

impl<OutputTime: OutputTimeRepresentation, W: IndirectWriteColor<Vec<u8>>> VerdictFactory<TotalIncremental, OutputTime>
    for LogPrinter<OutputTime, W>
{
    type Error = std::io::Error;
    type Verdict = Vec<u8>;

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

        if self.verbosity == Verbosity::Silent && self.debug_streams.is_empty() {
            return Ok(Vec::new());
        }

        let ts = self.output_time.to_string(ts);
        let mut writer = W::for_writer(Vec::new());

        for (idx, val) in inputs {
            let sr = StreamReference::In(idx);
            let name = &self.stream_names[&sr];
            self.emit(
                &mut writer,
                self.stream_verbosity[&sr],
                move |w| write!(w, "[Input][{}][Value] = {}", name, Self::display_value(val)),
                &ts,
                sr,
            )?;
        }

        for (out, changes) in outputs {
            let sr = StreamReference::Out(out);
            let name = match self.trigger_ids.get(&out) {
                None => format!("[Output][{}", self.stream_names[&sr]),
                Some(idx) => format!("[Trigger][#{idx}"),
            };
            let name = &name;
            for change in changes {
                match change {
                    Change::Spawn(parameter) => {
                        self.debug(
                            &mut writer,
                            |w| write!(w, "{}][Spawn] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                            sr,
                        )?;
                    },
                    Change::Value(parameter, val) => {
                        let msg = move |w: &mut W| {
                            write!(
                                w,
                                "{}{}][Value] = {}",
                                name,
                                Self::display_parameter(parameter),
                                Self::display_value(val)
                            )
                        };
                        self.emit(&mut writer, self.stream_verbosity[&sr], msg, &ts, sr)?;
                    },
                    Change::Close(parameter) => {
                        self.debug(
                            &mut writer,
                            move |w| write!(w, "{}][Close] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                            sr,
                        )?;
                    },
                }
            }
        }

        writer.flush()?;
        Ok(writer.into_inner())
    }
}

impl<OutputTime: OutputTimeRepresentation, W: IndirectWriteColor<Vec<u8>>>
    NewVerdictFactory<TotalIncremental, OutputTime> for LogPrinter<OutputTime, W>
{
    type CreationData = Verbosity;

    fn new(ir: &RtLolaMir, data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self::new(data, ir))
    }
}

impl<O: OutputTimeRepresentation, W: IndirectWriteColor<Vec<u8>>> LogPrinter<O, W> {
    /// Accepts a message and forwards it to the appropriate output channel.
    /// If the configuration prohibits printing the message, `msg` is never called.
    fn emit<F>(&self, out: &mut W, kind: Verbosity, msg: F, ts: &str, sr: StreamReference) -> std::io::Result<()>
    where
        F: FnOnce(&mut W) -> std::io::Result<()>,
    {
        if kind <= self.verbosity || self.debug_streams.contains(&sr) {
            write!(out, "[{}]", ts)?;
            out.set_color(ColorSpec::default().set_fg(Some(kind.into())))?;
            msg(out)?;
            writeln!(out)?;
            out.reset()
        } else {
            Ok(())
        }
    }

    fn debug<F>(&self, out: &mut W, msg: F, ts: &str, sr: StreamReference) -> std::io::Result<()>
    where
        F: FnOnce(&mut W) -> std::io::Result<()>,
    {
        self.emit(out, Verbosity::Debug, msg, ts, sr)
    }
}
