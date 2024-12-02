use std::collections::HashMap;

use rtlola_interpreter::input::{AssociatedEventFactory, EventFactory, EventFactoryError};
use rtlola_interpreter::rtlola_mir::InputReference;
use rtlola_interpreter::Value;
use rtlola_interpreter_macros::{CompositFactory, ValueFactory};

#[test]
fn simple() {
    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg1 {
        a: usize,
        b: f64,
    }

    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg2 {
        c: String,
        d: i64,
    }

    #[derive(CompositFactory)]
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

    let input = <Rec as AssociatedEventFactory>::Factory::new(map, ()).unwrap();

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
fn ignore() {
    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg1 {
        a: usize,
        b: f64,
    }

    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg2 {
        c: String,
        d: i64,
    }

    #[derive(CompositFactory)]
    enum Rec {
        Var1(Msg1),
        Var2(Msg2),
        #[factory(ignore)]
        #[allow(dead_code)]
        Var3,
        #[factory(ignore)]
        #[allow(dead_code)]
        Var4(String),
        #[factory(ignore)]
        Var5 {
            #[allow(dead_code)]
            x: usize,
        },
    }

    let map: HashMap<String, InputReference> = vec![
        ("Msg1_a".to_string(), 0),
        ("Msg1_b".to_string(), 1),
        ("Msg2_c".to_string(), 2),
        ("Msg2_d".to_string(), 3),
    ]
    .into_iter()
    .collect();

    let input = <Rec as AssociatedEventFactory>::Factory::new(map, ()).unwrap();

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

    assert!(matches!(input.get_event(Rec::Var3), Err(EventFactoryError::VariantIgnored(name)) if name == "Var3"));
    assert!(
        matches!(input.get_event(Rec::Var4("Asd".to_string())), Err(EventFactoryError::VariantIgnored(name)) if name == "Var4")
    );
    assert!(
        matches!(input.get_event(Rec::Var5 {x: 5}), Err(EventFactoryError::VariantIgnored(name)) if name == "Var5")
    );
}

#[test]
fn overlap() {
    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg1 {
        a: usize,
        #[factory(custom_name = time)]
        time: f64,
    }

    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg2 {
        b: i64,
        #[factory(custom_name = time)]
        time: f64,
    }

    #[derive(CompositFactory)]
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

    let input = <Rec as AssociatedEventFactory>::Factory::new(map, ()).unwrap();

    let t1 = Rec::Var1(Msg1 { a: 42, time: 13.37 });
    let expected = vec![Value::Unsigned(42), Value::None, Value::try_from(13.37).unwrap()];
    assert_eq!(input.get_event(t1).unwrap(), expected);

    let t2 = Rec::Var2(Msg2 { b: -1337, time: 42.42 });
    let expected = vec![Value::None, Value::Signed(-1337), Value::try_from(42.42).unwrap()];
    assert_eq!(input.get_event(t2).unwrap(), expected);
}

#[test]
fn missing() {
    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg1 {
        a: usize,
        #[factory(custom_name = time)]
        time: f64,
    }

    #[derive(ValueFactory)]
    #[factory(prefix)]
    struct Msg2 {
        b: i64,
        #[factory(custom_name = time)]
        time: f64,
    }

    #[derive(CompositFactory)]
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

    let res = <Rec as AssociatedEventFactory>::Factory::new(map, ());
    let Err(EventFactoryError::InputStreamUnknown(errs)) = res else {
        panic!("Expected error reporting unknown stream!")
    };
    assert_eq!(errs.len(), 1);
    let name = errs.into_iter().next().unwrap();
    assert_eq!(name.as_str(), "unknown");
}
