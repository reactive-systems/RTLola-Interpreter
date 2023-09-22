use rtlola_interpreter_macros::ValueFactory;

#[derive(ValueFactory)]
enum RejectEnum {
    A,
    B
}

#[derive(ValueFactory)]
struct RejectStruct (String, usize);

fn main(){}