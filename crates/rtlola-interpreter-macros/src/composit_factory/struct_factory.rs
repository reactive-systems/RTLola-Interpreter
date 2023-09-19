use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Field, Fields};

use crate::composit_factory::{CompositDeriver, FactoryItemAttr};

pub(crate) struct StructDeriver {
    name: Ident,
    fields: Fields,
    included_fields: Vec<Ident>,
    field_names: Vec<Ident>,
    field_types: Vec<TokenStream2>,
}

impl StructDeriver {
    pub(crate) fn new(input: DeriveInput) -> Result<(Self, TokenStream2), TokenStream2> {
        let name = input.ident;
        let mut fields = match input.data {
            Data::Struct(e) => e.fields,
            Data::Enum(_) | Data::Union(_) => unreachable!(),
        };
        if matches!(fields, Fields::Unit) {
            return Err(
                syn::Error::new(fields.span(), "CompositInput cannot be derived for Unit Structs.").to_compile_error(),
            );
        }
        let attributes: Result<Vec<FactoryItemAttr>, _> =
            fields.iter_mut().map(|v| deluxe::extract_attributes(v)).collect();
        let attributes = match attributes {
            Ok(v) => v,
            Err(e) => return Err(e.into_compile_error()),
        };
        let included_fields: Vec<(Field, Ident)> = fields
            .iter()
            .zip(attributes)
            .enumerate()
            .filter_map(|(idx, (f, attr))| {
                if attr.ignore {
                    None
                } else {
                    let ident = f
                        .ident
                        .clone()
                        .unwrap_or_else(|| format_ident!("inner_{}", idx, span = f.span()));
                    Some((f.clone(), ident))
                }
            })
            .collect();

        // Todo: Add shortcut for structs with only one field.
        let (field_names, field_types): (Vec<Ident>, Vec<TokenStream2>) = included_fields
            .iter()
            .map(|(field, ident)| {
                let ty = &field.ty;
                (
                    format_ident!("{}_factory", ident),
                    quote! {<#ty as rtlola_interpreter::input::AssociatedFactory>::Factory},
                )
            })
            .unzip();

        let included_fields: (Vec<Field>, Vec<Ident>) = included_fields.into_iter().unzip();

        Ok((
            Self {
                name,
                fields,
                included_fields: included_fields.1,
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

    fn get_event(&self) -> TokenStream2 {
        let name = &self.name;
        let names = &self.included_fields;
        let deconstructor = match &self.fields {
            Fields::Named(_) => {
                quote! {
                    let #name {#(#names),* , ..} = rec;
                }
            },
            Fields::Unnamed(_) => {
                quote! {
                    let #name (#(#names),* , ..) = rec;
                }
            },
            Fields::Unit => unreachable!(),
        };
        let event: Vec<Ident> = self
            .included_fields
            .iter()
            .map(|f| format_ident!("{}_event", f))
            .collect();
        let factory = &self.field_names;

        quote! {
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                #deconstructor
                #(
                    let #event = self.#factory.get_event(#names)?;
                );*
                Ok(rtlola_interpreter::izip!(#(#event),*).map(|(#(#names),*)| rtlola_interpreter::Value::None #(.and_then(#names))*).collect())
            }
        }
    }
}
