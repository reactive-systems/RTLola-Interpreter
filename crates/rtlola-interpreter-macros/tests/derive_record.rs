use rtlola_interpreter::monitor::Record;
use rtlola_interpreter::Value;
use rtlola_interpreter_macros::Record;

#[test]
fn simple() {
    #[derive(Record)]
    struct Test {
        a: usize,
        b: f64,
        c: String,
    }

    let a_getter = Test::func_for_input("a", ()).unwrap();
    let b_getter = Test::func_for_input("b", ()).unwrap();
    let c_getter = Test::func_for_input("c", ()).unwrap();

    let t = Test {
        a: 42,
        b: 13.37,
        c: "Hello World!".to_string(),
    };

    assert_eq!(a_getter(&t).unwrap(), Value::Unsigned(42));
    assert_eq!(b_getter(&t).unwrap(), Value::try_from(13.37).unwrap());
    assert_eq!(
        c_getter(&t).unwrap(),
        Value::Str("Hello World!".to_string().into_boxed_str())
    );
}

#[test]
fn prefix() {
    #[derive(Record)]
    #[record(prefix)]
    struct StructPrefix {
        a: usize,
        b: f64,
        c: String,
    }

    let a_getter = StructPrefix::func_for_input("StructPrefix_a", ()).unwrap();
    let b_getter = StructPrefix::func_for_input("StructPrefix_b", ()).unwrap();
    let c_getter = StructPrefix::func_for_input("StructPrefix_c", ()).unwrap();

    let t = StructPrefix {
        a: 42,
        b: 13.37,
        c: "Hello World!".to_string(),
    };

    assert_eq!(a_getter(&t).unwrap(), Value::Unsigned(42));
    assert_eq!(b_getter(&t).unwrap(), Value::try_from(13.37).unwrap());
    assert_eq!(
        c_getter(&t).unwrap(),
        Value::Str("Hello World!".to_string().into_boxed_str())
    );
}

#[test]
fn custom() {
    #[derive(Record)]
    #[record(custom_prefix = YourAdHere)]
    struct TestCustomNames {
        a: usize,
        b: f64,
        #[record(custom_name = Different)]
        c: String,
    }

    let a_getter = TestCustomNames::func_for_input("YourAdHere_a", ()).unwrap();
    let b_getter = TestCustomNames::func_for_input("YourAdHere_b", ()).unwrap();
    let c_getter = TestCustomNames::func_for_input("Different", ()).unwrap();

    let t = TestCustomNames {
        a: 42,
        b: 13.37,
        c: "Hello World!".to_string(),
    };

    assert_eq!(a_getter(&t).unwrap(), Value::Unsigned(42));
    assert_eq!(b_getter(&t).unwrap(), Value::try_from(13.37).unwrap());
    assert_eq!(
        c_getter(&t).unwrap(),
        Value::Str("Hello World!".to_string().into_boxed_str())
    );
}

#[test]
fn ignore() {
    #[derive(Record)]
    struct TestCustomNames {
        a: usize,
        b: f64,
        #[record(ignore)]
        #[allow(dead_code)]
        c: String,
    }

    assert!(TestCustomNames::func_for_input("a", ()).is_ok());
    assert!(TestCustomNames::func_for_input("b", ()).is_ok());
    assert!(TestCustomNames::func_for_input("c", ()).is_err());
}
