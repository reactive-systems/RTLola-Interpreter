//! This module contains necessary trait to interface the output of the interpreter with your datastructures.
//! The [VerdictFactory] trait represents a factory for verdicts given a [VerdictRepresentation] of the monitor.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use rtlola_frontend::mir::{OutputReference, Stream, StreamReference};
use rtlola_frontend::RtLolaMir;

use crate::monitor::{Total, VerdictRepresentation};
use crate::time::{OutputTimeRepresentation, TimeConversion};
use crate::{Value, ValueConvertError};

/// This trait provides the functionally to convert the monitor output.
/// You can either implement this trait for your own datatype or use one of the predefined output methods.
/// See [VerdictRepresentationFactory]
pub trait VerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    /// Type of the expected Output representation
    type Verdict;
    /// Error when converting the monitor output to the verdict
    type Error: Error + 'static;

    /// This function converts a monitor to a verdict.
    fn get_verdict(&mut self, rec: MonitorOutput, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error>;
}

#[derive(Debug, Clone)]
pub enum StreamValue {
    Stream(Option<Value>),
    Instances(HashMap<Vec<Value>, Value>),
}
pub trait FromValues: Sized {
    type OutputTime;
    fn streams() -> Vec<String>;

    fn construct(ts: Self::OutputTime, data: Vec<StreamValue>) -> Result<Self, ValueConvertError>;
}

#[derive(Debug)]
pub enum StructVerdictError {
    UnknownField(String),
    ValueError(ValueConvertError),
}
impl Display for StructVerdictError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StructVerdictError::UnknownField(field) => write!(f, "No stream found for struct field: {}", field),
            StructVerdictError::ValueError(ve) => write!(f, "{}", ve),
        }
    }
}

impl Error for StructVerdictError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StructVerdictError::UnknownField(_) => None,
            StructVerdictError::ValueError(ve) => Some(ve),
        }
    }
}

impl From<ValueConvertError> for StructVerdictError {
    fn from(value: ValueConvertError) -> Self {
        Self::ValueError(value)
    }
}

#[derive(Debug, Clone)]
pub struct StructVerdictFactory<V: FromValues> {
    map: Vec<StreamReference>,
    parameterized_streams: HashSet<OutputReference>,
    inner: PhantomData<V>,
}

impl<V: FromValues> StructVerdictFactory<V> {
    pub fn new(ir: &RtLolaMir) -> Result<Self, StructVerdictError> {
        let map = V::streams()
            .iter()
            .map(|name| {
                ir.inputs
                    .iter()
                    .find(|is| &is.name == name)
                    .map(|is| is.reference)
                    .or_else(|| ir.outputs.iter().find(|os| &os.name == name).map(|os| os.reference))
                    .or_else(|| {
                        name.starts_with("trigger_")
                            .then(|| name.split_once('_'))
                            .flatten()
                            .and_then(|(_, trg_idx)| trg_idx.parse::<usize>().ok())
                            .and_then(|trg_idx| ir.triggers.get(trg_idx).map(|trg| trg.output_reference))
                    })
                    .ok_or_else(|| StructVerdictError::UnknownField(name.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let parameterized_streams = ir
            .outputs
            .iter()
            .filter(|os| os.is_parameterized())
            .map(|o| o.reference.out_ix())
            .collect();
        Ok(Self {
            map,
            parameterized_streams,
            inner: Default::default(),
        })
    }
}

impl<O, I, V> VerdictFactory<Total, O> for StructVerdictFactory<V>
where
    V: FromValues<OutputTime = I>,
    O: OutputTimeRepresentation + TimeConversion<I>,
{
    type Error = StructVerdictError;
    type Verdict = V;

    fn get_verdict(&mut self, rec: Total, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        let values: Vec<StreamValue> = self
            .map
            .iter()
            .map(|sr| {
                match sr {
                    StreamReference::In(i) => StreamValue::Stream(rec.inputs[*i].clone()),
                    StreamReference::Out(o) if !self.parameterized_streams.contains(o) => {
                        StreamValue::Stream(rec.outputs[*o][0].1.clone())
                    },
                    StreamReference::Out(o) => {
                        StreamValue::Instances(
                            rec.outputs[*o]
                                .iter()
                                .filter(|(_, value)| value.is_some())
                                .map(|(inst, val)| (inst.clone().unwrap(), val.clone().unwrap()))
                                .collect(),
                        )
                    },
                }
            })
            .collect();
        let time = O::into(ts);
        Ok(V::construct(time, values)?)
    }
}
