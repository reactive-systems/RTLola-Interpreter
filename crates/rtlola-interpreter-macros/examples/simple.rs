use rtlola_interpreter::monitor::Record;
use rtlola_interpreter_macros::Record;

#[derive(Record)]
struct Test {
    a: usize,
    b: f64,
    c: String,
}

fn main() {}
