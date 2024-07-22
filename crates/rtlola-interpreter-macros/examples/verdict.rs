use std::collections::{HashMap, HashSet};

use rtlola_interpreter_macros::FromStreamValues;

#[derive(FromStreamValues)]
struct MyOutputs {
    time: f64,
    a: i64,
    b: String,
    trigger_1: bool,
    trigger_2: HashSet<(bool, u64)>,
    trigger_3: HashMap<(u64, String), String>,
    c: Option<bool>,
    d: HashMap<(i64, bool), String>,
    e: HashMap<String, String>,
}

fn main() {}
