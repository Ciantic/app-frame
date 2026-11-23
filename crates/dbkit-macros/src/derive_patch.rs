//! Derive of a model's patch input type.
//!
//! The generated struct holds the primary key and version as-is, so an update
//! always names its row and its expected version, and turns every other
//! business column into a [`dbkit::Patch`] field: a missing key leaves the
//! column alone and `null` clears it. Audit columns are not inputs, so they are
//! omitted.

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

    let Some(version) = &annotations.version else {
        return Err(syn::Error::new(
            input.ident.span(),
            "`DbPatch` requires a `#[db(version)]` field for optimistic locking",
        ));
    };

    let Data::Struct(data) = &input.data else {
        unreachable!("`annotations::parse` rejects non-structs");
    };
    let Fields::Named(fields) = &data.fields else {
        unreachable!("`annotations::parse` rejects unnamed fields");
    };

    let patch = format_ident!("{name}Patch");
    let doc = format!(
        "Patch input of [`{name}`]. The primary key and `version` are required; every other \
         column is a [`dbkit::Patch`] field."
    );
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let mut definitions = Vec::new();
    let mut schema_fields = Vec::new();

    for field in &fields.named {
        let field_name = field.ident.as_ref().expect("named field");
        let key = field_name.to_string();
        let ty = &field.ty;

        if annotations.created_at.as_ref() == Some(field_name)
            || annotations.updated_at.as_ref() == Some(field_name)
        {
            continue;
        }

        if annotations.primary_key.contains(field_name) || version == field_name {
            // The version is the optimistic-locking token, so it is always an
            // integer; the primary key may be any type with its own schema.
            let schema = if version == field_name {
                quote!(::zod_rs::number().int())
            } else {
                type_schema(ty)
            };
            definitions.push(quote!(pub #field_name: #ty));
            schema_fields.push(quote!(.field(#key, #schema)));
        } else {
            let inner = non_optional(ty);
            definitions.push(quote! {
                #[serde(default, skip_serializing_if = "::dbkit::Patch::is_absent")]
                pub #field_name: ::dbkit::Patch<#inner>
            });
            let schema = type_schema(inner);
            schema_fields.push(quote! {
                .optional_field(#key, ::dbkit::Patch::schema(#schema))
            });
        }
    }

    Ok(quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        pub struct #patch #generics {
            #(#definitions),*
        }

        impl #impl_generics #patch #type_generics #where_clause {
            /// Validates a patch body: required fields must be present and
            /// every `Patch::Set` value must satisfy its column's schema.
            pub fn schema() -> impl ::zod_rs::Schema<::zod_rs::prelude::Value> {
                ::zod_rs::object()
                    #(#schema_fields)*
            }

            /// Validates `value` against [`Self::schema`] and deserializes it.
            pub fn validate_and_parse(
                value: &::zod_rs::prelude::Value,
            ) -> ::core::result::Result<
                Self,
                ::zod_rs::__private::ValidationResult,
            > {
                match ::zod_rs::Schema::validate(&Self::schema(), value) {
                    Ok(_) => ::serde_json::from_value(value.clone()).map_err(|error| {
                        ::zod_rs::__private::ValidationError::custom(format!(
                            "Deserialization failed: {error}"
                        ))
                        .into()
                    }),
                    Err(errors) => Err(errors),
                }
            }
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

/// The schema a model field is validated with.
///
/// Mirrors `#[derive(ZodSchema)]`: primitives get a built-in schema and every
/// other type is expected to have a `schema()` constructor.
fn type_schema(ty: &Type) -> TokenStream {
    let Type::Path(path) = ty else {
        return quote!(::zod_rs::string());
    };
    let Some(segment) = path.path.segments.last() else {
        return quote!(::zod_rs::string());
    };

    match segment.ident.to_string().as_str() {
        "String" => quote!(::zod_rs::string()),
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" => {
            quote!(::zod_rs::number().int())
        }
        "f32" | "f64" => quote!(::zod_rs::number()),
        "bool" => quote!(::zod_rs::boolean()),
        "Vec" => {
            let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return quote!(::zod_rs::array(::zod_rs::string()));
            };
            match arguments.args.first() {
                Some(syn::GenericArgument::Type(element)) => {
                    let element = type_schema(element);
                    quote!(::zod_rs::array(#element))
                }
                _ => quote!(::zod_rs::array(::zod_rs::string())),
            }
        }
        _ => {
            let name = &segment.ident;
            quote!(#name::schema())
        }
    }
}
