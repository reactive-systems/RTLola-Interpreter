mod struct_factory;

use proc_macro::TokenStream;
use proc_macro2::Ident;
use proc_macro_error::abort;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

use crate::value_factory::struct_factory::{expand_named_struct, expand_unnamed_struct};

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(factory))]
struct RecordDeriveAttr {
    #[deluxe(default)]
    prefix: bool,
    custom_prefix: Option<Ident>,
}

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(factory))]
struct RecordItemAttr {
    custom_name: Option<Ident>,
    #[deluxe(default)]
    ignore: bool,
}
pub(crate) fn expand(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let res = match &input.data {
        Data::Struct(s) => {
            match s.fields {
                Fields::Named(_) => expand_named_struct(input),
                Fields::Unnamed(_) => expand_unnamed_struct(input),
                Fields::Unit => todo!(),
            }
        },
        Data::Enum(_) => todo!(),
        Data::Union(_) => abort!(input, "ValueFactory can only be derived for Structs!"),
    };

    proc_macro::TokenStream::from(res)
}
