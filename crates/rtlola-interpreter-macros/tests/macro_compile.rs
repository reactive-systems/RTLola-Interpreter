#[test]
fn derive_value_factory() {
    let t = trybuild::TestCases::new();
    t.pass("examples/simple.rs");
    t.pass("examples/custom_names.rs");
}

#[test]
fn derive_composit_factory() {
    let t = trybuild::TestCases::new();
    t.pass("examples/enum.rs");
}

#[test]
fn derive_from_values() {
    let t = trybuild::TestCases::new();
    t.pass("examples/verdict.rs");
}
