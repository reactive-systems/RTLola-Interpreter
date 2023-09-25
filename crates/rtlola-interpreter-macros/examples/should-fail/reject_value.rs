use rtlola_interpreter_macros::ValueFactory;
use std::time::Duration;

// TryInto<Value> is not implemented for Duration
#[derive(ValueFactory)]
struct D {
    a: Duration,
    b: usize,
    c: String,
}

fn main(){}