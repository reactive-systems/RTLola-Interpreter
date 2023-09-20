use rtlola_interpreter_macros::{CompositFactory, Record};

#[derive(Record)]
struct A {
    a: usize,
    b: f64,
    c: String,
}

#[derive(Record)]
#[record(custom_prefix = Custom)]
struct B {
    a: usize,
    b: f64,
    #[record(custom_name = Different)]
    c: String,
}

struct C {}

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum2 {
    A(A),
    B(B),
    C(C),
}

fn main() {}
