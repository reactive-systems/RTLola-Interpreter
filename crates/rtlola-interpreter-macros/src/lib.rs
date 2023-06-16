extern crate proc_macro;
use deluxe;
use proc_macro::TokenStream;
use proc_macro_error::{abort, proc_macro_error};
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident};

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
