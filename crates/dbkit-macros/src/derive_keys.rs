//! Generation of a table's key structs.
//!
//! `{Model}PK` holds exactly the primary key columns, which an insert returns.
//! `{Model}PKV` adds the version column when the model has one, for operations
//! that identify a row and check its version. Both decode from a `RETURNING`
//! list by column name, which keeps a raw identifier working.

use crate::annotations;
use crate::annotations::Column;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Ident;
use syn::Type;
use syn::Visibility;

pub(crate) fn expand(
    input: &DeriveInput,
    annotations: &annotations::Annotations,
) -> syn::Result<TokenStream> {
    if !matches!(annotations.shape, annotations::Shape::Table(_)) {
        return Ok(quote!());
    }

    let name = &input.ident;
    let visibility = &input.vis;

    let mut key = Vec::new();
    for field in &annotations.primary_key {
        key.push(column(annotations, field)?);
    }

    let pk = format_ident!("{name}PK");
    let pk_doc = format!("The primary key of [`{name}`].");

    let pkv = format_ident!("{name}PKV");
    let pkv = if let Some(version) = &annotations.version {
        let mut key_and_version = key.clone();
        key_and_version.push(column(annotations, version)?);

        let doc = format!(
            "The primary key and version of [`{name}`], for operations that identify a row and \
             check its version."
        );

        key_struct(&pkv, visibility, &key_and_version, doc)
    } else {
        let doc = format!(
            "The primary key of [`{name}`]; the model has no version, so this is [`{pk}`]."
        );

        quote! {
            #[doc = #doc]
            #[allow(dead_code)]
            #visibility type #pkv = #pk;
        }
    };

    let pk = key_struct(&pk, visibility, &key, pk_doc);

    Ok(quote! {
        #pk
        #pkv
    })
}

/// The key fields as written in the model: the first is the primary key and the
/// last, when present, is the version.
fn key_struct(
    name: &Ident,
    visibility: &Visibility,
    fields: &[(&Ident, &Type)],
    doc: String,
) -> TokenStream {
    let mut definitions = Vec::new();
    let mut decoders = Vec::new();

    for (field, ty) in fields {
        let literal = syn::LitStr::new(field.to_string().trim_start_matches("r#"), field.span());

        definitions.push(quote!(pub #field: #ty));
        decoders.push(quote!(#field: row.try_get(#literal)?));
    }

    quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        #visibility struct #name {
            #(#definitions),*
        }

        impl<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> for #name {
            fn from_row(
                row: &'r ::sqlx::postgres::PgRow,
            ) -> ::core::result::Result<Self, ::sqlx::Error> {
                use ::sqlx::Row as _;

                ::core::result::Result::Ok(Self {
                    #(#decoders),*
                })
            }
        }
    }
}

/// Looks up the scalar field behind a role annotation.
fn column<'a>(
    annotations: &'a annotations::Annotations,
    field: &Ident,
) -> syn::Result<(&'a Ident, &'a Type)> {
    annotations
        .columns
        .iter()
        .find_map(|column| match column {
            Column::Scalar { name, ty, .. } if name == field => Some((name, ty)),
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new_spanned(
                field,
                "a primary key or version column must be a scalar field",
            )
        })
}
