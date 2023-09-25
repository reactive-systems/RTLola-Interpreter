#[test]
fn derive_value_factory() {
    let t = trybuild::TestCases::new();
    t.pass("examples/simple.rs");
    t.pass("examples/custom_names.rs");
    t.compile_fail("examples/should-fail/reject_value.rs");
}

#[test]
fn derive_composit_factory() {
    let t = trybuild::TestCases::new();
    t.pass("examples/enum.rs");
    t.compile_fail("examples/should-fail/reject_composit.rs");
}
