//! Derive of `Model::delete`: implements `dbkit::DeleteKey` for `{Model}PKV` and batches deletes in one transaction.

use crate::annotations;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let annotations = annotations::parse(input)?;

    match &annotations.shape {
        annotations::Shape::Table(_) => {}
        annotations::Shape::SubQuery(_) => {
            return Err(syn::Error::new_spanned(
                input,
                "`DbDelete` cannot be derived for a `#[db(subquery = \"...\")]` struct; a \
                 subquery is read-only",
            ));
        }
        annotations::Shape::Fragment => {
            return Err(syn::Error::new_spanned(
                input,
                "`DbDelete` cannot be derived for a `#[db(fragment)]` struct; a fragment has no \
                 table to delete from",
            ));
        }
    }

    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let key = format_ident!("{name}PKV");
    let primary_key = &annotations.primary_key;
    let version = match &annotations.version {
        Some(version) => quote!(::core::option::Option::Some(&self.#version)),
        None => quote!(::core::option::Option::None),
    };

    Ok(quote! {
        #[automatically_derived]
        impl ::dbkit::DeleteKey for #key {
            type Model = #name;

            fn key(&self) -> ::std::vec::Vec<&dyn ::dbkit::Bind> {
                ::std::vec![#(&self.#primary_key),*]
            }

            fn version(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                #version
            }
        }

        impl #impl_generics #name #type_generics #where_clause {
            #[doc = "A transaction that deletes the rows the given keys name."]
            pub fn delete(
                entries: impl ::core::iter::IntoIterator<Item = #key>,
            ) -> ::dbkit::DeleteQuery<#key> {
                ::dbkit::DeleteQuery::new(entries)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_subquery() {
        let input = syn::parse_str::<DeriveInput>(
            r#"
            #[db(subquery = "SELECT id FROM widget")]
            struct WidgetSummary {
                #[db(pgtype = "uuid")]
                id: Uuid,
            }
        "#,
        )
        .unwrap();

        let message = expand(&input).unwrap_err().to_string();

        assert!(message.contains("a subquery is read-only"), "{message}");
    }

    #[test]
    fn rejects_a_fragment() {
        let input = syn::parse_str::<DeriveInput>(
            r#"
            #[db(fragment)]
            struct Size {
                #[db(pgtype = "int4")]
                width: i32,
            }
        "#,
        )
        .unwrap();

        let message = expand(&input).unwrap_err().to_string();

        assert!(message.contains("a fragment has no table"), "{message}");
    }
}
