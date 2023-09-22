use rtlola_interpreter_macros::{CompositFactory, ValueFactory};

#[derive(ValueFactory)]
struct A {
    a: usize,
    b: f64,
    c: String,
}

#[derive(ValueFactory)]
#[factory(custom_prefix = Custom)]
struct B {
    a: usize,
    b: f64,
    #[factory(custom_name = Different)]
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
