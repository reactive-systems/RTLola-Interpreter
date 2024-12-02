//! This module contains necessary trait to interface the output of the interpreter with your datastructures.
//! The [VerdictFactory] trait represents a factory for verdicts given a [VerdictRepresentation] of the monitor.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use rtlola_frontend::mir::{OutputReference, Stream, StreamReference};
use rtlola_frontend::RtLolaMir;

use crate::monitor::{Change, Total, TotalIncremental, VerdictRepresentation};
use crate::time::{OutputTimeRepresentation, TimeConversion};
use crate::{Value, ValueConvertError};

/// Extends the [VerdictFactory] trait with a new method.
pub trait NewVerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation>:
    VerdictFactory<MonitorOutput, OutputTime> + Sized
{
    /// A custom data type supplied when creating the factory.
    type CreationData;
    /// A custom error type returned on a failure during creation of the factory.
    type CreationError;

    /// Creates a new Verdict Factory from the MIR.
    fn new(ir: &RtLolaMir, data: Self::CreationData) -> Result<Self, Self::CreationError>;
}

/// This trait provides the functionally to convert the monitor output.
/// You can either implement this trait for your own datatype or use one of the predefined output methods from the `rtlola-io-plugins` crate.
pub trait VerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    /// Type of the expected Output representation.
    type Verdict;

    /// Error when converting the monitor output to the verdict.
    type Error: Error + 'static;

    /// This function converts a monitor to a verdict.
    fn get_verdict(&mut self, rec: MonitorOutput, ts: OutputTime::InnerTime) -> Result<Self::Verdict, Self::Error>;
}

/// A trait to annotate Self with an [VerdictFactory] that outputs Self as a Verdict.
pub trait AssociatedVerdictFactory<MonitorOutput: VerdictRepresentation, OutputTime: OutputTimeRepresentation> {
    /// The associated factory.
    type Factory: NewVerdictFactory<MonitorOutput, OutputTime>;
}

/// Represents the state of a stream in a Verdict.
/// Used by the [FromValues] Trait.
#[derive(Debug, Clone)]
pub enum StreamValue {
    /// The state of an unparametrized stream.
    Stream(Option<Value>),
    /// The state of a parametrized stream.
    Instances(HashMap<Vec<Value>, Value>),
}

/// Represents the capability of a type to be constructed from a vector of [StreamValue]s.
pub trait FromValues: Sized {
    /// The type representing the timestamp of teh values.
    type OutputTime;

    /// Returns a vector of stream names that are required for constructing `Self`.
    fn streams() -> Vec<String>;

    /// Tries to construct `Self` from a vector of [StreamValue]s and a timestamp.
    /// The stream values are in the same order as the names returned by `Self::streams()`.
    fn construct(ts: Self::OutputTime, data: Vec<StreamValue>) -> Result<Self, FromValuesError>;
}

impl<V, ExpectedTime, MonitorTime> AssociatedVerdictFactory<Total, MonitorTime> for V
where
    V: FromValues<OutputTime = ExpectedTime>,
    MonitorTime: TimeConversion<ExpectedTime>,
{
    type Factory = StructVerdictFactory<V>;
}

impl<V, ExpectedTime, MonitorTime> AssociatedVerdictFactory<TotalIncremental, MonitorTime> for V
where
    V: FromValues<OutputTime = ExpectedTime>,
    MonitorTime: TimeConversion<ExpectedTime>,
{
    type Factory = StructVerdictFactory<V>;
}

/// Represents the errors that can occur when constructing an arbitraty type from a vector of [StreamValue]s.
#[derive(Debug)]
pub enum FromValuesError {
    /// The StreamValue can not be converted to desired Rust type.
    ValueConversion(ValueConvertError),
    /// A Non-Optional value was expected but None was given as a [StreamValue].
    ExpectedValue {
        /// The name of the stream for which a value was expected.
        stream_name: String,
    },
    /// The stream instance hashmap can not be converted to the desired Rust HashMap.
    InvalidHashMap {
        /// The name of the stream for which the parameters did not match.
        stream_name: String,
        /// The number of parameters expected by the implementation.
        expected_num_params: usize,
        /// The number of parameters as defined in the specification.
        got_number_params: usize,
    },
    /// A parameterized stream was expected but a non-parametrized value was received, or vice versa.
    StreamKindMismatch,
}

impl Display for FromValuesError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FromValuesError::ValueConversion(v) => write!(f, "{}", v),
            FromValuesError::ExpectedValue { stream_name } => {
                write!(
                    f,
                    "The value for stream {} was expected to exist but was not present in the monitor verdict.",
                    stream_name
                )
            },
            FromValuesError::InvalidHashMap {
                stream_name,
                expected_num_params,
                got_number_params,
            } => {
                write!(
                    f,
                    "Mismatch in the number of parameters of stream {}\nExpected {} parameters, but got {}",
                    stream_name, expected_num_params, got_number_params
                )
            },
            FromValuesError::StreamKindMismatch => {
                write!(
                    f,
                    "Expected a parameterized stream but got a non-parameterized stream or vice-versa."
                )
            },
        }
    }
}

impl Error for FromValuesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FromValuesError::ValueConversion(e) => Some(e),
            FromValuesError::ExpectedValue { .. } => None,
            FromValuesError::InvalidHashMap { .. } => None,
            FromValuesError::StreamKindMismatch => None,
        }
    }
}

