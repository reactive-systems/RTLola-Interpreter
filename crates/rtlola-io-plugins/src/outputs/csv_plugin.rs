//! An output plugin that produces output in csv format

use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Write;
use std::iter;

use rtlola_interpreter::monitor::TotalIncremental;
use rtlola_interpreter::output::NewVerdictFactory;
use rtlola_interpreter::rtlola_frontend::tag_parser::verbosity_parser::StreamVerbosity;
use rtlola_interpreter::rtlola_mir::{RtLolaMir, StreamReference};
use rtlola_interpreter::time::OutputTimeRepresentation;

use super::{CliAnnotations, VerdictFactory, VerdictsSink};

/// A verdict sink to write the new values of the output streams in CSV format
///
/// Note: only suitable on specifications that do not contain parameterized streams
#[derive(Debug)]
pub struct CsvVerdictSink<O: OutputTimeRepresentation, W: Write> {
    writer: csv::Writer<W>,
    factory: CsvVerdictFactory<O>,
}

#[derive(PartialEq, Ord, PartialOrd, Eq, Debug, Clone, Copy)]
/// The verbosity of the CSV output
pub enum CsvVerbosity {
    /// don't print anything (except streams explicitly marked as debug)
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
}

impl From<StreamVerbosity> for CsvVerbosity {
    fn from(value: StreamVerbosity) -> Self {
        match value {
            StreamVerbosity::Streams => CsvVerbosity::Streams,
            StreamVerbosity::Outputs => CsvVerbosity::Outputs,
            StreamVerbosity::Public => CsvVerbosity::Public,
            StreamVerbosity::Warnings => CsvVerbosity::Warnings,
            StreamVerbosity::Violations => CsvVerbosity::Violations,
        }
    }
}

impl<O: OutputTimeRepresentation, W: Write> CsvVerdictSink<O, W> {
    /// Construct a CsvVerdictSink to print the verdicts according to the specified verbosity to CSV
    pub fn for_verbosity(
        ir: &RtLolaMir,
        writer: W,
        verbosity: CsvVerbosity,
        annotations: CliAnnotations,
    ) -> Result<Self, String> {
        let verbosity_map = ir
            .all_streams()
            .filter_map(|s| {
                match Self::include_stream(s, verbosity, &annotations) {
                    Ok(true) => Some(Ok(s)),
                    Ok(false) => None,
                    Err(e) => Some(Err(e)),
                }
            })
            .enumerate()
            .map(|(i, sr)| Ok((sr?, i)))
            .collect::<Result<_, String>>()?;

        Self::new(ir, writer, verbosity_map)
    }

    /// Construct a CsvVerdictSink to print the verdicts of the specified streams to CSV
    pub fn for_streams(ir: &RtLolaMir, writer: W, streams: Vec<StreamReference>) -> Result<Self, String> {
        let stream_map = streams.into_iter().enumerate().map(|(i, s)| (s, i)).collect();

        Self::new(ir, writer, stream_map)
    }

    fn include_stream(
        sr: StreamReference,
        verbosity: CsvVerbosity,
        annotations: &CliAnnotations,
    ) -> Result<bool, String> {
        let include = verbosity >= CsvVerbosity::from(annotations.verbosity(sr)) || annotations.debug(sr);
        Ok(include)
    }

    /// Construct a new sink for the given specification that writes to the supplied writer
    fn new(ir: &RtLolaMir, writer: W, stream_map: HashMap<StreamReference, usize>) -> Result<Self, String> {
        for &stream in stream_map.keys() {
            let stream = ir.stream(stream);
            if stream.is_parameterized() {
                return Err(format!("Trying to output parameterized stream \"{}\", but CSV output format only supported for unparameterized specifications.", stream.name()));
            }
        }

        debug_assert!((0..stream_map.len()).all(|col| stream_map.values().any(|v| *v == col)));

        let factory = CsvVerdictFactory::new(ir, stream_map).unwrap();
        let mut writer = csv::Writer::from_writer(writer);

        if !factory.stream_map.is_empty() {
            writer
                .write_record(
                    iter::once("time")
                        .chain(
                            ir.all_streams()
                                .filter(|s| factory.stream_map.contains_key(s))
                                .map(|s| ir.stream(s).name()),
                        )
                        .collect::<Vec<&str>>(),
                )
                .unwrap();
        }
        Ok(Self { writer, factory })
    }
}

impl<O: OutputTimeRepresentation, W: Write> VerdictsSink<TotalIncremental, O> for CsvVerdictSink<O, W> {
    type Error = Infallible;
    type Factory = CsvVerdictFactory<O>;
    type Return = ();

    fn sink(&mut self, verdict: Option<Vec<String>>) -> Result<Self::Return, Self::Error> {
        if let Some(verdict) = verdict {
            self.writer.write_record(verdict).unwrap();
            self.writer.flush().unwrap();
        }
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// Factory to construct the CSV representation for a single verdict
#[derive(Debug)]
pub struct CsvVerdictFactory<O: OutputTimeRepresentation> {
    output_time: O,
    stream_map: HashMap<StreamReference, usize>,
}

impl<O: OutputTimeRepresentation> VerdictFactory<TotalIncremental, O> for CsvVerdictFactory<O> {
    type Error = Infallible;
    type Verdict = Option<Vec<String>>;

    fn get_verdict(&mut self, rec: TotalIncremental, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        if self.stream_map.is_empty() {
            return Ok(None);
        }

        let mut values = vec![None; self.stream_map.len()];

        for (input, value) in rec.inputs {
            if let Some(index) = self.stream_map.get(&StreamReference::In(input)) {
                values[*index] = Some(value);
            }
        }
        for (output, changes) in rec.outputs {
            if let Some(index) = self.stream_map.get(&StreamReference::Out(output)) {
                for change in changes {
                    match change {
                        rtlola_interpreter::monitor::Change::Spawn(_) => {},
                        rtlola_interpreter::monitor::Change::Value(None, v) => values[*index] = Some(v),
                        rtlola_interpreter::monitor::Change::Value(Some(p), v) if p.is_empty() => {
                            values[*index] = Some(v)
                        },
                        rtlola_interpreter::monitor::Change::Value(Some(_), _) => unreachable!("checked in new"),
                        rtlola_interpreter::monitor::Change::Close(_) => {},
                    }
                }
            };
        }
        if values.iter().all(|v| v.is_none()) {
            Ok(None)
        } else {
            Ok(Some(
                iter::once(self.output_time.to_string(ts))
                    .chain(
                        values
                            .into_iter()
                            .map(|v| v.map(|v| v.to_string()).unwrap_or_else(|| "#".into())),
                    )
                    .collect(),
            ))
        }
    }
}

impl<O: OutputTimeRepresentation> NewVerdictFactory<TotalIncremental, O> for CsvVerdictFactory<O> {
    type CreationData = HashMap<StreamReference, usize>;

    fn new(_ir: &RtLolaMir, data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self {
            output_time: O::default(),
            stream_map: data,
        })
    }
}
