pub(crate) mod helper;

mod composit_factory;
mod enum_composer;
mod value_factory;

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use proc_macro_error::proc_macro_error;
use quote::{format_ident, quote};
use syn::DeriveInput;

use crate::composit_factory::expand as composit_expand;
use crate::value_factory::expand as value_expand;

pub(crate) trait ComposingDeriver {
    fn struct_field_names(&self) -> Vec<Ident>;
    fn struct_field_types(&self) -> Vec<TokenStream2>;

    fn get_event(&self) -> TokenStream2;

    fn aux_code(&self) -> TokenStream2 {
        TokenStream2::new()
    }
    fn expand(&self, input: &DeriveInput) -> TokenStream2 {
        let DeriveInput {
            ident: name,
            generics,
            vis,
            ..
        } = input;

        let struct_name = format_ident!("{name}Factory");
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let init_code = self.aux_code();
        let field_names = self.struct_field_names();
        let field_types = self.struct_field_types();
        let get_event = self.get_event();

        quote! {
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
        }
    }
}

#[derive(deluxe::ParseAttributes, Debug, Clone)]
#[deluxe(attributes(factory))]
pub(crate) struct FactoryAttr {
    #[deluxe(default)]
    ignore: bool,
    #[deluxe(default)]
    prefix: bool,
    custom_prefix: Option<Ident>,
    custom_name: Option<Ident>,
}

#[proc_macro_derive(ValueFactory, attributes(factory))]
#[proc_macro_error]
/// A derive macro that implements the [EventFactory](rtlola_interpreter::input::EventFactory) trait for a type which is composed of types that implement `TryInto<Value>`.
/// For an example look at `simple.rs` and `custom_names.rs` in the example folder.
pub fn derive_record_impl(input: TokenStream) -> TokenStream {
    value_expand(input)
}

#[proc_macro_derive(CompositFactory, attributes(factory))]
#[proc_macro_error]
/// A derive macro that implements the [EventFactory](rtlola_interpreter::input::EventFactory) trait for a type which is composed of types that implement [AssociateFactor](rtlola_interpreter::input::AssociatedFactory).
/// For an example look at `enum.rs` in the example folder.
pub fn derive_input_impl(input: TokenStream) -> TokenStream {
    composit_expand(input)
}
