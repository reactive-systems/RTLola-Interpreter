use rtlola_interpreter::monitor::{Change, Incremental, Verdicts};
use rtlola_interpreter::time::AbsoluteFloat;
use rtlola_interpreter::Value;

pub(crate) mod input_macros;
pub(crate) mod input_map;

pub(crate) fn create_verdicts() -> Vec<Incremental> {
    let v0: Vec<(usize, _)> = vec![(0, vec![Change::Value(None, 10.0.try_into().unwrap())])];
    let v1: Vec<(usize, _)> = vec![(1, vec![Change::Value(None, Value::Unsigned(3))])];
    vec![v0, v1]
}

pub(crate) fn check_verdict(v: Verdicts<Incremental, AbsoluteFloat>, expected: Incremental) {
    let Verdicts {
        timed,
        event,
        ts: _,
    } = v;
    assert_eq!(event.len(), expected.len());
    assert!(timed.is_empty());
    event
        .into_iter()
        .zip(expected.into_iter())
        .for_each(|((v_ref, v), (e_ref, e))| {
            assert_eq!(v_ref, e_ref);
            assert_eq!(v, e);
        });
}

pub(crate) const SPEC: &'static str =
    "input a : Float64\ninput b: Float64\ninput c: UInt64\noutput d := a + b\noutput e := c + 1";