impl From<ValueConvertError> for FromValuesError {
    fn from(value: ValueConvertError) -> Self {
        Self::ValueConversion(value)
    }
}

/// Captures the errors that might occure when constructing a struct that implements [FromValues].
#[derive(Debug)]
pub enum StructVerdictError {
    /// The `FromValues::streams()` method returned a stream that does not exist in the specification.
    UnknownStream(String),
    /// An error occurred when converting the stream state to a rust datatype.
    ValueError(FromValuesError),
}
impl Display for StructVerdictError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StructVerdictError::UnknownStream(field) => write!(f, "No stream found for struct field: {}", field),
            StructVerdictError::ValueError(ve) => write!(f, "{}", ve),
        }
    }
}

impl Error for StructVerdictError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StructVerdictError::UnknownStream(_) => None,
            StructVerdictError::ValueError(ve) => Some(ve),
        }
    }
}

impl From<FromValuesError> for StructVerdictError {
    fn from(value: FromValuesError) -> Self {
        Self::ValueError(value)
    }
}

/// A [VerdictFactory] to construct types that implement `FromValues` from a [Total] or [TotalIncremental] monitor verdict.
#[derive(Debug, Clone)]
pub struct StructVerdictFactory<V: FromValues> {
    // 'maps' a vector position to a stream reference.
    map: Vec<StreamReference>,
    // Maps a stream reference to a vector position.
    map_inv: HashMap<StreamReference, usize>,
    parameterized_streams: HashSet<OutputReference>,
    inner: PhantomData<V>,
}

impl<V: FromValues> StructVerdictFactory<V> {
    /// Creates a new [StructVerdictFactory] given an [RtLolaMir].
    pub fn new(ir: &RtLolaMir) -> Result<Self, StructVerdictError> {
        let map: Vec<StreamReference> = V::streams()
            .iter()
            .map(|name| {
                ir.get_stream_by_name(name)
                    .map(|s| s.as_stream_ref())
                    .or_else(|| {
                        name.starts_with("trigger_")
                            .then(|| name.split_once('_'))
                            .flatten()
                            .and_then(|(_, trg_idx)| trg_idx.parse::<usize>().ok())
                            .and_then(|trg_idx| ir.triggers.get(trg_idx).map(|trg| trg.output_reference))
                    })
                    .ok_or_else(|| StructVerdictError::UnknownStream(name.to_string()))
            })
            .collect::<Result<_, _>>()?;
        let map_inv = map.iter().enumerate().map(|(idx, sr)| (*sr, idx)).collect();
        let parameterized_streams = ir
            .outputs
            .iter()
            .filter(|os| os.is_parameterized())
            .map(|o| o.reference.out_ix())
            .collect();
        Ok(Self {
            map,
            map_inv,
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

impl<O, I, V> NewVerdictFactory<Total, O> for StructVerdictFactory<V>
where
    V: FromValues<OutputTime = I>,
    O: OutputTimeRepresentation + TimeConversion<I>,
{
    type CreationData = ();
    type CreationError = StructVerdictError;

    fn new(ir: &RtLolaMir, _data: Self::CreationData) -> Result<Self, Self::Error> {
        Self::new(ir)
    }
}

impl<O, I, V> VerdictFactory<TotalIncremental, O> for StructVerdictFactory<V>
where
    V: FromValues<OutputTime = I>,
    O: OutputTimeRepresentation + TimeConversion<I>,
{
    type Error = StructVerdictError;
    type Verdict = V;

    fn get_verdict(&mut self, rec: TotalIncremental, ts: O::InnerTime) -> Result<Self::Verdict, Self::Error> {
        let mut values: Vec<StreamValue> = self
            .map
            .iter()
            .map(|sr| {
                if sr.is_output() && self.parameterized_streams.contains(&sr.out_ix()) {
                    StreamValue::Instances(HashMap::new())
                } else {
                    StreamValue::Stream(None)
                }
            })
            .collect();

        for (ir, v) in rec.inputs {
            if let Some(idx) = self.map_inv.get(&StreamReference::In(ir)) {
                values[*idx] = StreamValue::Stream(Some(v));
            }
        }
        for (or, changes) in rec.outputs {
            if let Some(idx) = self.map_inv.get(&StreamReference::Out(or)) {
                if self.parameterized_streams.contains(&or) {
                    let StreamValue::Instances(res) = &mut values[*idx] else {
                        unreachable!("Mapping did not work!");
                    };
                    for change in changes {
                        if let Change::Value(p, v) = change {
                            res.insert(p.unwrap(), v);
                        }
                    }
                } else {
                    let value = changes.into_iter().find_map(|change| {
                        if let Change::Value(_, v) = change {
                            Some(v)
                        } else {
                            None
                        }
                    });
                    values[*idx] = StreamValue::Stream(value);
                }
            }
        }
        let time = O::into(ts);
        Ok(V::construct(time, values)?)
    }
}

impl<O, I, V> NewVerdictFactory<TotalIncremental, O> for StructVerdictFactory<V>
where
    V: FromValues<OutputTime = I>,
    O: OutputTimeRepresentation + TimeConversion<I>,
{
    type CreationData = ();
    type CreationError = StructVerdictError;

    fn new(ir: &RtLolaMir, _data: Self::CreationData) -> Result<Self, Self::Error> {
        Self::new(ir)
    }
}
