use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Field, Fields};

use crate::FactoryAttr;

pub(crate) fn expand_named_struct(input: &DeriveInput) -> TokenStream {
    let mut attr: FactoryAttr = match deluxe::parse_attributes(input) {
        Ok(attr) => attr,
        Err(e) => return e.into_compile_error(),
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

    let Data::Struct(struct_data) = &data else {
        unreachable!()
    };
    let Fields::Named(fields) = &struct_data.fields else {
        unreachable!()
    };

    let fields = fields
        .named
        .iter()
        .map(|f| deluxe::parse_attributes(f).map(|args| (f, args)))
        .collect::<Result<Vec<(&Field, FactoryAttr)>, _>>();
    let fields = match fields {
        Ok(f) => f,
        Err(e) => return e.into_compile_error(),
    };
    let (field_idents, field_names): (Vec<Ident>, Vec<String>) = fields
        .into_iter()
        .filter_map(|(f, attr)| {
            if !attr.ignore {
                let name = attr
                    .custom_name
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap()));
                Some((f.ident.clone().unwrap(), name))
            } else {
                None
            }
        })
        .unzip();

    quote! {
        impl #impl_generics rtlola_interpreter::input::InputMap for #name #ty_generics #where_clause {
            type CreationData = ();
            type Error = rtlola_interpreter::input::EventFactoryError;

            fn func_for_input(name: &str, data: Self::CreationData) -> Result<rtlola_interpreter::input::ValueGetter<Self, Self::Error>, Self::Error>{
                match name {
                    #(
                        #field_names => Ok(Box::new(|data| Ok(data.#field_idents.clone().try_into()?))),
                    )*
                    _ => Err(rtlola_interpreter::input::EventFactoryError::InputStreamUnknown(vec![name.to_string()]))
                }
            }
        }
    }
}

pub(crate) fn expand_unnamed_struct(input: &DeriveInput) -> TokenStream {
    let DeriveInput {
        ident: name,
        generics,
        data,
        ..
    } = input;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let Data::Struct(struct_data) = data else {
        unreachable!()
    };
    let Fields::Unnamed(fields) = &struct_data.fields else {
        unreachable!()
    };
    let fields = &fields.unnamed;

    let factory = format_ident!("{name}Factory");
    let num_inputs = fields.len();
    let from_type: Vec<_> = fields.iter().map(|_| quote! {rtlola_interpreter::Value}).collect();
    let field_names: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(idx, field)| format_ident!("inner_{idx}", span = field.span()))
        .collect();
    let inner_type = quote! {
        rtlola_interpreter::input::ArrayFactory<
                #num_inputs,
                std::convert::Infallible,
                (#(#from_type),*)
            >
    };
    let type_args: Vec<_> = generics.params.iter().collect();

    quote! {
        impl #impl_generics rtlola_interpreter::input::AssociatedEventFactory for #name #ty_generics #where_clause {
            type Factory = #factory #ty_generics;
        }

        struct #factory #ty_generics (#inner_type, std::marker::PhantomData<(#(#type_args),*)> ) #where_clause;

        impl #impl_generics rtlola_interpreter::input::EventFactory for #factory #ty_generics
            #where_clause
        {
            type Record = #name #ty_generics;
            type Error = rtlola_interpreter::input::EventFactoryError;
            type CreationData = ();

            fn try_new(
                map: std::collections::HashMap<String, rtlola_interpreter::rtlola_frontend::mir::InputReference>,
                setup_data: Self::CreationData,
            ) -> Result<(Self, Vec<String>), Self::Error>{
                rtlola_interpreter::input::ArrayFactory::try_new(map, setup_data).map(|(f, found)| (#factory(f, std::marker::PhantomData::default()), found))
            }

            fn get_event(&self, rec: Self::Record) -> Result<rtlola_interpreter::monitor::Event, Self::Error>{
                let #name(#(#field_names),*) = rec;
                self.0.get_event((#(#field_names.try_into()?),*)).map_err(|e| e.into())
            }
        }
    }
}

pub(crate) fn expand_unit_struct(input: &DeriveInput) -> TokenStream {
    let DeriveInput {
        ident: name, generics, ..
    } = input;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics rtlola_interpreter::input::AssociatedEventFactory for #name #ty_generics #where_clause {
            type Factory = rtlola_interpreter::input::EmptyFactory<Self>;
        }
    }
}
