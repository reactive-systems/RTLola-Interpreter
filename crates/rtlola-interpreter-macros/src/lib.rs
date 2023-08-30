extern crate proc_macro;
use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro_error::__export::proc_macro2::Span;
use proc_macro_error::{abort, proc_macro_error};
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Field, Fields, Ident, Type, Variant};

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
    #[deluxe(default)]
    ignore: bool,
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
    let fields = fields
        .into_iter()
        .map(|mut f| deluxe::extract_attributes(&mut f).map(|args| (f, args)))
        .collect::<Result<Vec<(Field, RecordItemAttr)>, _>>();
    let fields = match fields {
        Ok(f) => f,
        Err(e) => return e.into_compile_error().into(),
    };
    let (field_idents, field_names): (Vec<Ident>, Vec<String>) = fields
        .into_iter()
        .filter_map(|(f, attr)| {
            if !attr.ignore {
                let name = attr
                    .custom_name
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap()));
                Some((f.ident.unwrap(), name))
            } else {
                None
            }
        })
        .unzip();

    let record_impl = quote! {
        impl #impl_generics rtlola_interpreter::monitor::Record for #name #ty_generics #where_clause {
            type CreationData = ();
            type Error = rtlola_interpreter::monitor::InputError;

            fn func_for_input(name: &str, data: Self::CreationData) -> Result<rtlola_interpreter::monitor::ValueProjection<Self, Self::Error>, Self::Error>{
                match name {
                    #(
                        #field_names => Ok(Box::new(|data| Ok(data.#field_idents.clone().try_into()?))),
                    )*
                    _ => Err(rtlola_interpreter::monitor::InputError::InputStreamUnknown(vec![name.to_string()]))
                }
            }
        }
    };
    proc_macro::TokenStream::from(record_impl)
}

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(input))]
struct InputItemAttr {
    #[deluxe(default)]
    ignore: bool,
}

#[proc_macro_derive(Input, attributes(input))]
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

    let struct_name = format_ident!("{name}Input");
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let variants = match data {
        Data::Enum(e) => e.variants,
        Data::Struct(_) | Data::Union(_) => abort!(input, "Input can only be derived for an Enum of Records!"),
    };
    let variants: Result<Vec<(Variant, InputItemAttr)>, _> = variants
        .into_iter()
        .map(|mut v| deluxe::extract_attributes(&mut v).map(|args| (v, args)))
        .collect();
    let variants_attrs = match variants {
        Ok(v) => v,
        Err(e) => return e.into_compile_error().into(),
    };
    let variants = variants_attrs
        .iter()
        .filter_map(|(v, attr)| {
            if !attr.ignore {
                let field = match &v.fields {
                    Fields::Unnamed(fields) if fields.unnamed.len() == 1 => fields.unnamed.first().unwrap(),
                    Fields::Unnamed(_) | Fields::Named(_) | Fields::Unit => {
                        abort!(&v, "Only variants with a single unnamed field are supported!")
                    },
                };
                Some((v.ident.clone(), field.ty.clone()))
            } else {
                None
            }
        })
        .collect::<HashMap<Ident, Type>>();
    let (struct_field_names, struct_field_types): (Vec<Ident>, Vec<_>) = variants
        .iter()
        .map(|(ident, ty)| {
            let lc = ident.to_string().to_lowercase();
            (
                format_ident!("{lc}_record_in", span = ident.span()),
                quote! {<#ty as rtlola_interpreter::monitor::DerivedInput>::Input},
            )
        })
        .unzip();

    let variant_paths: Vec<_> = variants
        .keys()
        .map(|id| {
            let inner_name = Ident::new("rec", Span::call_site());
            quote! {#name::#id(#inner_name)}
        })
        .collect();

    let (ignored_variant_paths, ignored_variant_names): (Vec<_>, Vec<String>) = variants_attrs
        .iter()
        .filter_map(|(var, attr)| {
            let id = &var.ident;
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
            type Error = rtlola_interpreter::monitor::InputError;
            type CreationData = ();

            fn try_new(map: std::collections::HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>, setup_data: Self::CreationData) -> Result<(Self, Vec<String>), Self::Error> {
                let mut all_found = std::collections::HashSet::with_capacity(map.len());

                #(
                    let (#struct_field_names, found) = #struct_field_types::try_new(map.clone(), ())?;
                    all_found.extend(found);
                )*

                Ok( (#struct_name{
                    #(
                        #struct_field_names,
                    )*
                }, all_found.into_iter().collect()))
            }



            /// This function converts a record to an event.
            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                match rec {
                    #(
                        #variant_paths => Ok(self.#struct_field_names.get_event(rec)?),
                    )*
                    #(
                        #ignored_variant_paths => Err(rtlola_interpreter::monitor::InputError::VariantIgnored(#ignored_variant_names.to_string())),
                    )*
                }
            }
        }
    };
    proc_macro::TokenStream::from(input_impl)
}
