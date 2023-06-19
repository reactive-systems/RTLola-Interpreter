#[test]
fn derive_record() {
    let t = trybuild::TestCases::new();
    t.pass("examples/simple.rs");
    t.compile_fail("examples/should-fail/reject.rs");
}
