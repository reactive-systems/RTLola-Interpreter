//! Module that contains the implementation of the default [VerdictsSink] used by the CLI for printing log messages
use std::convert::Infallible;
use std::io::Write;

use rtlola_interpreter::monitor::{Change, Parameters, TotalIncremental};
use rtlola_interpreter::rtlola_mir::{OutputKind, RtLolaMir};
use rtlola_interpreter::time::OutputTimeRepresentation;

use crate::{VerdictFactory, VerdictsSink};

pub trait Logger {
    fn debug<F, T: Into<String>>(&self, out: &mut impl Write, msg: F, ts: &str)
    where
        F: FnOnce() -> T;
    fn stream<F, T: Into<String>>(&self, out: &mut impl Write, msg: F, ts: &str, is_trigger: bool)
    where
        F: FnOnce() -> T;
}

#[derive(Debug)]
pub struct LogPrinter<OutputTime: OutputTimeRepresentation, L: Logger, W: Write> {
    output_time: OutputTime,
    logger: L,
    writer: W,
    ir: RtLolaMir,

    factory: TotalIncrementalTimestamped,
}

impl<OutputTime: OutputTimeRepresentation, L: Logger, W: Write> LogPrinter<OutputTime, L, W> {
    pub fn new(logger: L, writer: W, ir: &RtLolaMir) -> Self {
        Self {
            output_time: OutputTime::default(),
            logger,
            writer,
            factory: TotalIncrementalTimestamped,
            ir: ir.clone(),
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
}

/// Simply attach the timestamp next to the total incremental verdict
#[derive(Debug, Clone, Copy)]
pub struct TotalIncrementalTimestamped;

impl<OutputTime: OutputTimeRepresentation> VerdictFactory<TotalIncremental, OutputTime>
    for TotalIncrementalTimestamped
{
    type Error = Infallible;
    type Verdict = (TotalIncremental, OutputTime::InnerTime);

    fn get_verdict(&mut self, rec: TotalIncremental, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error> {
        Ok((rec, ts))
    }
}

impl<OutputTime: OutputTimeRepresentation, L: Logger, W: Write> VerdictsSink<TotalIncremental, OutputTime>
    for LogPrinter<OutputTime, L, W>
{
    type Error = Infallible;
    type Factory = TotalIncrementalTimestamped;
    type Return = ();

    fn sink(&mut self, (verdict, ts): (TotalIncremental, OutputTime::InnerTime)) -> Result<Self::Return, Self::Error> {
        let TotalIncremental {
            inputs,
            outputs,
            trigger: _,
        } = verdict;

        let ts = self.output_time.to_string(ts);

        for (idx, val) in inputs {
            let name = &self.ir.inputs[idx].name;
            self.logger.debug(
                &mut self.writer,
                move || format!("[Input][{}][Value] = {}", name, val),
                &ts,
            );
        }

        for (out, changes) in outputs {
            let kind = &self.ir.outputs[out].kind;
            let name = match kind {
                OutputKind::NamedOutput(name) => format!("[Output][{name}"),
                OutputKind::Trigger(idx) => format!("[#{idx}"),
            };
            let name = &name;
            for change in changes {
                match change {
                    Change::Spawn(parameter) => {
                        self.logger.debug(
                            &mut self.writer,
                            || format!("{}][Spawn] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                        );
                    },
                    Change::Value(parameter, val) => {
                        let msg = move || format!("{}{}][Value] = {}", name, Self::display_parameter(parameter), val);
                        let is_trigger = matches!(kind, OutputKind::Trigger(_));
                        self.logger.stream(&mut self.writer, msg, &ts, is_trigger);
                    },
                    Change::Close(parameter) => {
                        self.logger.debug(
                            &mut self.writer,
                            move || format!("{}][Close] = {}", name, Self::display_parameter(Some(parameter))),
                            &ts,
                        );
                    },
                }
            }
        }
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}
