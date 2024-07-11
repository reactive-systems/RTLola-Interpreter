use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

use rtlola_interpreter::monitor::Total;
use rtlola_interpreter::rtlola_frontend::RtLolaMir;
use rtlola_interpreter::rtlola_mir::{OutputReference, Stream, StreamReference};
use rtlola_interpreter::time::{OutputTimeRepresentation, TimeConversion};
use rtlola_interpreter::{Value, ValueConvertError};
use rtlola_interpreter::output::{FromValues, StreamValue, VerdictFactory};
use rtlola_interpreter_macros::FromStreamValues;

#[derive(FromStreamValues)]
struct MyOutputs {
    time: f64,
    a: i64,
    b: String,
    c: Option<bool>,
    d: HashMap<(i64, bool), String>,
}

// impl FromValues for MyOutputs {
//     type OutputTime = f64;
//
//     fn streams() -> &'static [&'static str] {
//         &["a", "b", "c", "d"]
//     }
//
//     fn construct(ts: Self::OutputTime, data: Vec<StreamValue>) -> Result<Self, ValueConvertError> {
//         let [StreamValue::Stream(a), StreamValue::Stream(b), StreamValue::Stream(c), StreamValue::Instances(d)]: [StreamValue; 4] =
//             data.try_into().expect("Mapping to work!")
//         else {
//             panic!("Mapping did not work");
//         };
//
//         // let d = d.into_iter().map(|(params, val)|{
//         //     let [p1, p2]: [Value; 2] = params.try_into().expect("A correct HashMap spec");
//         //     Ok(((p1.try_into()?, p2.try_into()?), val.try_into()?))
//         // }).collect::<Result<_,_>>()?;
//
//         // Ok(MyOutputs {
//         //     time: ts,
//         //     a: init_field!(i64, a),
//         //     b: init_field!(String, b),
//         //     c: init_field!(Option<bool>, c),
//         //     d: init_field!(HashMap<(i64, bool), String>, 2, d)
//         // })
//         todo!()
//     }
// }



fn main() {}
