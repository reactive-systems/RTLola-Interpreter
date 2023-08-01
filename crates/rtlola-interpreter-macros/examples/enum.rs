use rtlola_interpreter::monitor::{DerivedInput, Incremental};
use rtlola_interpreter::time::RelativeFloat;
use rtlola_interpreter::{ConfigBuilder, Monitor};
use rtlola_interpreter_macros::{Input, Record};

#[derive(Record)]
struct A {
    a: usize,
    b: f64,
}

#[derive(Record)]
struct B {
    b: f64,
    c: String,
}

#[derive(Input)]
#[allow(dead_code)]
enum TestEnum {
    A(A),
    B(B),
    #[input(ignore)]
    C {
        e: usize,
        d: String,
    },
}

fn main() {
    let _monitor: Monitor<_, _, Incremental, _> = ConfigBuilder::new()
        .spec_str(
            "input a: UInt64\n\
                   input b: Float64\n\
                   input c: String\n",
        )
        .offline::<RelativeFloat>()
        .custom_input::<<TestEnum as DerivedInput>::Input>()
        .with_verdict::<Incremental>()
        .monitor()
        .expect("Failed to create monitor.");
}
