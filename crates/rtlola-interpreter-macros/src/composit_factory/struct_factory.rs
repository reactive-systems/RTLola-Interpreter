use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{format_ident, quote, ToTokens};
use syn::{Data, DeriveInput, Fields, Type, Variant};

use crate::composit_factory::{CompositDeriver, FactoryItemAttr};
use crate::helper::new_snake_ident;

pub(crate) struct StructDeriver {
    name: Ident,
    field_names: Vec<Ident>,
    field_types: Vec<TokenStream2>,
}

impl StructDeriver {
    pub(crate) fn new(input: DeriveInput) -> Result<(Self, TokenStream2), TokenStream2> {
        let name = input.ident;
        let fields = match input.data {
            Data::Struct(e) => e.fields,
            Data::Enum(_) | Data::Union(_) => unreachable!(),
        };
        let attributes: Result<Vec<FactoryItemAttr>, _> = fields.iter().map(|v| deluxe::parse_attributes(v)).collect();
        let attributes = match attributes {
            Ok(v) => v,
            Err(e) => return Err(e.into_compile_error()),
        };

        // Todo: Add shortcut for structs with only one field.
        let (field_names, field_types): (Vec<Ident>, Vec<TokenStream2>) = fields
            .iter()
            .zip(attributes)
            .enumerate()
            .filter_map(|(idx, (field, attribute))| {
                if (attribute.ignore) {
                    None
                } else {
                    let ident = field
                        .ident
                        .unwrap_or_else(|| format_ident!("inner_{}", idx, span = field.span()));
                    let ty = &field.ty;
                    Some((
                        format_ident!("{}_factory", ident),
                        quote! {<#ty as rtlola_interpreter::input::AssociatedFactory>::Factory},
                    ))
                }
            })
            .unzip();

        Ok((
            Self {
                name,
                field_names,
                field_types,
            },
            TokenStream2::new(),
        ))
    }
}

impl CompositDeriver for StructDeriver {
    fn struct_field_names(&self) -> Vec<Ident> {
        self.field_names.clone()
    }

    fn struct_field_types(&self) -> Vec<TokenStream2> {
        self.field_types.clone()
    }

    fn get_event(&self) -> TokenStream2 {}
}
