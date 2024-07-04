//! Module that implements [VerdictsSink] to represent the verdicts in JSONL format.
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{ErrorKind, Write};

use jsonl::WriteError;
use rtlola_interpreter::monitor::TotalIncremental;
use rtlola_interpreter::rtlola_mir::{OutputReference, RtLolaMir};
use rtlola_interpreter::time::OutputTimeRepresentation;
use serde::Serialize;

use crate::{VerdictFactory, VerdictsSink};

/// Print the verdicts in JSONL format to the writer
#[derive(Debug)]
pub struct JsonlSink<O: OutputTimeRepresentation, W: Write> {
    writer: W,
    factory: JsonFactory<O>,
}

impl<O: OutputTimeRepresentation, W: Write> JsonlSink<O, W> {
    /// Construct a new sink for the given specification that writes to the supplied writer
    pub fn new(ir: &RtLolaMir, writer: W) -> Self {
        let output_names = ir
            .outputs
            .iter()
            .map(|output| (output.reference.out_ix(), output.name.clone()))
            .collect();
        let factory = JsonFactory {
            output_names,
            output_time: O::default(),
        };
        Self { writer, factory }
    }
}

impl<OutputTime: OutputTimeRepresentation, W: Write> VerdictsSink<TotalIncremental, OutputTime>
    for JsonlSink<OutputTime, W>
{
    type Error = Infallible;
    type Factory = JsonFactory<OutputTime>;
    type Return = ();

    fn sink(&mut self, verdict: JsonVerdict) -> Result<Self::Return, Self::Error> {
        if !verdict.updates.is_empty() {
            match jsonl::write(&mut self.writer, &verdict) {
                Ok(_) => {
                    self.writer.flush().unwrap();
                },
                Err(WriteError::Io(e)) if e.kind() == ErrorKind::BrokenPipe => {
                    // we could exit the whole verdict factory / sink etc here.
                },
                Err(e) => panic!("{:?}", e),
            }
        }
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

/// Factory to construct the JSON representation for a single verdict
#[derive(Debug)]
pub struct JsonFactory<OutputTime: OutputTimeRepresentation> {
    output_names: HashMap<OutputReference, String>,
    output_time: OutputTime,
}

/// The JSON representation of a single verdict
#[derive(Serialize, Debug)]
pub struct JsonVerdict {
    time: String,
    #[serde(flatten)]
    updates: HashMap<String, ValueUpdate>,
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum ValueUpdate {
    Value(String),
    Instances(Vec<InstanceUpdate>),
}

#[derive(Serialize, Debug)]
struct InstanceUpdate {
    instance: Vec<String>,
    value: String,
}

impl<O: OutputTimeRepresentation> VerdictFactory<TotalIncremental, O> for JsonFactory<O> {
    type Error = Infallible;
    type Verdict = JsonVerdict;

    fn get_verdict(&mut self, rec: TotalIncremental, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        let mut updates = HashMap::new();

        for (stream, changes) in rec.outputs {
            let stream_name = &self.output_names[&stream];
            for change in changes {
                match change {
                    rtlola_interpreter::monitor::Change::Spawn(_) => {}, // we could also build a more detailed jsonl logger that also includes spawn and close
                    rtlola_interpreter::monitor::Change::Value(None, v) => {
                        updates.insert(stream_name.clone(), ValueUpdate::Value(v.to_string()));
                    },
                    rtlola_interpreter::monitor::Change::Value(Some(p), v) if p == [] => {
                        updates.insert(stream_name.clone(), ValueUpdate::Value(v.to_string()));
                    },
                    rtlola_interpreter::monitor::Change::Value(Some(parameter), v) => {
                        if !updates.contains_key(stream_name) {
                            updates.insert(stream_name.clone(), ValueUpdate::Instances(Vec::new()));
                        }
                        let ValueUpdate::Instances(instances_map) = updates.get_mut(stream_name).unwrap() else {
                            panic!("Verdict contained unparameterized update as well as parameterized one for the same stream");
                        };
                        instances_map.push(InstanceUpdate {
                            instance: parameter.into_iter().map(|p| p.to_string()).collect(),
                            value: v.to_string(),
                        });
                    },
                    rtlola_interpreter::monitor::Change::Close(_) => {},
                }
            }
        }
        Ok(JsonVerdict {
            updates,
            time: self.output_time.to_string(ts),
        })
    }
}
