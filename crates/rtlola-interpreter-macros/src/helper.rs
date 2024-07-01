use convert_case::{Case, Casing};
use proc_macro2::Ident;
use quote::format_ident;

pub(crate) fn new_snake_ident(base: &Ident, suffix: &str) -> Ident {
    let ident_str = base.to_string().to_case(Case::Snake);
    format_ident!("{}_{}", ident_str, suffix, span = base.span())
}
