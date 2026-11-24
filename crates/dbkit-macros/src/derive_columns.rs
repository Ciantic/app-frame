//! Generation of a model's selectable columns.
//!
//! The generated enum has one variant per column declared on the model, named
//! in PascalCase and carrying the column name as its serde representation, so
//! a request names a column as the database spells it and an unknown name fails
//! to deserialize. A variant is the only proof that a column exists, and its
//! name is a `&'static str`, so a query layer can build SQL from it without
//! ever formatting a request string into the statement.
//!
//! Role columns are selectable too: a patch never touches the key, the version
//! or the audit columns, but a read needs them. A `#[db(flatten)]` field adds
//! no variants, because its type's columns are unknown at macro expansion; they
//! are spliced into the model's metadata at runtime instead.

use crate::annotations;
use crate::annotations::Column;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Ident;

pub(crate) fn expand(
    input: &DeriveInput,
    annotations: &annotations::Annotations,
) -> syn::Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "`DbModel` cannot generate a column enum for a struct with generic parameters; the \
             column list is one `static` per type",
        ));
    }

    let name = &input.ident;
    let visibility = &input.vis;
    let columns = format_ident!("{name}Column");
    let doc = format!("A selectable column of [`{name}`].");
    let all_doc = "Every column declared on the model itself, in declaration order.";
    let name_doc = "The column name as the database spells it.";
    let type_doc = "The column type as DDL spells it.";
    let default_doc = "The column default as DDL spells it, or `None` when the column has none.";
    let nullable_doc = "Whether the column is nullable.";
    let ops_doc = "The comparisons the column may be queried with, in declaration order.";

    let mut variants = Vec::new();
    let mut all = Vec::new();
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut defaults = Vec::new();
    let mut nullability = Vec::new();
    let mut operators = Vec::new();

    for column in &annotations.columns {
        let Column::Scalar {
            name,
            ty,
            nullable,
            pg_type,
            pg_default,
            query_ops,
        } = column
        else {
            continue;
        };

        let variant = variant(name);
        let column_name = column_name(name);
        let ty = annotations::underlying(ty);
        let pg_type = match pg_type {
            annotations::PgType::Declared(pg_type) => quote!(#pg_type),
            annotations::PgType::Inherited => quote!(<#ty as ::dbkit::DbType>::PG_TYPE),
        };
        let pg_default = match pg_default {
            annotations::PgDefault::Declared(default) => {
                quote!(::core::option::Option::Some(#default))
            }
            annotations::PgDefault::None => quote!(::core::option::Option::None),
            annotations::PgDefault::Inherited => quote!(<#ty as ::dbkit::DbType>::PG_DEFAULT),
        };
        let query_ops: Vec<TokenStream> = query_ops
            .iter()
            .map(|operator| {
                let variant = Ident::new(operator.variant(), proc_macro2::Span::call_site());
                quote!(::dbkit::FilterOperator::#variant)
            })
            .collect();

        variants.push(quote! {
            #[serde(rename = #column_name)]
            #variant
        });
        all.push(quote!(Self::#variant));
        names.push(quote!(Self::#variant => #column_name));
        types.push(quote!(Self::#variant => #pg_type));
        defaults.push(quote!(Self::#variant => #pg_default));
        nullability.push(quote!(Self::#variant => #nullable));
        operators.push(quote!(Self::#variant => &[#(#query_ops),*]));
    }

    Ok(quote! {
        #[doc = #doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            ::serde::Serialize,
            ::serde::Deserialize,
        )]
        #visibility enum #columns {
            #(#variants),*
        }

        impl #columns {
            #[doc = #all_doc]
            pub const ALL: &'static [Self] = &[#(#all),*];

            #[doc = #name_doc]
            pub const fn name(self) -> &'static str {
                match self {
                    #(#names),*
                }
            }

            #[doc = #type_doc]
            pub const fn pg_type(self) -> &'static str {
                match self {
                    #(#types),*
                }
            }

            #[doc = #default_doc]
            pub const fn pg_default(self) -> ::core::option::Option<&'static str> {
                match self {
                    #(#defaults),*
                }
            }

            #[doc = #nullable_doc]
            pub const fn nullable(self) -> bool {
                match self {
                    #(#nullability),*
                }
            }

            #[doc = #ops_doc]
            pub const fn query_ops(self) -> &'static [::dbkit::FilterOperator] {
                match self {
                    #(#operators),*
                }
            }
        }

        impl ::dbkit::Column for #columns {
            type Model = #name;

            const ALL: &'static [Self] = #columns::ALL;

            fn name(self) -> &'static str {
                #columns::name(self)
            }

            fn pg_type(self) -> &'static str {
                #columns::pg_type(self)
            }

            fn pg_default(self) -> ::core::option::Option<&'static str> {
                #columns::pg_default(self)
            }

            fn nullable(self) -> bool {
                #columns::nullable(self)
            }

            fn query_ops(self) -> &'static [::dbkit::FilterOperator] {
                #columns::query_ops(self)
            }
        }
    })
}

/// The column name as the database spells it, without a raw-identifier prefix.
fn column_name(column: &Ident) -> String {
    column.to_string().trim_start_matches("r#").to_owned()
}

/// `email_verified_at` becomes `EmailVerifiedAt`.
pub(crate) fn variant(column: &Ident) -> Ident {
    let span = column.span();
    let column = column_name(column);
    let mut variant = String::with_capacity(column.len());
    let mut capitalize = true;

    for character in column.chars() {
        if character == '_' {
            capitalize = true;
        } else if capitalize {
            variant.extend(character.to_uppercase());
            capitalize = false;
        } else {
            variant.push(character);
        }
    }

    Ident::new(&variant, span)
}
