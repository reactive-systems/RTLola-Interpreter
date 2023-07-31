use std::collections::HashMap;

use rtlola_interpreter::monitor::{DerivedInput, Input, RecordError};
use rtlola_interpreter::rtlola_mir::InputReference;
use rtlola_interpreter::Value;
use rtlola_interpreter_macros::{Input, Record};

#[test]
fn simple() {
    #[derive(Record)]
    #[record(prefix)]
    struct Msg1 {
        a: usize,
        b: f64,
    }

    #[derive(Record)]
    #[record(prefix)]
    struct Msg2 {
        c: String,
        d: i64,
    }

    #[derive(Input)]
    enum Rec {
        Var1(Msg1),
        Var2(Msg2),
    }

    let map: HashMap<String, InputReference> = vec![
        ("Msg1_a".to_string(), 0),
        ("Msg1_b".to_string(), 1),
        ("Msg2_c".to_string(), 2),
        ("Msg2_d".to_string(), 3),
    ]
    .into_iter()
    .collect();

    let input = <Rec as DerivedInput>::Input::new(map, ()).unwrap();

    let t1 = Rec::Var1(Msg1 { a: 42, b: 13.37 });
    let expected = vec![
        Value::Unsigned(42),
        Value::try_from(13.37).unwrap(),
        Value::None,
        Value::None,
    ];
    assert_eq!(input.get_event(t1).unwrap(), expected);

    let t2 = Rec::Var2(Msg2 {
        c: "Hello World!".to_string(),
        d: -1337,
    });
    let expected = vec![
        Value::None,
        Value::None,
        Value::Str("Hello World!".to_string().into_boxed_str()),
        Value::Signed(-1337),
    ];
    assert_eq!(input.get_event(t2).unwrap(), expected);
}

#[test]
fn overlap() {
    #[derive(Record)]
    #[record(prefix)]
    struct Msg1 {
        a: usize,
        #[record(custom_name = time)]
        time: f64,
    }

    #[derive(Record)]
    #[record(prefix)]
    struct Msg2 {
        b: i64,
        #[record(custom_name = time)]
        time: f64,
    }

    #[derive(Input)]
    enum Rec {
        Var1(Msg1),
        Var2(Msg2),
    }

    let map: HashMap<String, InputReference> = vec![
        ("Msg1_a".to_string(), 0),
        ("Msg2_b".to_string(), 1),
        ("time".to_string(), 2),
    ]
    .into_iter()
    .collect();

    let input = <Rec as DerivedInput>::Input::new(map, ()).unwrap();

    let t1 = Rec::Var1(Msg1 { a: 42, time: 13.37 });
    let expected = vec![Value::Unsigned(42), Value::None, Value::try_from(13.37).unwrap()];
    assert_eq!(input.get_event(t1).unwrap(), expected);

    let t2 = Rec::Var2(Msg2 { b: -1337, time: 42.42 });
    let expected = vec![Value::None, Value::Signed(-1337), Value::try_from(42.42).unwrap()];
    assert_eq!(input.get_event(t2).unwrap(), expected);
}

#[test]
fn missing() {
    #[derive(Record)]
    #[record(prefix)]
    struct Msg1 {
        a: usize,
        #[record(custom_name = time)]
        time: f64,
    }

    #[derive(Record)]
    #[record(prefix)]
    struct Msg2 {
        b: i64,
        #[record(custom_name = time)]
        time: f64,
    }

    #[derive(Input)]
    #[allow(dead_code)]
    enum Rec {
        Var1(Msg1),
        Var2(Msg2),
    }

    let map: HashMap<String, InputReference> = vec![
        ("Msg1_a".to_string(), 0),
        ("Msg2_b".to_string(), 1),
        ("time".to_string(), 2),
        ("unknown".to_string(), 3),
    ]
    .into_iter()
    .collect();

    let res = <Rec as DerivedInput>::Input::new(map, ());
    let Err(RecordError::InputStreamNotFound(errs)) = res else {
        panic!("Expected error reporting unknown stream!")
    };
    assert_eq!(errs.len(), 1);
    let (name, reasons) = errs.into_iter().next().unwrap();
    assert_eq!(name.as_str(), "unknown");

    for reason in reasons {
        let RecordError::InputStreamUnknown(name) = reason else {
            panic!("Expected stream unknown error from record")
        };
        assert_eq!(name.as_str(), "unknown");
    }
}
