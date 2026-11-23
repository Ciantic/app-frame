//! Derive macros for `dbkit`.
//!
//! `DbModel` parses the `#[db(...)]` annotations and emits
//! `impl dbkit::Model`, whose `META` const describes the table at runtime.
//! `DbPatch` reads the same annotations and emits the model's patch input
//! type.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;
use syn::Expr;

mod annotations;
mod derive_patch;

/// Reads the `#[db(...)]` annotations of a model struct and generates
/// `impl dbkit::Model for ...`, carrying them as `Model::META`.
///
/// Accepted on the struct: `#[db(table = "...")]`.
/// Accepted on a field: `#[db(pgtype = "...")]`, `#[db(pgdefault = "...")]`,
/// `#[db(primary_key)]`, `#[db(version)]`, `#[db(created_at)]`,
/// `#[db(updated_at)]`.
///
/// `table`, `primary_key` and one `pgtype` per field are required; a table
/// without a version or audit column omits those role annotations. `pgtype`
/// is the column type as DDL spells it (`uuid`, `text`, `bigint`,
/// `timestamptz`, `varchar(255)`, `numeric(10,2)`, `text[]`), so it can be
/// checked against the database and later generate `schema.sql`. `pgdefault`
/// is the optional column default in the same notation (`now()`, `0`,
/// `'{}'::jsonb`), which the schema test compares with the database and which
/// marks the column as safe to omit on insert.
#[proc_macro_derive(DbModel, attributes(db))]
pub fn derive_db_model(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates the patch input type of a model, `<Model>Patch`.
///
/// Reads the same `#[db(...)]` annotations as `DbModel`. The primary key is a
/// required field of the patch, taken as it is, and so is the version when the
/// model has one; every other business column becomes a `dbkit::Patch<T>`
/// field, so a missing key leaves the column unchanged and an explicit `null`
/// clears a nullable one. Audit columns (`created_at`, `updated_at`) are not
/// patchable and are omitted.
///
/// The generated struct is `Serialize + Deserialize`, and it has `schema()`
/// and `validate_and_parse()` like a `ZodSchema` model: `Patch::Set` values are
/// validated by the column's own schema.
#[proc_macro_derive(DbPatch, attributes(db))]
pub fn derive_db_patch(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_patch::expand(&input) {
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
            let pg_default = optional(column.pg_default.as_deref());
            syn::parse_quote!(
                ::dbkit::ColumnMeta {
                    name: #name,
                    nullable: #nullable,
                    pg_type: #pg_type,
                    pg_default: #pg_default,
                }
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

fn optional<T: ToString>(name: Option<T>) -> Expr {
    match name {
        Some(name) => {
            let name = name.to_string();
            syn::parse_quote!(::core::option::Option::Some(#name))
        }
        None => syn::parse_quote!(::core::option::Option::None),
    }
}
