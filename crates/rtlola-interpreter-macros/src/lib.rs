extern crate proc_macro;
use std::collections::HashMap;

use deluxe;
use proc_macro::TokenStream;
use proc_macro2::Span as Span2;
use proc_macro_error::__export::proc_macro2::Span;
use proc_macro_error::{abort, proc_macro_error};
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Type};

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(record))]
struct RecordDeriveAttr {
    #[deluxe(default)]
    prefix: bool,
    custom_prefix: Option<Ident>,
}

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(record))]
struct RecordItemAttr {
    custom_name: Option<Ident>,
}

#[proc_macro_derive(Record, attributes(record))]
#[proc_macro_error]
pub fn derive_record_impl(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let mut input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let mut attr: RecordDeriveAttr = match deluxe::extract_attributes(&mut input) {
        Ok(attr) => attr,
        Err(e) => return e.into_compile_error().into(),
    };
    if attr.custom_prefix.is_some() {
        attr.prefix = true;
    }
    let DeriveInput {
        ident: name,
        generics,
        data,
        ..
    } = input.clone();

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let prefix = attr
        .prefix
        .then(|| {
            let mut p = attr.custom_prefix.unwrap_or(name.clone()).to_string();
            p.push('_');
            p
        })
        .unwrap_or_default();

    let fields = match data {
        Data::Struct(s) => {
            match s.fields {
                Fields::Named(fields) => fields.named,
                Fields::Unnamed(_) | Fields::Unit => {
                    abort!(&s.fields, "Record can only be derived for Structs with named fields!")
                },
            }
        },
        Data::Enum(_) | Data::Union(_) => abort!(input, "Record can only be derived for Structs!"),
    };
    let field_idents: Vec<Ident> = fields.iter().map(|f| f.ident.to_owned().unwrap()).collect();
    let field_names = match fields
        .into_iter()
        .map(|mut f| {
            deluxe::extract_attributes(&mut f).map(|attr: RecordItemAttr| {
                attr.custom_name
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap().to_string()))
            })
        })
        .collect::<Result<Vec<String>, _>>()
    {
        Ok(f) => f,
        Err(e) => return e.into_compile_error().into(),
    };

    let record_impl = quote! {
        impl #impl_generics rtlola_interpreter::monitor::Record for #name #ty_generics #where_clause {
            type CreationData = ();
            type Error = rtlola_interpreter::monitor::RecordError;

            fn func_for_input(name: &str, data: Self::CreationData) -> Result<rtlola_interpreter::monitor::ValueProjection<Self, Self::Error>, Self::Error>{
                match name {
                    #(
                        #field_names => Ok(Box::new(|data| Ok(data.#field_idents.clone().try_into()?))),
                    )*
                    _ => Err(rtlola_interpreter::monitor::RecordError::InputStreamUnknown(name.to_string()))
                }
            }
        }
    };
    return proc_macro::TokenStream::from(record_impl);
}

#[proc_macro_derive(Input, attributes(input))]
#[proc_macro_error]
pub fn derive_input_impl(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let DeriveInput {
        ident: name,
        generics,
        data,
        vis,
        ..
    } = input.clone();

    let struct_name = Ident::new(&format!("{name}Input"), Span2::call_site());
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let variants = match data {
        Data::Enum(e) => {
            e.variants
                .iter()
                .map(|v| {
                    let field = match &v.fields {
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => fields.unnamed.first().unwrap(),
                        Fields::Unnamed(_) | Fields::Named(_) | Fields::Unit => {
                            abort!(&v, "Only variants with a single unnamed field are supported!")
                        },
                    };
                    (v.ident.clone(), field.ty.clone())
                })
                .collect::<HashMap<Ident, Type>>()
        },
        Data::Struct(_) | Data::Union(_) => abort!(input, "Input can only be derived for an Enum of Records!"),
    };
    let (struct_field_names, struct_field_types): (Vec<Ident>, Vec<_>) = variants
        .iter()
        .map(|(ident, ty)| {
            (
                Ident::new(
                    &format!("{}_record_in", ident.to_string().to_lowercase()),
                    Span2::call_site(),
                ),
                quote! {rtlola_interpreter::monitor::RecordInput<#ty>},
            )
        })
        .unzip();
    let variant_paths: Vec<_> = variants.keys().map(|id| {
        let inner_name = Ident::new("rec", Span::call_site());
        quote!{#name::#id(#inner_name)}
    }).collect();

    let variant_strings: Vec<String> = variants.keys().map(Ident::to_string).collect();

    let trait_ident = Ident::new(&format!("{name}DerivedInput"), Span::call_site());

    let input_impl = quote! {
        #vis trait #trait_ident{
            type Input;
        }

        impl #impl_generics #trait_ident for #name #ty_generics #where_clause {
            type Input = #struct_name;
        }

        #vis struct #struct_name #ty_generics #where_clause {
            #(
                #struct_field_names: #struct_field_types
            ),*
        }

        impl #impl_generics rtlola_interpreter::monitor::Input for #struct_name #ty_generics #where_clause{
            type Record = #name;
            type Error = rtlola_interpreter::monitor::RecordError;
            type CreationData = ();

            fn new(map: std::collections::HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>, setup_data: Self::CreationData) -> Result<Self, Self::Error> {
                #(
                    let #struct_field_names: #struct_field_types = rtlola_interpreter::monitor::RecordInput::new_ignore_undefined(map.clone(), ()).0;
                )*

                Ok(TestInputInput {
                    #(
                        #struct_field_names,
                    )*
                })
            }

            /// This function converts a record to an event.
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                match rec {
                    #(
                        #variant_paths => Ok(self.#struct_field_names.get_event(rec)?),
                    )*
                }
            }
        }
    };
    return proc_macro::TokenStream::from(input_impl);
}
