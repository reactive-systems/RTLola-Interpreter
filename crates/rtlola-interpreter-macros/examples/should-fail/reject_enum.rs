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

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum {
    A(A, B),
    B(B),
}

struct C {}

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum2 {
    A(A),
    B(B),
    C(C),
}

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum3 {
    A(A),
    B(B),
    C{
        a: A,
        b: B,
    },
}

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum4 {
    A(A),
    B(B),
    C,
}

fn main() {}
