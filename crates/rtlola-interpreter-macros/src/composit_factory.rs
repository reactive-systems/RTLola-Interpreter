mod struct_factory;

use proc_macro::TokenStream;
use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

use crate::composit_factory::struct_factory::StructDeriver;
use crate::enum_composer::EnumComposer;
use crate::ComposingDeriver;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);
    let sub_trait = quote! {CompositFactory};

    let deriver: Box<dyn ComposingDeriver> = match &input.data {
        Data::Struct(_) => {
            match StructDeriver::new(&input) {
                Ok(sd) => Box::new(sd),
                Err(e) => return e.into(),
            }
        },
        Data::Enum(_) => {
            match EnumComposer::new(&input, sub_trait, true) {
                Ok(ed) => Box::new(ed),
                Err(e) => return e.into(),
            }
        },
        Data::Union(_) => abort!(&input, "Input can only be derived for structs and enums"),
    };

    TokenStream::from(deriver.expand(&input))
}
