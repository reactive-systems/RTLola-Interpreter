#[test]
fn derive() {
    let t = trybuild::TestCases::new();
    t.pass("examples/simple.rs");
    t.compile_fail("examples/should-fail/reject.rs");
}
