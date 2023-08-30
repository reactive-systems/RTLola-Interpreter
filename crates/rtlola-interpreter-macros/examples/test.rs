use std::collections::{HashMap, HashSet};

use rtlola_interpreter::izip;
use rtlola_interpreter::monitor::{DerivedInput, Event, Input, InputError};
use rtlola_interpreter::rtlola_mir::InputReference;
use rtlola_interpreter_macros::{Input, Record};

#[derive(Debug, Clone, Record)]
struct SubEventA {
    a: String,
    c: usize,
    d: f64,
}

#[derive(Debug, Clone, Record)]
struct SubSubEventB {
    x: bool,
    y: usize,
    z: f64,
}

#[derive(Debug, Clone, Input)]
enum SubEventB {
    A(SubEventA),
    B(SubSubEventB),
}

struct TestEvent {
    time: usize,
    a: String,
    b: bool,
    sub_input_a: SubEventA,
    sub_input_b: SubEventB,
}

// Generated Part starts here
#[derive(Debug, Clone, Record)]
struct TestEventChildRecord {
    time: usize,
    a: String,
    b: bool,
}

struct TestEventInput {
    child_record: <TestEventChildRecord as DerivedInput>::Input,
    sub_input_a_input: <SubEventA as DerivedInput>::Input,
    sub_input_b_input: <SubEventB as DerivedInput>::Input,
}
impl Input for TestEventInput {
    type CreationData = ();
    type Error = InputError;
    type Record = TestEvent;

    fn try_new(
        map: HashMap<String, InputReference>,
        setup_data: Self::CreationData,
    ) -> Result<(Self, Vec<String>), InputError> {
        let mut found = HashSet::with_capacity(map.len());
        let (child_record, f) = <TestEventChildRecord as DerivedInput>::Input::try_new(map.clone(), setup_data)?;
        found.extend(f);
        let (sub_input_a_input, f) = <SubEventA as DerivedInput>::Input::try_new(map.clone(), setup_data)?;
        found.extend(f);
        let (sub_input_b_input, f) = <SubEventB as DerivedInput>::Input::try_new(map.clone(), setup_data)?;
        found.extend(f);
        Ok((
            Self {
                child_record,
                sub_input_a_input,
                sub_input_b_input,
            },
            found.into_iter().collect(),
        ))
    }

    fn get_event(&self, rec: Self::Record) -> Result<Event, Self::Error> {
        let Self::Record {
            time,
            a,
            b,
            sub_input_a,
            sub_input_b,
        } = rec;
        let child_record_event: Event = self.child_record.get_event(TestEventChildRecord { time, a, b })?;
        let sub_input_a_event: Event = self.sub_input_a_input.get_event(sub_input_a)?;
        let sub_input_b_event: Event = self.sub_input_b_input.get_event(sub_input_b)?;
        Ok(izip!(child_record_event, sub_input_a_event, sub_input_b_event)
            .map(|(a, b, c)| a.and_then(b).and_then(c))
            .collect())
    }
}

fn main() {}
