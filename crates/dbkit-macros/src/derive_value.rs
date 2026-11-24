//! Generation of the borrow conversion of a value type.
//!
//! A column handle takes `impl Into<T>`, so `c.id.eq(&id)` needs
//! `From<&OrderId> for OrderId`. The orphan rule rejects a blanket
//! `impl<T: Clone> From<&T> for T`, so the newtype declares the conversion with
//! this derive instead. The conversion clones, so the type must be `Clone`; a
//! newtype that does not want to copy itself simply does not derive `DbValue`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;

pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "`DbValue` can only be derived for a one-field tuple struct",
        ));
    };
    let Fields::Unnamed(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "`DbValue` requires a one-field tuple struct, such as `OrderId(pub Uuid)`",
        ));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "`DbValue` requires exactly one field",
        ));
    }

    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::core::convert::From<&#name #type_generics>
            for #name #type_generics #where_clause
        {
            fn from(value: &#name #type_generics) -> Self {
                ::core::clone::Clone::clone(value)
            }
        }
    })
}
