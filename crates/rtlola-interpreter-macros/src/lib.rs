pub(crate) mod helper;
mod input_enums;

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_error::{abort, proc_macro_error};
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Field, Fields, Ident};

use crate::input_enums::EnumDeriver;

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
        impl #impl_generics rtlola_interpreter::input::Record for #name #ty_generics #where_clause {
            type CreationData = ();
            type Error = rtlola_interpreter::input::InputError;

            fn func_for_input(name: &str, data: Self::CreationData) -> Result<rtlola_interpreter::input::ValueProjection<Self, Self::Error>, Self::Error>{
                match name {
                    #(
                        #field_names => Ok(Box::new(|data| Ok(data.#field_idents.clone().try_into()?))),
                    )*
                    _ => Err(rtlola_interpreter::input::InputError::InputStreamUnknown(vec![name.to_string()]))
                }
            }
        }
    };
    proc_macro::TokenStream::from(record_impl)
}

pub(crate) trait InputDeriver {
    fn struct_field_names(&self) -> Vec<Ident>;
    fn struct_field_types(&self) -> Vec<TokenStream2>;
    fn get_event(&self) -> TokenStream2;
}

#[derive(deluxe::ExtractAttributes, Debug, Clone)]
#[deluxe(attributes(input))]
struct InputItemAttr {
    #[deluxe(default)]
    ignore: bool,
}

#[proc_macro_derive(Input, attributes(input))]
#[proc_macro_error]
/// A derive macro that implements the [Input](rtlola_interpreter::monitor::EventFactory) trait for an enum who's variants have a single unnamed field that implements [Record](rtlola_interpreter::monitor::Record) trait.
/// For an example look at `enum.rs` in the example folder.
pub fn derive_input_impl(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input: DeriveInput = parse_macro_input!(input as DeriveInput);

    let (deriver, init_code): (Box<dyn InputDeriver>, TokenStream2) = match &input.data {
        Data::Struct(_) => todo!(),
        Data::Enum(_) => {
            match EnumDeriver::new(input.clone()) {
                Ok((ed, stream)) => (Box::new(ed), stream),
                Err(e) => return e.into(),
            }
        },
        Data::Union(_) => abort!(&input, "Input can only be derived for structs and enums"),
    };

    let DeriveInput {
        ident: name,
        generics,
        vis,
        ..
    } = input;

    let struct_name = format_ident!("{name}Input");
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let field_names = deriver.struct_field_names();
    let field_types = deriver.struct_field_types();
    let get_event = deriver.get_event();

    let input_impl = quote! {
        impl #impl_generics rtlola_interpreter::input::AssociatedInput for #name #ty_generics #where_clause {
            type Input = #struct_name;
        }

        #init_code

        #vis struct #struct_name #ty_generics #where_clause {
            #(
                #field_names: #field_types
            ),*
        }

        impl #impl_generics rtlola_interpreter::input::Input for #struct_name #ty_generics #where_clause{
            type Record = #name;
            type Error = rtlola_interpreter::input::InputError;
            type CreationData = ();

            fn try_new(map: std::collections::HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>, setup_data: Self::CreationData) -> Result<(Self, Vec<String>), Self::Error> {
                let mut all_found = std::collections::HashSet::with_capacity(map.len());

                #(
                    let (#field_names, found) = #field_types::try_new(map.clone(), ())?;
                    all_found.extend(found);
                )*

                Ok( (#struct_name{
                    #(
                        #field_names,
                    )*
                }, all_found.into_iter().collect()))
            }

            #get_event
        }
    };
    proc_macro::TokenStream::from(input_impl)
}
