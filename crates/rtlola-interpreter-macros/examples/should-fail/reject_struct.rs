use rtlola_interpreter_macros::Record;

#[derive(Record)]
enum RejectEnum {
    A,
    B
}

#[derive(Record)]
struct RejectStruct (String, usize);

fn main(){}