use proc_macro2::{Ident, TokenStream as TokenStream2, TokenStream};
use quote::__private::ext::RepToTokensExt;
use quote::{format_ident, quote, ToTokens};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Type, Variant};

use crate::composit_factory::{CompositDeriver, FactoryItemAttr};
use crate::helper::new_snake_ident;

pub(crate) struct EnumDeriver {
    name: Ident,
    attributes: Vec<(Variant, FactoryItemAttr)>,
    field_names: Vec<Ident>,
    field_types: Vec<TokenStream2>,
}

fn field_name(name: &Ident) -> Ident {
    new_snake_ident(name, "factory")
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
                    let v_ident = &v.ident;
                    let ty = match &v.fields {
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                            Some(fields.unnamed.first().unwrap().ty.to_token_stream())
                        },
                        Fields::Unnamed(fields) => {
                            let ty = format_ident!("{}{}Tuple", name, v.ident);
                            let (names, types): (Vec<Ident>, Vec<Type>) = fields
                                .unnamed
                                .iter()
                                .enumerate()
                                .map(|(idx, f)| (format_ident!("inner_{}", idx, span = f.span()), f.ty.clone()))
                                .unzip();
                            aux_structs.extend(quote! {
                                #[derive(CompositFactory)]
                                struct #ty (#(#types),*);

                                impl From<#name> for #ty {
                                    fn from(value: #name) -> Self {
                                        if let #name::#v_ident(#(#names),*) = value {
                                            Self(#(#names),*)
                                        } else {
                                            panic!("Tried to construct helper struct from wrong variant.")
                                        }
                                    }
                                }
                            });
                            Some(ty.to_token_stream())
                        },
                        Fields::Named(fields) => {
                            let ty = format_ident!("{}{}Struct", name, v.ident);
                            let (names, types): (Vec<Ident>, Vec<Type>) = fields
                                .named
                                .iter()
                                .map(|f| (f.ident.as_ref().unwrap().clone(), f.ty.clone()))
                                .unzip();
                            aux_structs.extend(quote! {
                                #[derive(CompositFactory)]
                                struct #ty {
                                    #(#names: #types),*
                                }

                                impl From<#name> for #ty {
                                    fn from(value: #name) -> Self {
                                        if let #name::#v_ident{#(#names),*} = value {
                                            Self{#(#names),*}
                                        } else {
                                            panic!("Tried to construct helper struct from wrong variant.")
                                        }
                                    }
                                }
                            });
                            Some(ty.to_token_stream())
                        },
                        Fields::Unit => Some(quote! {()}),
                    };
                    ty.map(|t| (v_ident.clone(), t))
                } else {
                    None
                }
            })
            .collect::<Vec<(Ident, TokenStream)>>();

        let (field_names, field_types) = variants
            .iter()
            .map(|(ident, ty)| {
                (
                    field_name(ident),
                    quote! {<#ty as rtlola_interpreter::input::AssociatedFactory>::Factory},
                )
            })
            .unzip();

        Ok((
            Self {
                name,
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
        let name = &self.name;

        let match_lines: Vec<TokenStream2> = self
            .attributes
            .iter()
            .map(|(var, attr)| {
                let id = &var.ident;
                let id_str = id.to_string();
                let field_name = field_name(&var.ident);
                match &var.fields {
                    Fields::Named(_) => {
                        if !attr.ignore {
                            quote! {
                                #name::#id{..} =>  Ok(self.#field_name.get_event(rec.into())?)
                            }
                        } else {
                            quote! {
                                #name::#id{..} => Err(rtlola_interpreter::input::EventFactoryError::VariantIgnored(#id_str.to_string()))
                            }
                        }
                    },
                    Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                        let inner_name = Ident::new("rec", fields.unnamed.next().unwrap().span());
                        if !attr.ignore {
                            quote! {
                                #name::#id(#inner_name) => Ok(self.#field_name.get_event(#inner_name)?)
                            }
                        } else {
                            quote! {
                                #name::#id(_) => Err(rtlola_interpreter::input::EventFactoryError::VariantIgnored(#id_str.to_string()))
                            }
                        }
                    },
                    Fields::Unnamed(fields) => {
                        let fields = fields.unnamed.iter().map(|_| quote! {_});
                        if !attr.ignore {
                            quote! {
                                #name::#id(#(#fields),*) => Ok(self.#field_name.get_event(rec.into())?)
                            }
                        } else {
                            quote! {
                                #name::#id(#(#fields),*) => Err(rtlola_interpreter::input::EventFactoryError::VariantIgnored(#id_str.to_string()))
                            }
                        }
                    },
                    Fields::Unit => {
                        if !attr.ignore {
                            quote! {
                                #name::#id => Ok(self.#field_name.get_event(())?)
                            }
                        } else {
                            quote! {
                                #name::#id => Err(rtlola_interpreter::input::EventFactoryError::VariantIgnored(#id_str.to_string()))
                            }
                        }
                    },
                }
            })
            .collect();

        quote! {
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                match rec {
                    #(
                       #match_lines
                    ),*
                }
            }
        }
    }
}
