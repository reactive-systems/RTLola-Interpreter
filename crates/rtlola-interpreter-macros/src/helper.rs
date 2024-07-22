use convert_case::{Case, Casing};
use proc_macro2::Ident;
use quote::format_ident;
use syn::{GenericArgument, PathArguments, Type};

pub(crate) fn new_snake_ident(base: &Ident, suffix: &str) -> Ident {
    let ident_str = base.to_string().to_case(Case::Snake);
    format_ident!("{}_{}", ident_str, suffix, span = base.span())
}

pub(crate) fn option_inner_type(ty: &Type) -> Option<&GenericArgument> {
    if let Type::Path(path) = ty {
        let segment = path.path.segments.last()?;
        if &segment.ident.to_string() == "Option" {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                return args.args.first();
            }
        }
    };
    None
}

pub(crate) fn hashmap_types(ty: &Type) -> Option<(Vec<&Type>, &GenericArgument)> {
    if let Type::Path(path) = ty {
        let segment = path.path.segments.last()?;
        if &segment.ident.to_string() == "HashMap" {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                let value_ty = args.args.last()?;
                let instance = args.args.first()?;
                let args: Vec<&Type> = if let GenericArgument::Type(Type::Tuple(args)) = &instance {
                    args.elems.iter().collect()
                } else if let GenericArgument::Type(t) = instance {
                    vec![t]
                } else {
                    vec![]
                };
                return Some((args, value_ty));
            }
        }
    };
    None
}

pub(crate) fn hashset_types(ty: &Type) -> Option<Vec<&Type>> {
    if let Type::Path(path) = ty {
        let segment = path.path.segments.last()?;
        if &segment.ident.to_string() == "HashSet" {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                let instance = args.args.first()?;
                let args: Vec<&Type> = if let GenericArgument::Type(Type::Tuple(args)) = &instance {
                    args.elems.iter().collect()
                } else if let GenericArgument::Type(t) = instance {
                    vec![t]
                } else {
                    vec![]
                };
                return Some(args);
            }
        }
    };
    None
}

pub(crate) fn is_bool(ty: &Type) -> bool {
    if let Type::Path(path) = ty {
        path.path
            .segments
            .last()
            .map(|segment| segment.ident.to_string() == "bool")
            .unwrap_or(false)
    } else {
        false
    }
}

pub(crate) fn is_bool_generic(ty: &GenericArgument) -> bool {
    if let GenericArgument::Type(ty) = ty {
        is_bool(ty)
    } else {
        false
    }
}
