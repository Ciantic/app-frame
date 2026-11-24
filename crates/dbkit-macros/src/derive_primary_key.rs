//! Generation of a table's primary-key struct.
//!
//! An insert returns only the columns the database generated or the caller
//! supplied as the key, so `{Model}PrimaryKey` holds exactly those fields. It
//! decodes from the `RETURNING` list by column name, which keeps a raw
//! identifier working.

use crate::annotations;
use crate::annotations::Column;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;

pub(crate) fn expand(
    input: &DeriveInput,
    annotations: &annotations::Annotations,
) -> syn::Result<TokenStream> {
    if !matches!(annotations.shape, annotations::Shape::Table(_)) {
        return Ok(quote!());
    }

    let name = &input.ident;
    let visibility = &input.vis;
    let key = format_ident!("{name}PrimaryKey");
    let doc = format!("The primary key of [`{name}`], returned by an insert.");

    let mut fields = Vec::new();
    let mut decoders = Vec::new();

    for field in &annotations.primary_key {
        let column = annotations.columns.iter().find_map(|column| match column {
            Column::Scalar { name, ty, .. } if name == field => Some((name, ty)),
            _ => None,
        });
        let Some((name, ty)) = column else {
            return Err(syn::Error::new_spanned(
                field,
                "a primary key column must be a scalar field",
            ));
        };
        let literal = syn::LitStr::new(name.to_string().trim_start_matches("r#"), name.span());

        fields.push(quote!(pub #name: #ty));
        decoders.push(quote!(#name: row.try_get(#literal)?));
    }

    Ok(quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        #visibility struct #key {
            #(#fields),*
        }

        impl<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> for #key {
            fn from_row(
                row: &'r ::sqlx::postgres::PgRow,
            ) -> ::core::result::Result<Self, ::sqlx::Error> {
                use ::sqlx::Row as _;

                ::core::result::Result::Ok(Self {
                    #(#decoders),*
                })
            }
        }
    })
}
