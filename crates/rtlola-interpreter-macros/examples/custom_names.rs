use rtlola_interpreter_macros::ValueFactory;

#[derive(ValueFactory)]
#[factory(custom_prefix = Custom)]
/// Exposes the struct fields 'a' and 'b' to input streams named Custom_a and Custom_b
/// The field 'c' is exposed as 'Different'
struct TestCustomNames {
    a: usize,
    b: f64,
    #[factory(custom_name = Different)]
    c: String,
    #[factory(ignore)]
    #[allow(dead_code)]
    d: Vec<String>,
}

#[derive(ValueFactory)]
#[factory(prefix)]
/// Exposes the fields of the struct to input streams named Prefixed_a, Prefixed_b and Prefixed_c
struct Prefixed {
    a: usize,
    b: f64,
    c: String,
}

#[derive(ValueFactory)]
struct EmptyStruct {}

#[derive(ValueFactory)]
struct UnitStruct;

#[derive(ValueFactory)]
enum ComplexEnum {
    #[allow(dead_code)]
    UnnamedVariant(usize, String, i32),
    #[allow(dead_code)]
    #[factory(prefix)]
    NamedVariant {
        x: usize,
        #[factory(custom_name = NestedName)]
        y: u64,
        z: String,
    },
    #[allow(dead_code)]
    UnitVariant,
}

fn main() {}
