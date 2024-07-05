use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;
use std::time::Duration;

use rtlola_interpreter::monitor::{Total, TotalIncremental};
use rtlola_interpreter::rtlola_frontend::RtLolaMir;
use rtlola_interpreter::rtlola_mir::StreamReference;
use rtlola_interpreter::time::RelativeFloat;
use rtlola_interpreter::{Value, ValueConvertError};
use rtlola_io_plugins::VerdictFactory;

struct MyOutputs {
    time: f64,
    a: i64,
    b: String,
    c: bool,
}

trait FromLinearVerdict: Sized {
    fn streams() -> &'static [&'static str];

    fn construct(ts: Duration, data: Vec<Value>) -> Result<Self, ValueConvertError>;
}

impl FromLinearVerdict for MyOutputs {
    fn streams() -> &'static [&'static str] {
        &["a", "b", "c"]
    }

    fn construct(ts: Duration, data: Vec<Value>) -> Result<Self, ValueConvertError> {
        let [a, b, c] = data.try_into().expect("Mapping to work!");
        Ok(MyOutputs {
            time: ts.as_secs_f64(),
            a: a.try_into()?,
            b: b.try_into()?,
            c: c.try_into()?,
        })
    }
}

#[derive(Debug)]
enum StructFactoryError {
    UnknownField(String),
    ValueError(ValueConvertError),
}
impl Display for StructFactoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            StructFactoryError::UnknownField(field) => write!(f, "No stream found for struct field: {}", field),
            StructFactoryError::ValueError(ve) => write!(f, "{}", ve),
        }
    }
}

impl Error for StructFactoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StructFactoryError::UnknownField(_) => None,
            StructFactoryError::ValueError(ve) => Some(ve),
        }
    }
}

impl From<ValueConvertError> for StructFactoryError {
    fn from(value: ValueConvertError) -> Self {
        Self::ValueError(value)
    }
}

struct StructFactory<V: FromLinearVerdict> {
    map: Vec<StreamReference>,
    inner: PhantomData<V>,
}

impl<V: FromLinearVerdict> StructFactory<V> {
    fn new(ir: &RtLolaMir) -> Result<Self, StructFactoryError> {
        V::streams()
            .iter()
            .map(|name| {
                ir.inputs
                    .iter()
                    .find(|is| &is.name == name)
                    .map(|is| is.reference)
                    .or_else(|| ir.outputs.iter().find(|os| &os.name == name).map(|os| os.reference))
                    .ok_or_else(|| StructFactoryError::UnknownField(name.to_string()))
            })
            .collect::<Result<_, _>>()
            .map(|map| {
                Self {
                    map,
                    inner: Default::default(),
                }
            })
    }
}

impl<V: FromLinearVerdict> VerdictFactory<Total, RelativeFloat> for StructFactory<V> {
    type Error = StructFactoryError;
    type Verdict = V;

    fn get_verdict(&mut self, rec: Total, ts: Duration) -> Result<Self::Verdict, Self::Error> {
        let values: Vec<Value> = self
            .map
            .iter()
            .map(|sr| {
                match sr {
                    StreamReference::In(i) => rec.inputs[*i].clone().unwrap(),
                    StreamReference::Out(o) => rec.outputs[*o][0].1.clone().unwrap(),
                }
            })
            .collect();
        Ok(V::construct(ts, values)?)
    }
}

fn main() {}
