pub(crate) mod helper;

mod composit_factory;
mod struct_verdict;
mod value_factory;

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Ident;
use proc_macro_error::proc_macro_error;

use crate::composit_factory::expand as composit_expand;
use crate::struct_verdict::expand as struct_verdict_expand;
use crate::value_factory::expand as value_expand;

#[derive(deluxe::ParseAttributes, Debug, Clone)]
#[deluxe(attributes(factory))]
pub(crate) struct FactoryAttr {
    #[deluxe(default)]
    ignore: bool,
    #[deluxe(default)]
    prefix: bool,
    #[deluxe(default)]
    is_time: bool,
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

#[proc_macro_derive(FromStreamValues, attributes(factory))]
#[proc_macro_error]
pub fn derive_struct_verdict(input: TokenStream) -> TokenStream {
    struct_verdict_expand(input)
}
