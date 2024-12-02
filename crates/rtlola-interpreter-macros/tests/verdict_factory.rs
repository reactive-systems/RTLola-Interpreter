use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rtlola_interpreter::monitor::{TotalIncremental, Verdicts};
use rtlola_interpreter::output::{
    FromValuesError, StructVerdictError, StructVerdictFactory, VerdictFactory,
};
use rtlola_interpreter::rtlola_frontend::ParserConfig;
use rtlola_interpreter::time::RelativeFloat;
use rtlola_interpreter::ConfigBuilder;
use rtlola_interpreter_macros::{ValueFactory, VerdictFactory};

#[test]
fn complex_types() {
    let spec = "input a: Int64\n\
    input i: UInt64\n\
    output some_stream @a&i := \"Some String Value\"\n\
    output c @1Hz := a.hold(or: 5) > 42\n\
    output d (p1, p2)\n\
        spawn with (a, true)\n\
        eval with \"Parameter 1 is {{}}; Current a is {{}}\".format(p1, a)\n\
    output e (p1)\n\
        spawn with \"Stream a: {{}}\".format(a)\n\
        eval @a&i with p1\n\
    trigger a > 5 \"a is greater 5\"\n\
    trigger (p1, p2)\n\
        spawn with (i > 5, i)\n\
        eval when p1 && i < 42 with \"i is between 5 and 42\"\n\
    trigger (p1, p2)\n\
        spawn with (i > 5, i)\n\
        eval when p1 && i < 42 with \"i is between 5 and 42\"\n\
    ";

    #[derive(Debug, Clone, PartialEq, Default)]
    struct Test {}

    #[derive(Debug, Clone, PartialEq, VerdictFactory)]
    struct MyOutputs {
        // Any field named 'time', 'ts' or 'timestamp' is automatically recognized as the time.
        #[factory(is_time)]
        time_field: f64,
        a: i64,
        #[factory(custom_name=some_stream)]
        b: String,
        // Trigger can be bool to capture their truth value or Option<String> to also get their message.
        trigger_0: bool,
        // Parameterized trigger can be captured by a HashSet or a HashMap analogously.
        trigger_1: HashSet<(bool, u64)>,
        trigger_2: HashMap<(bool, u64), String>,
        c: Option<bool>,
        d: HashMap<(i64, bool), String>,
        e: HashMap<String, String>,
        // Ignored field types must implement Default
        #[factory(ignore)]
        f: Test,
    }

    #[derive(ValueFactory)]
    struct Inputs {
        a: i64,
        i: u64,
    }

    let ir =
        rtlola_interpreter::rtlola_frontend::parse(&ParserConfig::for_string(spec.to_string()))
            .unwrap();
    let factory: &mut dyn VerdictFactory<
        TotalIncremental,
        RelativeFloat,
        Error = StructVerdictError,
        Verdict = MyOutputs,
    > = &mut StructVerdictFactory::<MyOutputs>::new(&ir).unwrap();

    let mut monitor = ConfigBuilder::new()
        .with_ir(ir)
        .offline::<RelativeFloat>()
        .with_mapped_events::<Inputs>()
        .with_verdict::<TotalIncremental>()
        .monitor()
        .unwrap();

    let Verdicts {
        timed: _,
        event,
        ts,
    } = monitor
        .accept_event(Inputs { a: -13, i: 24 }, Duration::from_secs_f64(1.2))
        .unwrap();
    let my_output = factory.get_verdict(event, ts).unwrap();
    assert_eq!(
        my_output,
        MyOutputs {
            time_field: 1.2,
            a: -13,
            b: "Some String Value".to_string(),
            trigger_0: false,
            trigger_1: vec![(true, 24)].into_iter().collect(),
            trigger_2: vec![((true, 24), "i is between 5 and 42".to_string())]
                .into_iter()
                .collect(),
            c: None,
            d: vec![(
                (-13, true),
                "Parameter 1 is -13; Current a is -13".to_string()
            )]
            .into_iter()
            .collect(),
            e: vec![("Stream a: -13".to_string(), "Stream a: -13".to_string())]
                .into_iter()
                .collect(),
            f: Default::default(),
        }
    );
}

