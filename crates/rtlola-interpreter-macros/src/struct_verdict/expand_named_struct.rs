use std::default::Default;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Field, Fields, Type, TypeTuple};

use crate::helper::{hashmap_types, hashset_types, is_bool, is_bool_generic, new_snake_ident, option_inner_type};
use crate::FactoryAttr;

const TIME_NAMES: [&str; 3] = ["time", "ts", "timestamp"];

// fn expand_value_convert(stream_val: TokenStream, is_trigger: bool, target_ty: TokenStream) -> TokenStream {
//
// }

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
    let (stream_names, (deconstructor, field_init)): (Vec<String>, (Vec<TokenStream>,Vec<TokenStream>)) = fields
        .iter()
        .filter_map(|(f, attr)| {
            let name = f.ident.as_ref().unwrap().to_string();
            if TIME_NAMES.contains(&name.as_str()) || attr.is_time {
                time_field.replace((*f).clone());
                None
            } else if !attr.ignore {
                let stream_name = attr
                    .custom_name
                    .as_ref()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("{prefix}{}", f.ident.as_ref().unwrap()));
                let is_trigger = stream_name.starts_with("trigger_");
                let val_ident = new_snake_ident(f.ident.as_ref().unwrap(), "val");
                let ident = f.ident.as_ref().unwrap();
                let (decon, field_init) = if let Some((params, val)) = hashmap_types(&f.ty) {
                    let decon = quote! {rtlola_interpreter::output::StreamValue::Instances(#val_ident)};
                    let params_count = params.len();
                    let params_idents: Vec<Ident> = (1..=params_count).map(|idx| format_ident!("p{}", idx)).collect();
                    let field_init = if is_trigger && is_bool_generic(val) {
                        quote! {
                            #ident: #val_ident.into_iter().map(|(params, val)|{
                            let got_number_params = params.len();
                            let [#(#params_idents),*]: [rtlola_interpreter::Value; #params_count] = params.try_into().map_err(|_| rtlola_interpreter::output::FromValuesError::InvalidHashMap{stream_name: #stream_name.to_string(), expected_num_params: #params_count, got_number_params})?;
                            Ok(
                                ((#(<rtlola_interpreter::Value as TryInto<#params>>::try_into(#params_idents)?),*), true)
                            )
                        }).collect::<Result<_,rtlola_interpreter::output::FromValuesError>>()?
                        }
                    } else {
                        quote! {
                            #ident: #val_ident.into_iter().map(|(params, val)|{
                            let got_number_params = params.len();
                            let [#(#params_idents),*]: [rtlola_interpreter::Value; #params_count] = params.try_into().map_err(|_| rtlola_interpreter::output::FromValuesError::InvalidHashMap{stream_name: #stream_name.to_string(), expected_num_params: #params_count, got_number_params})?;
                            Ok(
                                ((#(<rtlola_interpreter::Value as TryInto<#params>>::try_into(#params_idents)?),*), <rtlola_interpreter::Value as TryInto<#val>>::try_into(val)?)
                            )
                        }).collect::<Result<_,rtlola_interpreter::output::FromValuesError>>()?
                        }
                    };
                    (decon, field_init)
                }  else if let Some(params) = hashset_types(&f.ty){
                    if is_trigger {
                        let decon = quote! {rtlola_interpreter::output::StreamValue::Instances(#val_ident)};
                        let params_count = params.len();
                        let params_idents: Vec<Ident> = (1..=params_count).map(|idx| format_ident!("p{}", idx)).collect();
                        let field_init = quote! {
                            #ident: #val_ident.into_iter().map(|(params, _)|{
                            let got_number_params = params.len();
                            let [#(#params_idents),*]: [rtlola_interpreter::Value; #params_count] = params.try_into().map_err(|_| rtlola_interpreter::output::FromValuesError::InvalidHashMap{stream_name: #stream_name.to_string(), expected_num_params: #params_count, got_number_params})?;
                            Ok(
                                (#(<rtlola_interpreter::Value as TryInto<#params>>::try_into(#params_idents)?),*)
                            )
                        }).collect::<Result<_,rtlola_interpreter::output::FromValuesError>>()?
                        };
                        (decon, field_init)
                    } else {
                        panic!("HashSet field type is only valid for triggers");
                    }
                } else if let Some(inner) = option_inner_type(&f.ty) {
                    let decon = quote! {rtlola_interpreter::output::StreamValue::Stream(#val_ident)};
                    let field_init = quote! {#ident: #val_ident.map(<rtlola_interpreter::Value as TryInto<#inner>>::try_into).transpose()?};
                    (decon, field_init)
                } else {
                    let decon = quote! {rtlola_interpreter::output::StreamValue::Stream(#val_ident)};
                    let ty = &f.ty;
                    let field_init = if is_trigger && is_bool(ty) {
                        quote! {#ident: #val_ident.is_some()}
                    } else {
                        quote! {#ident: <rtlola_interpreter::Value as TryInto<#ty>>::try_into(#val_ident.ok_or_else(|| rtlola_interpreter::output::FromValuesError::ExpectedValue{stream_name: #stream_name.to_string()})?)?}
                    };
                    (decon, field_init)
                };
                Some((stream_name, (decon, field_init)))
            } else {
                None
            }
        })
        .unzip();

    let ignored_fields: Vec<TokenStream> = fields
        .iter()
        .filter_map(|(f, attr)| {
            if attr.ignore {
                let ident = f.ident.as_ref().unwrap();
                Some(quote! {#ident: std::default::Default::default()})
            } else {
                None
            }
        })
        .collect();

    let time_type = time_field.as_ref().map(|tf| tf.ty.clone()).unwrap_or_else(|| {
        Type::Tuple(TypeTuple {
            paren_token: Default::default(),
            elems: Default::default(),
        })
    });
    let time_init = time_field
        .as_ref()
        .and_then(|f| f.ident.as_ref())
        .map(|id| quote! {#id: ts});
    let num_streams = stream_names.len();

    let initializers: Vec<TokenStream> = time_init.into_iter().chain(field_init).chain(ignored_fields).collect();

    quote! {
        impl #impl_generics rtlola_interpreter::output::FromValues for #name #ty_generics #where_clause {
            type OutputTime = #time_type;

            fn streams() -> Vec<String> {
                vec![#(#stream_names.to_string()),*]
            }

            fn construct(ts: Self::OutputTime, data: Vec<rtlola_interpreter::output::StreamValue>) -> Result<Self, rtlola_interpreter::output::FromValuesError> {
                let [#(#deconstructor),*]: [rtlola_interpreter::output::StreamValue; #num_streams] =
                    data.try_into().expect("Mapping to work!")
                else {
                    return Err(rtlola_interpreter::output::FromValuesError::StreamKindMismatch);
                };
                Ok(
                    Self{
                        #(#initializers),*
                    }
                )
            }
        }
    }
}
