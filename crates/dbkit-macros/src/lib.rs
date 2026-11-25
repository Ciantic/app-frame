//! Derive macros for `dbkit`: model metadata, patch, insert, delete, value and select types; see `docs/dbkit/`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Expr;

mod annotations;
mod derive_columns;
mod derive_delete;
mod derive_insert;
mod derive_keys;
mod derive_patch;
mod derive_select;
mod derive_value;

/// Reads a model's `#[db(...)]` annotations and generates `impl dbkit::Model`; see `docs/dbkit/annotations.md`.
#[proc_macro_derive(DbModel, attributes(db))]
pub fn derive_db_model(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Records a value type's `pgtype` and optional `pgdefault` in `impl dbkit::DbType`; see `docs/dbkit/annotations.md`.
#[proc_macro_derive(DbType, attributes(db))]
pub fn derive_db_type(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match expand_type(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates `<Model>Patch` and `Model::patch`; see `docs/dbkit/patch-builder.md`.
#[proc_macro_derive(DbPatch, attributes(db))]
pub fn derive_db_patch(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_patch::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates `<Model>Insert` and `Model::insert`; see `docs/dbkit/insert-builder.md`.
#[proc_macro_derive(DbInsert, attributes(db))]
pub fn derive_db_insert(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_insert::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Adds `Model::delete(entries)`; see `docs/dbkit/delete-builder.md`.
#[proc_macro_derive(DbDelete, attributes(db))]
pub fn derive_db_delete(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_delete::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates `impl From<&T> for T` for a one-field tuple struct, so a column handle takes `&value`; T must be `Clone`.
#[proc_macro_derive(DbValue)]
pub fn derive_db_value(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_value::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates `<Model>Select`, whose `Selected<T>` fields distinguish absent columns; see `docs/dbkit/annotations.md`.
#[proc_macro_derive(DbSelect, attributes(db))]
pub fn derive_db_select(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_select::expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let annotations = annotations::parse(input)?;
    let name = &input.ident;
    let columns_type = format_ident!("{name}Column");
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    // `Vec` because a `const` cannot splice another model's columns; built once and leaked for `'static`.
    let push_columns: Vec<TokenStream2> = annotations
        .columns
        .iter()
        .map(|column| match column {
            annotations::Column::Scalar { name, .. } => {
                let variant = derive_columns::variant(name);
                syn::parse_quote! {
                    columns.push(
                        &#columns_type::#variant
                            as &'static dyn ::dbkit::ErasedColumn
                    );
                }
            }
            annotations::Column::Flattened { name, ty } => syn::parse_quote! {
                match <#ty as ::dbkit::Model>::meta() {
                    ::dbkit::Meta::Fragment(fragment) => {
                        columns.extend_from_slice(fragment.columns);
                    }
                    ::dbkit::Meta::Table(meta) => panic!(
                        "`#[db(flatten)]` field `{}` has type `{}`, which is a table; \
                         flattened fields must be `#[db(fragment)]` structs",
                        ::core::stringify!(#name),
                        meta.table,
                    ),
                    ::dbkit::Meta::SubQuery(_) => panic!(
                        "`#[db(flatten)]` field `{}` has type `{}`, which is a subquery; \
                         flattened fields must be `#[db(fragment)]` structs",
                        ::core::stringify!(#name),
                        ::core::stringify!(#ty),
                    ),
                }
            },
        })
        .collect();

    let metadata = match &annotations.shape {
        annotations::Shape::Fragment => quote! {
            static META: ::std::sync::OnceLock<::dbkit::TableFragment> =
                ::std::sync::OnceLock::new();

            ::dbkit::Meta::Fragment(META.get_or_init(|| {
                let mut columns: ::std::vec::Vec<&'static dyn ::dbkit::ErasedColumn> =
                    ::std::vec::Vec::new();
                #(#push_columns)*

                ::dbkit::TableFragment {
                    columns: ::std::boxed::Box::leak(columns.into_boxed_slice()),
                }
            }))
        },
        annotations::Shape::SubQuery(sql) => quote! {
            static META: ::std::sync::OnceLock<::dbkit::SubQuery> =
                ::std::sync::OnceLock::new();

            ::dbkit::Meta::SubQuery(META.get_or_init(|| {
                let mut columns: ::std::vec::Vec<&'static dyn ::dbkit::ErasedColumn> =
                    ::std::vec::Vec::new();
                #(#push_columns)*

                ::dbkit::SubQuery {
                    sql: #sql,
                    columns: ::std::boxed::Box::leak(columns.into_boxed_slice()),
                }
            }))
        },
        annotations::Shape::Table(table) => {
            let primary_key: Vec<String> = annotations
                .primary_key
                .iter()
                .map(ToString::to_string)
                .collect();
            let version = optional(annotations.version.as_ref());
            let created_at = optional(annotations.created_at.as_ref());
            let updated_at = optional(annotations.updated_at.as_ref());
            let tenant = optional(annotations.tenant.as_ref());

            quote! {
                static META: ::std::sync::OnceLock<::dbkit::TableMeta> =
                    ::std::sync::OnceLock::new();

                ::dbkit::Meta::Table(META.get_or_init(|| {
                    let mut columns: ::std::vec::Vec<&'static dyn ::dbkit::ErasedColumn> =
                        ::std::vec::Vec::new();
                    #(#push_columns)*

                    ::dbkit::TableMeta {
                        table: #table,
                        primary_key: &[#(#primary_key),*],
                        version: #version,
                        created_at: #created_at,
                        updated_at: #updated_at,
                        tenant: #tenant,
                        columns: ::std::boxed::Box::leak(columns.into_boxed_slice()),
                    }
                }))
            }
        }
    };

    let columns = derive_columns::expand(input, &annotations)?;
    let keys = derive_keys::expand(input, &annotations)?;
    let tenant_owned = match (&annotations.shape, &annotations.tenant) {
        (annotations::Shape::Table(_), Some(tenant)) => {
            let column =
                syn::LitStr::new(tenant.to_string().trim_start_matches("r#"), tenant.span());

            quote! {
                #[automatically_derived]
                impl #impl_generics ::dbkit::TenantOwned for #name #type_generics
                    #where_clause
                {
                    const TENANT_COLUMN: &'static str = #column;
                }
            }
        }
        _ => quote!(),
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::dbkit::Model for #name #type_generics #where_clause {
            fn meta() -> ::dbkit::Meta {
                #metadata
            }
        }

        #keys

        #columns

        #tenant_owned
    })
}

fn optional<T: ToString>(name: Option<T>) -> Expr {
    match name {
        Some(name) => {
            let name = name.to_string();
            syn::parse_quote!(::core::option::Option::Some(#name))
        }
        None => syn::parse_quote!(::core::option::Option::None),
    }
}

fn expand_type(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let annotations = annotations::parse_type(input)?;
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let pg_type = &annotations.pg_type;
    let pg_default = optional(annotations.pg_default.as_deref());

    Ok(syn::parse_quote! {
        #[automatically_derived]
        impl #impl_generics ::dbkit::DbType for #name #type_generics #where_clause {
            const PG_TYPE: &'static str = #pg_type;
            const PG_DEFAULT: ::core::option::Option<&'static str> = #pg_default;
        }
    })
}
