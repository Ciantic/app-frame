//! Derive macros for `dbkit`.
//!
//! `DbModel` parses the `#[db(...)]` annotations and emits
//! `impl dbkit::Model`, whose `META` const describes the table at runtime.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;
use syn::Expr;
use syn::Ident;

mod annotations;

/// Reads the `#[db(...)]` annotations of a model struct and generates
/// `impl dbkit::Model for ...`, carrying them as `Model::META`.
///
/// Accepted on the struct: `#[db(table = "...")]`.
/// Accepted on a field: `#[db(pgtype = "...")]`, `#[db(primary_key)]`,
/// `#[db(version)]`, `#[db(created_at)]`, `#[db(updated_at)]`.
///
/// `table`, `primary_key` and one `pgtype` per field are required; a table
/// without a version or audit column omits those role annotations. `pgtype`
/// is the column type as DDL spells it (`uuid`, `text`, `bigint`,
/// `timestamptz`, `varchar(255)`, `numeric(10,2)`, `text[]`), so it can be
/// checked against the database and later generate `schema.sql`.
#[proc_macro_derive(DbModel, attributes(db))]
pub fn derive_db_model(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let annotations = annotations::parse(input)?;
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let table = &annotations.table;
    let primary_key: Vec<String> = annotations
        .primary_key
        .iter()
        .map(ToString::to_string)
        .collect();
    let columns: Vec<Expr> = annotations
        .columns
        .iter()
        .map(|column| {
            let name = column.name.to_string();
            let nullable = column.nullable;
            let pg_type = &column.pg_type;
            syn::parse_quote!(
                ::dbkit::ColumnMeta { name: #name, nullable: #nullable, pg_type: #pg_type }
            )
        })
        .collect();
    let version = optional(annotations.version.as_ref());
    let created_at = optional(annotations.created_at.as_ref());
    let updated_at = optional(annotations.updated_at.as_ref());

    Ok(syn::parse_quote! {
        #[automatically_derived]
        impl #impl_generics ::dbkit::Model for #name #type_generics #where_clause {
            const META: ::dbkit::TableMeta = ::dbkit::TableMeta {
                table: #table,
                primary_key: &[#(#primary_key),*],
                version: #version,
                created_at: #created_at,
                updated_at: #updated_at,
                columns: &[#(#columns),*],
            };
        }
    })
}

fn optional(name: Option<&Ident>) -> Expr {
    match name {
        Some(name) => {
            let name = name.to_string();
            syn::parse_quote!(::core::option::Option::Some(#name))
        }
        None => syn::parse_quote!(::core::option::Option::None),
    }
}
