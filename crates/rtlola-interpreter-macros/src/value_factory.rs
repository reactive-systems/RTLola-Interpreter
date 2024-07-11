mod struct_factory;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

use crate::composit_factory::enum_composer::EnumComposer;
use crate::value_factory::struct_factory::{expand_named_struct, expand_unit_struct, expand_unnamed_struct};
use crate::composit_factory::ComposingDeriver;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let res = match &input.data {
        Data::Struct(s) => {
            match s.fields {
                Fields::Named(_) => expand_named_struct(&input),
                Fields::Unnamed(_) => expand_unnamed_struct(&input),
                Fields::Unit => expand_unit_struct(&input),
            }
        },
        Data::Enum(_) => {
            let sub_trait = quote! {ValueFactory};
            let deriver = match EnumComposer::new(&input, sub_trait, false) {
                Ok(ed) => ed,
                Err(e) => return e.into(),
            };
            deriver.expand(&input)
        },
        Data::Union(_) => abort!(input, "ValueFactory can only be derived for Structs and Enums!"),
    };

    proc_macro::TokenStream::from(res)
}
