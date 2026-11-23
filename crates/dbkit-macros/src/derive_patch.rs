//! Derive of a model's patch input type.
//!
//! The generated struct holds the primary key and, when the model has one, the
//! version as-is: an update always names its row and states the version it
//! expects. Every other business column becomes a [`dbkit::Patch`] field, so a
//! missing key leaves the column alone and `null` clears it. Audit columns are
//! not inputs, so they are omitted.
//!
//! The struct derives `garde::Validate`, and a patch field whose payload type
//! can carry rules dives into it, so `Set` values are validated while `Absent`
//! and `Null` pass.

use crate::annotations;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;
use syn::Type;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let annotations = annotations::parse(input)?;
    let name = &input.ident;
    let version = annotations.version.as_ref();

    let Data::Struct(data) = &input.data else {
        unreachable!("`annotations::parse` rejects non-structs");
    };
    let Fields::Named(fields) = &data.fields else {
        unreachable!("`annotations::parse` rejects unnamed fields");
    };

    let patch = format_ident!("{name}Patch");
    let doc = format!(
        "Patch input of [`{name}`]. The primary key is required, as is `version` when the model \
         has one; every other column is a [`dbkit::Patch`] field."
    );
    let generics = &input.generics;

    let mut definitions = Vec::new();

    for field in &fields.named {
        let field_name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;

        if annotations.created_at.as_ref() == Some(field_name)
            || annotations.updated_at.as_ref() == Some(field_name)
        {
            continue;
        }

        if annotations.primary_key.contains(field_name) || version == Some(field_name) {
            definitions.push(quote! {
                #[garde(skip)]
                pub #field_name: #ty
            });
        } else {
            let inner = non_optional(ty);
            let garde = if carries_rules(inner) {
                quote!(#[garde(dive)])
            } else {
                quote!(#[garde(skip)])
            };
            definitions.push(quote! {
                #[serde(default, skip_serializing_if = "::dbkit::Patch::is_absent")]
                #garde
                pub #field_name: ::dbkit::Patch<#inner>
            });
        }
    }

    Ok(quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::garde::Validate)]
        pub struct #patch #generics {
            #(#definitions),*
        }
    })
}

/// The type inside `Option<...>`, or the type itself.
///
/// A nullable column's patch is `Patch<T>` rather than `Patch<Option<T>>`, so
/// `null` means "clear the column" and a value means "set it".
fn non_optional(ty: &Type) -> &Type {
    let Type::Path(path) = ty else {
        return ty;
    };
    let Some(segment) = path.path.segments.last() else {
        return ty;
    };
    if segment.ident != "Option" {
        return ty;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return ty;
    };
    match arguments.args.first() {
        Some(syn::GenericArgument::Type(inner)) => inner,
        _ => ty,
    }
}

/// Whether a payload type can carry validation rules.
///
/// Only a local type can implement `garde::Validate`, so a primitive or a
/// foreign type is skipped; `Patch<T>` then validates nothing, which matches a
/// type that has no rules.
fn carries_rules(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return true;
    };
    let Some(segment) = path.path.segments.last() else {
        return true;
    };

    !matches!(
        segment.ident.to_string().as_str(),
        "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "isize"
            | "usize"
            | "f32"
            | "f64"
            | "Uuid"
            | "DateTime"
    )
}
