use rtlola_interpreter::monitor::Record;
use rtlola_interpreter_macros::Record;

#[derive(Record)]
#[record(custom_prefix = test)]
struct TestCustomNames {
    a: usize,
    b: f64,
    #[record(custom_name = Different)]
    c: String,
}

#[derive(Record)]
#[record(prefix)]
struct TestCustomNamesDefault {
    a: usize,
    b: f64,
    c: String,
}

fn main() {}
