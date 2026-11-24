//! Derive of `<Model>Select`: one `Selected<T>` per column, so a missing column differs from a selected `NULL`.

use crate::annotations;
use crate::annotations::Column;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Type;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let annotations = annotations::parse(input)?;
    let name = &input.ident;
    let select = format_ident!("{name}Select");
    let doc = format!(
        "One row of [`{name}`] decoded from a projection: every column is a [`dbkit::Selected`], \
         `Set` when the query selected it and `Absent` otherwise."
    );
    let generics = &input.generics;

    let mut definitions = Vec::new();
    let mut decoders = Vec::new();
    let mut emptiness = Vec::new();

    for column in &annotations.columns {
        match column {
            Column::Flattened { name: field, ty } => {
                let nested = select_type(ty);
                definitions.push(quote! {
                    #[serde(default, skip_serializing_if = "::dbkit::SelectRow::is_empty")]
                    pub #field: #nested
                });
                emptiness.push(quote! { ::dbkit::SelectRow::is_empty(&self.#field) });
                decoders.push(quote! {
                    #field: <#nested as ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow>>::from_row(row)?
                });
            }
            Column::Scalar {
                name: field, ty, ..
            } => {
                definitions.push(quote! {
                    #[serde(default, skip_serializing_if = "::dbkit::Selected::is_absent")]
                    pub #field: ::dbkit::Selected<#ty>
                });
                emptiness.push(quote! { self.#field.is_absent() });
                let literal = field.to_string();
                let literal = literal.trim_start_matches("r#");
                decoders.push(quote! {
                    #field: ::dbkit::selected(row, #literal)?
                });
            }
        }
    }

    let empty = if emptiness.is_empty() {
        quote!(true)
    } else {
        quote!(#(#emptiness)&&*)
    };

    Ok(quote! {
        #[doc = #doc]
        #[derive(
            Debug,
            Clone,
            Default,
            PartialEq,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        pub struct #select #generics {
            #(#definitions),*
        }

        impl #generics ::dbkit::SelectRow for #select {
            /// Whether every column in the row is absent.
            fn is_empty(&self) -> bool {
                #empty
            }
        }

        impl<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> for #select {
            fn from_row(
                row: &'r ::sqlx::postgres::PgRow,
            ) -> ::core::result::Result<Self, ::sqlx::Error> {
                ::core::result::Result::Ok(Self {
                    #(#decoders),*
                })
            }
        }

        impl ::dbkit::Selectable for #name {
            type Select = #select;
        }
    })
}

/// The select type of a flattened field: its last path segment plus `Select`.
fn select_type(ty: &Type) -> Type {
    let Type::Path(path) = ty else {
        return ty.clone();
    };

    let mut path = path.clone();
    if let Some(segment) = path.path.segments.last_mut() {
        segment.ident = format_ident!("{}Select", segment.ident);
    }

    Type::Path(path)
}
