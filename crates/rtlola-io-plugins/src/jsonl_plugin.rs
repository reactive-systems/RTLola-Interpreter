//! Module that implements [VerdictsSink] to represent the verdicts in JSONL format.
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::io::Write;

use jsonl::WriteError;
use rtlola_interpreter::monitor::{Change, TotalIncremental};
use rtlola_interpreter::rtlola_mir::{OutputReference, RtLolaMir, StreamReference};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_interpreter::Value;
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
    pub fn new(ir: &RtLolaMir, writer: W, verbosity: JsonVerbosity) -> Self {
        let stream_names = ir
            .all_streams()
            .map(|stream| (stream, ir.stream(stream).name().to_owned()))
            .collect();
        let triggers = ir
            .triggers
            .iter()
            .map(|trigger| trigger.output_reference.out_ix())
            .collect();
        let factory = JsonFactory {
            stream_names,
            output_time: O::default(),
            verbosity,
            triggers,
        };
        Self { writer, factory }
    }
}

impl<OutputTime: OutputTimeRepresentation, W: Write> VerdictsSink<TotalIncremental, OutputTime>
    for JsonlSink<OutputTime, W>
{
    type Error = WriteError;
    type Factory = JsonFactory<OutputTime>;
    type Return = ();

    fn sink(&mut self, verdict: JsonVerdict) -> Result<Self::Return, Self::Error> {
        if !verdict.updates.is_empty() {
            jsonl::write(&mut self.writer, &verdict)?;
            self.writer.flush()?;
        }
        Ok(())
    }

    fn factory(&mut self) -> &mut Self::Factory {
        &mut self.factory
    }
}

#[derive(PartialEq, Ord, PartialOrd, Eq, Debug, Clone, Copy)]
/// The verbosity of the JSON output
pub enum JsonVerbosity {
    /// also print the spawn and close of streams
    Debug,
    /// print the values of all streams (including inputs)
    Streams,
    /// print the values of the outputs (including trigger)
    Outputs,
    /// only print the values of the trigger
    Trigger,
}

impl JsonVerbosity {
    fn include_inputs(&self) -> bool {
        self <= &JsonVerbosity::Streams
    }

    fn include_non_trigger_outputs(&self) -> bool {
        self <= &JsonVerbosity::Outputs
    }

    fn include_triggers(&self) -> bool {
        true
    }

    fn include_spawn_and_close(&self) -> bool {
        self <= &JsonVerbosity::Debug
    }
}

/// Factory to construct the JSON representation for a single verdict
#[derive(Debug)]
pub struct JsonFactory<OutputTime: OutputTimeRepresentation> {
    stream_names: HashMap<StreamReference, String>,
    output_time: OutputTime,
    triggers: HashSet<OutputReference>,
    verbosity: JsonVerbosity,
}

impl<O: OutputTimeRepresentation> JsonFactory<O> {
    fn include_stream(&self, stream: StreamReference) -> bool {
        match stream {
            StreamReference::In(_) => self.verbosity.include_inputs(),
            StreamReference::Out(o) if self.triggers.contains(&o) => self.verbosity.include_triggers(),
            StreamReference::Out(_) => self.verbosity.include_non_trigger_outputs(),
        }
    }

    fn include_change(&self, change: &Change) -> bool {
        match change {
            Change::Spawn(_) | Change::Close(_) => self.verbosity.include_spawn_and_close(),
            Change::Value(_, _) => true,
        }
    }
}

/// The JSON representation of a single verdict
#[derive(Serialize, Debug)]
pub struct JsonVerdict {
    time: String,
    #[serde(flatten)]
    updates: HashMap<String, Vec<InstanceUpdate>>,
}

///
#[derive(Serialize, Debug)]
pub struct InstanceUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    instance: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    spawn: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    eval: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    close: bool,
}

impl InstanceUpdate {
    fn new(instance: &Option<&Vec<Value>>) -> Self {
        Self {
            instance: instance
                .map(|instances| instances.into_iter().map(|v| v.to_string()).collect())
                .unwrap_or_else(Vec::new),
            spawn: Default::default(),
            eval: Default::default(),
            close: Default::default(),
        }
    }

    fn input(value: &Value) -> Self {
        Self {
            instance: Vec::new(),
            spawn: Default::default(),
            eval: Some(value.to_string()),
            close: Default::default(),
        }
    }
}

impl<O: OutputTimeRepresentation> VerdictFactory<TotalIncremental, O> for JsonFactory<O> {
    type Error = Infallible;
    type Verdict = JsonVerdict;

    fn get_verdict(&mut self, rec: TotalIncremental, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        let updates = rec
            .outputs
            .iter()
            .filter(|(s, _)| self.include_stream(StreamReference::Out(*s)))
            .flat_map(|(stream, changes)| {
                let stream_name = &self.stream_names[&StreamReference::Out(*stream)];
                let mut instances = HashMap::new();
                for change in changes {
                    if !self.include_change(change) {
                        continue;
                    }
                    let instance = match &change {
                        Change::Spawn(i) | Change::Value(Some(i), _) | Change::Close(i) => Some(i),
                        Change::Value(None, _) => None,
                    };
                    let entry = instances.entry(instance).or_insert_with_key(InstanceUpdate::new);
                    match &change {
                        rtlola_interpreter::monitor::Change::Spawn(_) => {
                            entry.spawn = true;
                        },
                        rtlola_interpreter::monitor::Change::Value(_, v) => {
                            entry.eval = Some(v.to_string());
                        },
                        rtlola_interpreter::monitor::Change::Close(_) => {
                            entry.close = true;
                        },
                    }
                }
                (!instances.is_empty()).then(|| (stream_name.clone(), instances.into_values().collect::<Vec<_>>()))
            })
            .chain(
                (self.verbosity.include_inputs())
                    .then(|| {
                        rec.inputs.iter().map(|(stream, value)| {
                            let stream_name = &self.stream_names[&StreamReference::In(*stream)];
                            (stream_name.clone(), vec![InstanceUpdate::input(value)])
                        })
                    })
                    .into_iter()
                    .flatten(),
            )
            .collect();
        Ok(JsonVerdict {
            updates,
            time: self.output_time.to_string(ts),
        })
    }
}
