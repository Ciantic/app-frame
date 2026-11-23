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
/// `impl dbkit::Model for ...`, carrying them as `Model::meta()`.
///
/// Accepted on the struct: `#[db(table = "...")]` for a table row, or
/// `#[db(fragment)]` for a group of columns that another model embeds. The two
/// generate different `Model::meta()` values, `Meta::Table` and
/// `Meta::Fragment`, so a fragment never carries a table name or key.
/// Accepted on a field: `#[db(pgtype = "...")]`, `#[db(pgdefault = "...")]`,
/// `#[db(primary_key)]`, `#[db(version)]`, `#[db(created_at)]`,
/// `#[db(updated_at)]`, `#[db(query_ops(eq, neq, ...))]`, and
/// `#[db(flatten)]`, which splices the field type's `meta().columns` into the
/// table's columns.
///
/// `query_ops` records which comparisons a column supports in
/// `ColumnMeta::query_ops`; it is metadata, not query code.
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
/// The generated struct is `Serialize + Deserialize + garde::Validate`. A
/// patch field whose payload type can carry rules derives a `#[garde(dive)]`,
/// so `Patch::Set` values are validated while `Patch::Absent` and
/// `Patch::Null` pass.
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

    if annotations.shape == annotations::Shape::Fragment {
        let columns: Vec<Expr> = annotations.columns.iter().filter_map(column_meta).collect();

        return Ok(syn::parse_quote! {
            #[automatically_derived]
            impl #impl_generics ::dbkit::Model for #name #type_generics #where_clause {
                fn meta() -> ::dbkit::Meta {
                    static META: ::dbkit::Meta = ::dbkit::Meta::Fragment(
                        &::dbkit::TableFragment { columns: &[#(#columns),*] },
                    );

                    META
                }
            }
        });
    }

    let annotations::Shape::Table(table) = &annotations.shape else {
        unreachable!("the fragment shape returned above");
    };
    // `Vec` because a `const` cannot splice another type's columns; the list
    // is built once, then leaked so `TableMeta` stays a slice of `'static`.
    let push_columns: Vec<TokenStream2> = annotations
        .columns
        .iter()
        .map(|column| match column {
            annotations::Column::Scalar { .. } => {
                let meta = column_meta(column).expect("scalar columns have metadata");
                syn::parse_quote!(columns.push(#meta);)
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
                }
            },
        })
        .collect();
    let primary_key: Vec<String> = annotations
        .primary_key
        .iter()
        .map(ToString::to_string)
        .collect();
    let version = optional(annotations.version.as_ref());
    let created_at = optional(annotations.created_at.as_ref());
    let updated_at = optional(annotations.updated_at.as_ref());

    Ok(syn::parse_quote! {
        #[automatically_derived]
        impl #impl_generics ::dbkit::Model for #name #type_generics #where_clause {
            fn meta() -> ::dbkit::Meta {
                static META: ::std::sync::OnceLock<::dbkit::TableMeta> =
                    ::std::sync::OnceLock::new();

                ::dbkit::Meta::Table(META.get_or_init(|| {
                    let mut columns: ::std::vec::Vec<::dbkit::ColumnMeta> =
                        ::std::vec::Vec::new();
                    #(#push_columns)*

                    ::dbkit::TableMeta {
                        table: #table,
                        primary_key: &[#(#primary_key),*],
                        version: #version,
                        created_at: #created_at,
                        updated_at: #updated_at,
                        columns: ::std::boxed::Box::leak(columns.into_boxed_slice()),
                    }
                }))
            }
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

fn column_meta(column: &annotations::Column) -> Option<Expr> {
    let annotations::Column::Scalar {
        name,
        nullable,
        pg_type,
        pg_default,
        query_ops,
        ..
    } = column
    else {
        return None;
    };

    let name = name.to_string();
    let pg_default = optional(pg_default.as_deref());
    let query_ops: Vec<Expr> = query_ops
        .iter()
        .map(|filter| {
            let variant = syn::Ident::new(filter.variant(), proc_macro2::Span::call_site());
            syn::parse_quote!(::dbkit::FilterOperator::#variant)
        })
        .collect();

    Some(syn::parse_quote!(
        ::dbkit::ColumnMeta {
            name: #name,
            nullable: #nullable,
            pg_type: #pg_type,
            pg_default: #pg_default,
            query_ops: &[#(#query_ops),*],
        }
    ))
}
