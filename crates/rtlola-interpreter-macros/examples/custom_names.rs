use rtlola_interpreter_macros::Record;

#[derive(Record)]
#[record(custom_prefix = Custom)]
/// Exposes the struct fields 'a' and 'b' to input streams named Custom_a and Custom_b
/// The field 'c' is exposed as 'Different'
struct TestCustomNames {
    a: usize,
    b: f64,
    #[record(custom_name = Different)]
    c: String,
}

#[derive(Record)]
#[record(prefix)]
/// Exposes the fields of the struct to input streams named Prefixed_a, Prefixed_b and Prefixed_c
struct Prefixed {
    a: usize,
    b: f64,
    c: String,
}

fn main() {}
