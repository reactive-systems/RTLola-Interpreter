use rtlola_interpreter_macros::ValueFactory;

#[derive(ValueFactory)]
#[allow(dead_code)]
/// Exposes the fields of the struct to input streams named a, b and c
struct Test {
    a: usize,
    b: f64,
    c: String,
}

fn main() {}
