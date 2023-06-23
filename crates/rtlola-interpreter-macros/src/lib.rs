extern crate proc_macro;
use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::Span as Span2;
use proc_macro_error::__export::proc_macro2::Span;
use proc_macro_error::{abort, proc_macro_error};
use quote::{format_ident, quote};
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
/// A derive macro that implements the [Record](rtlola_interpreter::monitor::Record) trait for a struct based on its field names.
/// For an example look at `simple.rs` and `custom_names.rs` in the example folder.
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
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap()))
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
    proc_macro::TokenStream::from(record_impl)
}

#[proc_macro_derive(Input)]
#[proc_macro_error]
/// A derive macro that implements the [Input](rtlola_interpreter::monitor::Input) trait for an enum who's variants have a single unnamed field that implements [Record](rtlola_interpreter::monitor::Record) trait.
/// For an example look at `enum.rs` in the example folder.
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
                format_ident!("{ident}_record_in"),
                quote! {rtlola_interpreter::monitor::RecordInput<#ty>},
            )
        })
        .unzip();
    let struct_field_errs: Vec<Ident> = variants.keys().map(|var| format_ident!("{var}_errs")).collect();

    let variant_paths: Vec<_> = variants
        .keys()
        .map(|id| {
            let inner_name = Ident::new("rec", Span::call_site());
            quote! {#name::#id(#inner_name)}
        })
        .collect();

    let (variant_found, variant_missing): (Vec<Ident>, Vec<Ident>) = variants
        .keys()
        .map(|var| (format_ident!("{var}_found"), format_ident!("{var}_missing")))
        .unzip();

    let input_impl = quote! {
        impl #impl_generics rtlola_interpreter::monitor::DerivedInput for #name #ty_generics #where_clause {
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
                let all_streams: std::collections::HashSet<String> = map.keys().cloned().collect();
                let mut all_found = std::collections::HashSet::with_capacity(map.len());

                #(
                    let (#struct_field_names, mut #struct_field_errs) = rtlola_interpreter::monitor::RecordInput::new_ignore_undefined(map.clone(), ());
                    let #variant_missing: std::collections::HashSet<String> = #struct_field_errs.keys().cloned().collect();
                    let #variant_found = all_streams.difference(&#variant_missing).cloned();
                    all_found.extend(#variant_found);
                )*

                let errs: std::collections::HashMap<String, Vec<Self::Error>> = all_streams
                    .difference(&all_found)
                    .map(|missing| {
                        let mut record_errs: Vec<Self::Error> = vec![];

                        #(
                             #struct_field_errs.remove(missing.as_str()).map(
                                |e: <#struct_field_types as rtlola_interpreter::monitor::Input>::Error| {
                                    record_errs.push(rtlola_interpreter::monitor::RecordError::from(e))
                                },
                            );
                        )*

                        ((*missing).clone(), record_errs)
                    })
                    .collect();

                if errs.is_empty() {
                    Ok( #struct_name{
                        #(
                            #struct_field_names,
                        )*
                    })
                } else {
                    Err(rtlola_interpreter::monitor::RecordError::InputStreamNotFound(errs))
                }
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
    proc_macro::TokenStream::from(input_impl)
}
