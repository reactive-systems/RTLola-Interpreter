use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Field, Fields, Type, TypeTuple};
use crate::FactoryAttr;
use std::default::Default;
use crate::helper::hashmap_types;

const TIME_NAMES: [&'static str; 3] = ["time", "ts", "timestamp"];

macro_rules! init_field {
    ($(std::option::)?Option<$ftype: ty>, $ident: ident) => {
        $ident.map(<Value as TryInto<$ftype>>::try_into).transpose()?
    };
    ($(std::collections::)?HashMap<$params: tt, $targetType: ty>, $count: expr, $ident: ident) => {
        seq!(N in 1..=$count {
            $ident.into_iter().map(|(params, val)|{
                let [#(p~N,)*]: [Value; $count] = params.try_into().expect("A correct HashMap spec");
                Ok(
                    ((#(p~N.try_into()?,)*), val.try_into()?)
                )
            }).collect::<Result<_,_>>()?
        })
    };
    ($ftype: ty, $ident: ident) => {
        <Value as TryInto<$ftype>>::try_into($ident.unwrap())?
    };
}

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

    let mut time_field: Option<Field> = None;
    let (field_idents, stream_names): (Vec<Ident>, Vec<String>) = fields
        .into_iter()
        .filter_map(|(f, attr)| {
            let name = f.ident.as_ref().unwrap().to_string();
            if TIME_NAMES.contains(&name.as_str()) {
                time_field.replace(f.clone());
                None
            } else if !attr.ignore {
                let name = attr
                    .custom_name
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap()));
                dbg!(hashmap_types(&f.ty));
                Some((f.ident.clone().unwrap(), name))
            } else {
                None
            }
        })
        .unzip();

    let time_type = time_field.map(|tf| tf.ty).unwrap_or_else(|| Type::Tuple(TypeTuple{
        paren_token: Default::default(),
        elems: Default::default(),
    }));
    quote! {
        impl #impl_generics rtlola_interpreter::output::FromValues for #name #ty_generics #where_clause {
            type OutputTime = #time_type;

            fn streams() -> Vec<String> {
                vec![#(#stream_names.to_string()),*]
            }

            fn construct(ts: Self::OutputTime, data: Vec<StreamValue>) -> Result<Self, ValueConvertError> {
                let [StreamValue::Stream(a), StreamValue::Stream(b), StreamValue::Stream(c), StreamValue::Instances(d)]: [StreamValue; 4] =
                    data.try_into().expect("Mapping to work!")
                else {
                    panic!("Mapping did not work");
                };
            }

            // fn func_for_input(name: &str, data: Self::CreationData) -> Result<rtlola_interpreter::input::ValueGetter<Self, Self::Error>, Self::Error>{
            //     match name {
            //         #(
            //             #field_names => Ok(Box::new(|data| Ok(data.#field_idents.clone().try_into()?))),
            //         )*
            //         _ => Err(rtlola_interpreter::input::EventFactoryError::InputStreamUnknown(vec![name.to_string()]))
            //     }
            // }
        }
    }
}