use rtlola_interpreter::input::AssociatedEventFactory;
use rtlola_interpreter::monitor::Incremental;
use rtlola_interpreter::time::RelativeFloat;
use rtlola_interpreter::{ConfigBuilder, Monitor};
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};

#[derive(ValueFactory)]
struct A {
    a: usize,
    b: f64,
}

#[derive(ValueFactory)]
struct B {
    b: f64,
    c: String,
}

#[derive(CompositFactory)]
#[allow(dead_code)]
struct C;

#[derive(CompositFactory)]
#[allow(dead_code)]
enum TestEnum {
    A(A),
    B(B),
    #[factory(ignore)]
    C {
        e: usize,
        d: String,
    },
    D {
        a: A,
        b: B,
    },
    E(A, B),
    F,
}

fn main() {
    let _monitor: Monitor<_, _, Incremental, _> = ConfigBuilder::new()
        .spec_str(
            "input a: UInt64\n\
                   input b: Float64\n\
                   input c: String\n",
        )
        .offline::<RelativeFloat>()
        .with_event_factory::<<TestEnum as AssociatedEventFactory>::Factory>()
        .with_verdict::<Incremental>()
        .monitor()
        .expect("Failed to create monitor.");
}
