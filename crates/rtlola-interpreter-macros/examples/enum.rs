use rtlola_interpreter::input::AssociatedFactory;
use rtlola_interpreter::monitor::Incremental;
use rtlola_interpreter::time::RelativeFloat;
use rtlola_interpreter::{ConfigBuilder, Monitor};
use rtlola_interpreter_macros::{CompositFactory, Record};

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
        should_work: i64,
    },
    E(A, B),
}

fn main() {
    let _monitor: Monitor<_, _, Incremental, _> = ConfigBuilder::new()
        .spec_str(
            "input a: UInt64\n\
                   input b: Float64\n\
                   input c: String\n",
        )
        .offline::<RelativeFloat>()
        .with_event_factory::<<TestEnum as AssociatedFactory>::Factory>()
        .with_verdict::<Incremental>()
        .monitor()
        .expect("Failed to create monitor.");
}
