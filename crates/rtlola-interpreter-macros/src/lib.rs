pub(crate) mod helper;

mod composit_factory;
mod value_factory;

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro_error::proc_macro_error;

use crate::composit_factory::expand as composit_expand;
use crate::value_factory::expand as value_expand;

#[proc_macro_derive(ValueFactory, attributes(factory))]
#[proc_macro_error]
/// A derive macro that implements the [InputMap](rtlola_interpreter::monitor::InputMap) trait for a struct based on its field names.
/// For an example look at `simple.rs` and `custom_names.rs` in the example folder.
pub fn derive_record_impl(input: TokenStream) -> TokenStream {
    value_expand(input)
}

#[proc_macro_derive(CompositFactory, attributes(factory))]
#[proc_macro_error]
/// A derive macro that implements the [EventFactory](rtlola_interpreter::monitor::EventFactory) trait for a type which is composed of types that implement [AssociateFactor](rtlola_interpreter::input::AssociatedFactory).
/// For an example look at `enum.rs` in the example folder.
pub fn derive_input_impl(input: TokenStream) -> TokenStream {
    composit_expand(input)
}
