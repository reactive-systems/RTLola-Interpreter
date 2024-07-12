use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, ToTokens};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};

use crate::composit_factory::ComposingDeriver;
use crate::FactoryAttr;

pub(crate) struct StructDeriver {
    name: Ident,
    fields: Fields,
    included_fields: Vec<Ident>,
    field_names: Vec<Ident>,
    field_types: Vec<TokenStream2>,
}

impl StructDeriver {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, TokenStream2> {
        let name = input.ident.clone();
        let fields = match &input.data {
            Data::Struct(e) => &e.fields,
            Data::Enum(_) | Data::Union(_) => unreachable!(),
        };

        let attributes: Result<Vec<FactoryAttr>, _> = fields.iter().map(deluxe::parse_attributes).collect();
        let attributes = match attributes {
            Ok(v) => v,
            Err(e) => return Err(e.into_compile_error()),
        };
        let mut included_fields: Vec<(TokenStream2, Ident)> = fields
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
                    Some((f.ty.to_token_stream(), ident))
                }
            })
            .collect();

        if included_fields.is_empty() {
            included_fields.push((
                quote! {rtlola_interpreter::input::NoEvent},
                Ident::new("default", Span::call_site()),
            ))
        }

        let (field_names, field_types): (Vec<Ident>, Vec<TokenStream2>) = included_fields
            .iter()
            .map(|(ty, ident)| {
                (
                    format_ident!("{}_factory", ident),
                    quote! {<#ty as rtlola_interpreter::input::AssociatedFactory>::Factory},
                )
            })
            .unzip();

        let included_fields: (Vec<TokenStream2>, Vec<Ident>) = included_fields.into_iter().unzip();

        Ok(Self {
            name,
            fields: fields.clone(),
            included_fields: included_fields.1,
            field_names,
            field_types,
        })
    }
}

impl ComposingDeriver for StructDeriver {
    fn struct_field_names(&self) -> Vec<Ident> {
        self.field_names.clone()
    }

    fn struct_field_types(&self) -> Vec<TokenStream2> {
        self.field_types.clone()
    }

    fn get_event(&self) -> TokenStream2 {
        let name = &self.name;
        let names = &self.included_fields;
        let event: Vec<Ident> = self
            .included_fields
            .iter()
            .map(|f| format_ident!("{}_event", f))
            .collect();
        let factory = &self.field_names;

        let (deconstructor, event_generator): (TokenStream2, TokenStream2) = if !self.fields.is_empty() {
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
                Fields::Unit => TokenStream2::new(),
            };
            (
                deconstructor,
                quote! {
                    #(
                        let #event = self.#factory.get_event(#names)?;
                    );*
                },
            )
        } else {
            (
                TokenStream2::new(),
                quote! {
                    #(
                        let #event = self.#factory.get_event(rtlola_interpreter::input::NoEvent)?;
                    );*
                },
            )
        };

        quote! {
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                #deconstructor
                #event_generator
                Ok(rtlola_interpreter::izip!(#(#event),*).map(|(#(#names),*)| rtlola_interpreter::Value::None #(.and_then(#names))*).collect())
            }
        }
    }
}
