mod enum_factory;
mod struct_factory;

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use proc_macro_error::abort;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput};

use crate::composit_factory::enum_factory::EnumDeriver;
use crate::composit_factory::struct_factory::StructDeriver;

trait CompositDeriver {
    fn struct_field_names(&self) -> Vec<Ident>;
    fn struct_field_types(&self) -> Vec<TokenStream2>;
    fn get_event(&self) -> TokenStream2;
}

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(factory))]
pub(crate) struct FactoryItemAttr {
    #[deluxe(default)]
    ignore: bool,
}

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let (deriver, init_code): (Box<dyn CompositDeriver>, TokenStream2) = match &input.data {
        Data::Struct(_) => {
            match StructDeriver::new(input.clone()) {
                Ok((sd, stream)) => (Box::new(sd), stream),
                Err(e) => return e.into(),
            }
        },
        Data::Enum(_) => {
            match EnumDeriver::new(input.clone()) {
                Ok((ed, stream)) => (Box::new(ed), stream),
                Err(e) => return e.into(),
            }
        },
        Data::Union(_) => abort!(&input, "Input can only be derived for structs and enums"),
    };

    let DeriveInput {
        ident: name,
        generics,
        vis,
        ..
    } = input;

    let struct_name = format_ident!("{name}Factory");
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let field_names = deriver.struct_field_names();
    let field_types = deriver.struct_field_types();
    let get_event = deriver.get_event();

    let input_impl = quote! {
        impl #impl_generics rtlola_interpreter::input::AssociatedFactory for #name #ty_generics #where_clause {
            type Factory = #struct_name;
        }

        #init_code

        #vis struct #struct_name #ty_generics #where_clause {
            #(
                #field_names: #field_types
            ),*
        }

        impl #impl_generics rtlola_interpreter::input::EventFactory for #struct_name #ty_generics #where_clause{
            type Record = #name;
            type Error = rtlola_interpreter::input::EventFactoryError;
            type CreationData = ();

            fn try_new(map: std::collections::HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>, setup_data: Self::CreationData) -> Result<(Self, Vec<String>), Self::Error> {
                let mut all_found = std::collections::HashSet::with_capacity(map.len());

                #(
                    let (#field_names, found) = #field_types::try_new(map.clone(), ())?;
                    all_found.extend(found);
                )*

                Ok( (#struct_name{
                    #(
                        #field_names,
                    )*
                }, all_found.into_iter().collect()))
            }

            #get_event
        }
    };
    TokenStream::from(input_impl)
}
