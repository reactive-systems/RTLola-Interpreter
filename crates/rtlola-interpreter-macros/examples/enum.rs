use std::collections::HashMap;
use rtlola_interpreter::monitor::RecordError;
use rtlola_interpreter_macros::{Input, Record};

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

enum TestInput {
    A(A),
    B(B),
}

struct TestInputInput {
    a_record_in: rtlola_interpreter::monitor::RecordInput<A>,
    b_record_in: rtlola_interpreter::monitor::RecordInput<B>,
}

impl rtlola_interpreter::monitor::Input for TestInputInput {
    type CreationData = ();
    type Error = rtlola_interpreter::monitor::RecordError;
    type Record = TestInput;

    fn new(map: HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>, setup_data: Self::CreationData) -> Result<Self, Self::Error> {
        let all_streams = std::collections::HashSet::from_iter(map.keys());

        let (a_record_in, a_errs) = rtlola_interpreter::monitor::RecordInput::new_ignore_undefined(map.clone(), ());
        let a_found = all_streams.difference(&std::collections::HashSet::from_iter(a_errs.keys()));
        let (b_record_in, b_errs) = rtlola_interpreter::monitor::RecordInput::new_ignore_undefined(map, ());
        let b_found = all_streams.difference(&std::collections::HashSet::from_iter(a_errs.keys()));

        let mut all_found = std::collections::HashSet::with_capacity(map.len());

        all_found.extend(a_found);
        all_found.extend(b_found);

        let errs: std::collections::HashMap<String, std::collections::Vec<Self::Error>> = all_streams.difference(&all_found).map(|missing| {
            let mut record_errs = vec![];

            a_errs.get(missing).map(|e| record_errs.push(e));
            b_errs.get(missing).map(|e| record_errs.push(e));

            (missing.clone(), record_errs)
        }).collect();

        if errs.is_empty() {
            Ok(TestInputInput {
                a_record_in,
                b_record_in,
            })
        } else {
            Err(rtlola_interpreter::monitor::RecordError::InputStreamNotFound(errs))
        }
    }

    fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error> {
        match rec {
            TestInput::A(a) => Ok(self.a_record_in.get_event(a)?),
            TestInput::B(b) => Ok(self.b_record_in.get_event(b)?),
        }
    }
}

fn main() {}
