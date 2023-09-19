use std::collections::HashMap;

use proc_macro2::{Ident, Span, TokenStream as TokenStream2, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{Data, DeriveInput, Fields, Type, Variant};

use crate::composit_factory::{CompositDeriver, FactoryItemAttr};
use crate::helper::new_snake_ident;

pub(crate) struct EnumDeriver {
    name: Ident,
    variants: HashMap<Ident, TokenStream>,
    attributes: Vec<(Variant, FactoryItemAttr)>,
    field_names: Vec<Ident>,
    field_types: Vec<TokenStream2>,
}

impl EnumDeriver {
    pub(crate) fn new(input: DeriveInput) -> Result<(Self, TokenStream2), TokenStream2> {
        let name = input.ident;
        let variants = match input.data {
            Data::Enum(e) => e.variants,
            Data::Struct(_) | Data::Union(_) => unreachable!(),
        };
        let variants: Result<Vec<(Variant, FactoryItemAttr)>, _> = variants
            .into_iter()
            .map(|mut v| deluxe::extract_attributes(&mut v).map(|args| (v, args)))
            .collect();
        let attributes = match variants {
            Ok(v) => v,
            Err(e) => return Err(e.into_compile_error()),
        };

        let mut aux_structs = TokenStream2::new();
        let variants = attributes
            .iter()
            .filter_map(|(v, attr)| {
                if !attr.ignore {
                    let ty = match &v.fields {
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                            Some(fields.unnamed.first().unwrap().ty.to_token_stream())
                        },
                        Fields::Unnamed(fields) => {
                            let ty = format_ident!("{}{}TupleFactory", name, v.ident);
                            let types: Vec<Type> = fields.unnamed.iter().map(|f| f.ty.clone()).collect();
                            aux_structs.extend(quote! {
                                #[derive(CompositFactory)]
                                struct #ty (#(#types),*);
                            });
                            Some(ty.to_token_stream())
                        },
                        Fields::Named(fields) => {
                            let ty = format_ident!("{}{}Record", name, v.ident);
                            let (names, types): (Vec<Ident>, Vec<Type>) = fields
                                .named
                                .iter()
                                .map(|f| (f.ident.as_ref().unwrap().clone(), f.ty.clone()))
                                .unzip();
                            aux_structs.extend(quote! {
                                #[derive(Record)]
                                struct #ty {
                                    #(#names: #types),*
                                }
                            });
                            Some(ty.to_token_stream())
                        },
                        Fields::Unit => {
                            // Ignore Unit variants
                            None
                        },
                    };
                    ty.map(|t| (v.ident.clone(), t))
                } else {
                    None
                }
            })
            .collect::<HashMap<Ident, TokenStream>>();

        let (field_names, field_types) = variants
            .iter()
            .map(|(ident, ty)| {
                (
                    new_snake_ident(ident, "factory"),
                    quote! {<#ty as rtlola_interpreter::input::AssociatedFactory>::Factory},
                )
            })
            .unzip();

        Ok((
            Self {
                name,
                variants,
                attributes,
                field_names,
                field_types,
            },
            aux_structs,
        ))
    }
}

impl CompositDeriver for EnumDeriver {
    fn struct_field_names(&self) -> Vec<Ident> {
        self.field_names.clone()
    }

    fn struct_field_types(&self) -> Vec<TokenStream2> {
        self.field_types.clone()
    }

    fn get_event(&self) -> TokenStream2 {
        let variant_paths: Vec<_> = self
            .variants
            .keys()
            .map(|id| {
                let inner_name = Ident::new("rec", Span::call_site());
                let name = &self.name;
                quote! {#name::#id(#inner_name)}
            })
            .collect();

        let (ignored_variant_paths, ignored_variant_names): (Vec<_>, Vec<String>) = self
            .attributes
            .iter()
            .filter_map(|(var, attr)| {
                let id = &var.ident;
                let name = &self.name;
                if attr.ignore {
                    let path = match var.fields {
                        Fields::Named(_) => quote! {#name::#id{..}},
                        Fields::Unnamed(_) => quote! {#name::#id(_)},
                        Fields::Unit => quote! {#name::#id},
                    };
                    Some((path, id.to_string()))
                } else {
                    None
                }
            })
            .unzip();
        let struct_field_names = &self.field_names;

        quote! {
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                match rec {
                    #(
                        #variant_paths => Ok(self.#struct_field_names.get_event(rec)?),
                    )*
                    #(
                        #ignored_variant_paths => Err(rtlola_interpreter::input::EventFactoryError::VariantIgnored(#ignored_variant_names.to_string())),
                    )*
                }
            }
        }
    }
}
