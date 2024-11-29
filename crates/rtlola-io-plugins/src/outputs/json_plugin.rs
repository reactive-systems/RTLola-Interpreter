//! Module that implements [VerdictsSink] to represent the verdicts in JSONL format.
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::io::Write;
use std::marker::PhantomData;

use jsonl::WriteError;
use rtlola_interpreter::monitor::{Change, TotalIncremental, VerdictRepresentation};
use rtlola_interpreter::output::NewVerdictFactory;
use rtlola_interpreter::rtlola_mir::{OutputReference, RtLolaMir, StreamReference};
use rtlola_interpreter::time::OutputTimeRepresentation;
use rtlola_interpreter::Value;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::value::Value as JsonValue;

use super::{VerdictFactory, VerdictsSink};

/// Print the verdicts in JSONL format to the writer
#[derive(Debug)]
pub struct JsonSink<
    Factory: VerdictFactory<MonitorOutput, OutputTime>,
    MonitorOutput: VerdictRepresentation,
    OutputTime: OutputTimeRepresentation,
    W: Write,
> {
    writer: W,
    factory: Factory,
    verdict: PhantomData<MonitorOutput>,
    output_time: PhantomData<OutputTime>,
}

impl<
        Factory: VerdictFactory<MonitorOutput, OutputTime>,
        MonitorOutput: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
        W: Write,
    > JsonSink<Factory, MonitorOutput, OutputTime, W>
{
    /// Construct a new JsonSink with a factory produing Serializeable verdicts
    pub fn new(factory: Factory, writer: W) -> Self {
        Self {
            writer,
            factory,
            verdict: PhantomData,
            output_time: PhantomData,
        }
    }
}

impl<
        Factory: VerdictFactory<MonitorOutput, OutputTime, Verdict = Option<FactoryVerdict>>,
        MonitorOutput: VerdictRepresentation,
        OutputTime: OutputTimeRepresentation,
        W: Write,
        FactoryVerdict: Serialize,
    > VerdictsSink<MonitorOutput, OutputTime> for JsonSink<Factory, MonitorOutput, OutputTime, W>
{
    type Error = WriteError;
    type Factory = Factory;
    type Return = ();

    fn sink(&mut self, verdict: Option<FactoryVerdict>) -> Result<Self::Return, Self::Error> {
        if let Some(verdict) = verdict {
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
    /// only print the values of the trigger
    Trigger,
    /// print the values of the outputs (including trigger)
    Outputs,
    /// print the values of all streams (including inputs)
    Streams,
    /// also print the spawn and close of streams
    Debug,
}

impl JsonVerbosity {
    fn include_inputs(&self) -> bool {
        self >= &JsonVerbosity::Streams
    }

    fn include_non_trigger_outputs(&self) -> bool {
        self >= &JsonVerbosity::Outputs
    }

    fn include_triggers(&self) -> bool {
        true
    }

    fn include_spawn_and_close(&self) -> bool {
        self >= &JsonVerbosity::Debug
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
    /// Construct a new factory for the given specification that writes to the supplied writer
    pub fn new(ir: &RtLolaMir, verbosity: JsonVerbosity) -> Self {
        let stream_names = ir
            .all_streams()
            .map(|stream| (stream, ir.stream(stream).name().to_owned()))
            .collect();
        let triggers = ir
            .triggers
            .iter()
            .map(|trigger| trigger.output_reference.out_ix())
            .collect();
        Self {
            stream_names,
            output_time: Default::default(),
            triggers,
            verbosity,
        }
    }

    /// Turn the json factory into a sink writing to a writer
    pub fn sink<W: Write>(self, writer: W) -> JsonSink<Self, TotalIncremental, O, W> {
        JsonSink::new(self, writer)
    }

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
#[derive(Serialize, Deserialize, Debug)]
pub struct JsonVerdict {
    /// The timestamp of the verdict (in the output time representation)
    pub time: String,
    #[serde(flatten)]
    /// A mapping of stream name to updates for that stream
    pub updates: HashMap<String, Vec<InstanceUpdate>>,
}

/// The structured representation of the verdict
#[derive(Serialize, Deserialize, Debug)]
pub struct InstanceUpdate {
    /// The instance which is updated.
    /// Won't be serialized for streams that are not parameterized.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pub instance: Vec<JsonValue>,
    /// Whether the instance was spawned during the cycle.
    /// Is only serialized when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    pub spawn: bool,
    /// The new value of that instance.
    /// Is only serialized when the instance was evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub eval: Option<JsonValue>,
    /// Whether the instance was closed during the cycle.
    /// Is only serialized when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[serde(default)]
    pub close: bool,
}

impl InstanceUpdate {
    fn new(instance: &Option<&Vec<Value>>) -> Self {
        Self {
            instance: instance
                .map(|instances| instances.iter().map(json_value).collect())
                .unwrap_or_default(),
            spawn: Default::default(),
            eval: Default::default(),
            close: Default::default(),
        }
    }

    fn input(value: &Value) -> Self {
        Self {
            instance: Vec::new(),
            spawn: Default::default(),
            eval: Some(json_value(value)),
            close: Default::default(),
        }
    }
}

fn json_value(value: &Value) -> JsonValue {
    match value {
        Value::None => JsonValue::Null,
        &Value::Bool(b) => b.into(),
        &Value::Unsigned(n) => n.into(),
        &Value::Signed(n) => n.into(),
        &Value::Float(n) => f64::from(n).into(),
        &Value::Decimal(n) => n.to_f64().unwrap().into(),
        Value::Tuple(tup) => tup.iter().map(json_value).collect::<JsonValue>(),
        Value::Str(s) => s.to_string().into(),
        Value::Bytes(s) => s.iter().copied().collect::<JsonValue>(),
    }
}

impl<O: OutputTimeRepresentation> VerdictFactory<TotalIncremental, O> for JsonFactory<O> {
    type Error = Infallible;
    type Verdict = Option<JsonVerdict>;

    fn get_verdict(&mut self, rec: TotalIncremental, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        let updates: HashMap<_, _> = rec
            .outputs
            .iter()
            .filter(|(s, _)| self.include_stream(StreamReference::Out(*s)))
            .flat_map(|(stream, changes)| {
                let stream_name = &self.stream_names[&StreamReference::Out(*stream)];
                let mut instances: HashMap<Option<&Vec<Value>>, InstanceUpdate> = HashMap::new();
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
                        Change::Spawn(_) => {
                            entry.spawn = true;
                        },
                        Change::Value(_, v) => {
                            entry.eval = Some(json_value(v));
                        },
                        Change::Close(_) => {
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
        Ok((!updates.is_empty()).then_some(JsonVerdict {
            updates,
            time: self.output_time.to_string(ts),
        }))
    }
}

impl<O: OutputTimeRepresentation> NewVerdictFactory<TotalIncremental, O> for JsonFactory<O> {
    type CreationData = JsonVerbosity;

    fn new(ir: &RtLolaMir, data: Self::CreationData) -> Result<Self, Self::Error> {
        Ok(Self::new(ir, data))
    }
}