#[test]
fn parameter_mismatch() {
    let spec = "input a: Int64\n\
    output d (p1, p2)\n\
        spawn with (a, true)\n\
        eval with \"Parameter 1 is {{}}; Current a is {{}}\".format(p1, a)\n\
    ";

    #[derive(Debug, Clone, PartialEq, VerdictFactory)]
    struct MyOutputs {
        time: f64,
        d: HashMap<i64, String>,
    }

    #[derive(ValueFactory)]
    struct Inputs {
        a: i64,
    }

    let ir =
        rtlola_interpreter::rtlola_frontend::parse(&ParserConfig::for_string(spec.to_string()))
            .unwrap();
    let factory: &mut dyn VerdictFactory<
        TotalIncremental,
        RelativeFloat,
        Error = StructVerdictError,
        Verdict = MyOutputs,
    > = &mut StructVerdictFactory::<MyOutputs>::new(&ir).unwrap();

    let mut monitor = ConfigBuilder::new()
        .with_ir(ir)
        .offline::<RelativeFloat>()
        .with_mapped_events::<Inputs>()
        .with_verdict::<TotalIncremental>()
        .monitor()
        .unwrap();

    let Verdicts {
        timed: _,
        event,
        ts,
    } = monitor
        .accept_event(Inputs { a: -13 }, Duration::from_secs_f64(1.2))
        .unwrap();
    let output_err = factory.get_verdict(event, ts).unwrap_err();
    assert!(matches!(
        output_err,
        StructVerdictError::ValueError(FromValuesError::InvalidHashMap { .. })
    ));
}

#[test]
fn kind_mismatch() {
    let spec = "input a: Int64\n\
    output d (p1, p2)\n\
        spawn with (a, true)\n\
        eval with \"Parameter 1 is {{}}; Current a is {{}}\".format(p1, a)\n\
    ";

    #[derive(Debug, Clone, PartialEq, VerdictFactory)]
    struct MyOutputs {
        time: f64,
        d: String,
    }

    #[derive(ValueFactory)]
    struct Inputs {
        a: i64,
    }

    let ir =
        rtlola_interpreter::rtlola_frontend::parse(&ParserConfig::for_string(spec.to_string()))
            .unwrap();
    let factory: &mut dyn VerdictFactory<
        TotalIncremental,
        RelativeFloat,
        Error = StructVerdictError,
        Verdict = MyOutputs,
    > = &mut StructVerdictFactory::<MyOutputs>::new(&ir).unwrap();

    let mut monitor = ConfigBuilder::new()
        .with_ir(ir)
        .offline::<RelativeFloat>()
        .with_mapped_events::<Inputs>()
        .with_verdict::<TotalIncremental>()
        .monitor()
        .unwrap();

    let Verdicts {
        timed: _,
        event,
        ts,
    } = monitor
        .accept_event(Inputs { a: -13 }, Duration::from_secs_f64(1.2))
        .unwrap();
    let output_err = factory.get_verdict(event, ts).unwrap_err();
    assert!(matches!(
        output_err,
        StructVerdictError::ValueError(FromValuesError::StreamKindMismatch)
    ));
}

#[test]
fn expected_value() {
    let spec = "input a: Int64\n\
        output b @1Hz := a.hold(or: 42)
    ";

    #[derive(Debug, Clone, PartialEq, VerdictFactory)]
    struct MyOutputs {
        time: f64,
        b: String,
    }

    #[derive(ValueFactory)]
    struct Inputs {
        a: i64,
    }

    let ir =
        rtlola_interpreter::rtlola_frontend::parse(&ParserConfig::for_string(spec.to_string()))
            .unwrap();
    let factory: &mut dyn VerdictFactory<
        TotalIncremental,
        RelativeFloat,
        Error = StructVerdictError,
        Verdict = MyOutputs,
    > = &mut StructVerdictFactory::<MyOutputs>::new(&ir).unwrap();

    let mut monitor = ConfigBuilder::new()
        .with_ir(ir)
        .offline::<RelativeFloat>()
        .with_mapped_events::<Inputs>()
        .with_verdict::<TotalIncremental>()
        .monitor()
        .unwrap();

    let Verdicts {
        timed: _,
        event,
        ts,
    } = monitor
        .accept_event(Inputs { a: -13 }, Duration::from_secs_f64(1.2))
        .unwrap();
    let output_err = factory.get_verdict(event, ts).unwrap_err();
    assert!(matches!(
        output_err,
        StructVerdictError::ValueError(FromValuesError::ExpectedValue { .. })
    ));
}

#[test]
fn unkown_stream() {
    let spec = "input a: Int64\n\
        output b @1Hz := a.hold(or: 42)
    ";

    #[derive(Debug, Clone, PartialEq, VerdictFactory)]
    struct MyOutputs {
        time: f64,
        c: String,
    }

    let ir =
        rtlola_interpreter::rtlola_frontend::parse(&ParserConfig::for_string(spec.to_string()))
            .unwrap();
    let err = StructVerdictFactory::<MyOutputs>::new(&ir).unwrap_err();
    assert!(matches!(err, StructVerdictError::UnknownStream(_)));
}
