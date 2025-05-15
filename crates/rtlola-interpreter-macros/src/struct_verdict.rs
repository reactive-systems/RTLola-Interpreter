mod expand_named_struct;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

use crate::struct_verdict::expand_named_struct::expand_named_struct;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let res = match &input.data {
        Data::Struct(s) => match s.fields {
            Fields::Named(_) => expand_named_struct(&input),
            Fields::Unnamed(_) | Fields::Unit => {
                abort!(
                    input,
                    "LinearVerdict can only be derived for Structs with named fields!"
                )
            }
        },
        Data::Enum(_) | Data::Union(_) => {
            abort!(input, "LinearVerdict can only be derived for Structs!")
        }
    };

    proc_macro::TokenStream::from(res)
}
