#[test]
fn derive_record() {
    let t = trybuild::TestCases::new();
    t.pass("examples/simple.rs");
    t.pass("examples/custom_names.rs");
    t.compile_fail("examples/should-fail/reject_struct.rs");
}

#[test]
fn derive_input() {
    let t = trybuild::TestCases::new();
    t.pass("examples/enum.rs");
    t.compile_fail("examples/should-fail/reject_enum.rs");
}
