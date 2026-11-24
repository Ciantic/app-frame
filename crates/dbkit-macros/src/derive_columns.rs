//! Generation of a model's selectable columns.
//!
//! The generated enum has one variant per column declared on the model, named
//! in PascalCase. A `#[db(flatten)]` field contributes a variant wrapping the
//! fragment's own enum, so the fragment's columns stay typed and selectable as
//! `<Model>Column::<Field>(<Fragment>Column::<Column>)` even though the parent
//! enum cannot name them.
//!
//! Serde is written by hand rather than derived: the wire form of a column is
//! its database name, which a wrapped variant only knows at runtime, and an
//! unknown name must report the column list. A variant is the only proof that a
//! column exists, and its name is a `&'static str`, so a query layer can build
//! SQL from it without ever formatting a request string into the statement.
//!
//! Role columns are selectable too: a patch never touches the key, the version
//! or the audit columns, but a read needs them.

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

    let mut variants = Vec::new();
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut defaults = Vec::new();
    let mut nullability = Vec::new();
    let mut operators = Vec::new();
    let mut literals = Vec::new();
    let mut flattened = Vec::new();
    let mut tags = Vec::new();
    let mut handles = Vec::new();

    for column in &annotations.columns {
        match column {
            Column::Scalar {
                name: field,
                ty,
                nullable,
                pg_type,
                pg_default,
                query_ops,
            } => {
                let variant = variant(field);
                let literal = column_name(field);
                let ty = annotations::underlying(ty);
                let declared = query_ops;
                let pg_type = match pg_type {
                    annotations::PgType::Declared(pg_type) => quote!(#pg_type),
                    annotations::PgType::Inherited => quote!(<#ty as ::dbkit::DbType>::PG_TYPE),
                };
                let pg_default = match pg_default {
                    annotations::PgDefault::Declared(default) => {
                        quote!(::core::option::Option::Some(#default))
                    }
                    annotations::PgDefault::None => quote!(::core::option::Option::None),
                    annotations::PgDefault::Inherited => {
                        quote!(<#ty as ::dbkit::DbType>::PG_DEFAULT)
                    }
                };
                let query_ops: Vec<TokenStream> = query_ops
                    .iter()
                    .map(|operator| {
                        let operator =
                            Ident::new(operator.variant(), proc_macro2::Span::call_site());
                        quote!(::dbkit::FilterOperator::#operator)
                    })
                    .collect();

                let tag = tag(name, &variant);
                let capabilities: Vec<Ident> = declared
                    .iter()
                    .filter_map(|operator| capability(*operator))
                    .map(|capability| Ident::new(capability, proc_macro2::Span::call_site()))
                    .collect();

                // A tag is generic over the column enum, because a fragment's
                // column is spliced into whichever parent flattened it.
                tags.push(quote! {
                    #[doc(hidden)]
                    #visibility struct #tag;

                    #(impl<C: ::dbkit::Column> ::dbkit::#capabilities<C> for #tag {})*
                });
                handles.push((
                    field.clone(),
                    quote!(::dbkit::ColumnRef<#tag, C, #ty>),
                    quote!(::dbkit::ColumnRef::new(map(#columns::#variant))),
                ));

                variants.push(quote!(#variant));
                names.push(quote!(Self::#variant => #literal));
                literals.push(quote!(#literal => ::core::option::Option::Some(Self::#variant)));
                types.push(quote!(Self::#variant => #pg_type));
                defaults.push(quote!(Self::#variant => #pg_default));
                nullability.push(quote!(Self::#variant => #nullable));
                operators.push(quote!(Self::#variant => &[#(#query_ops),*]));
            }
            Column::Flattened { name: field, ty } => {
                let variant = variant(field);
                let handle = handle_type(ty);
                let ty = column_type(ty);

                handles.push((
                    field.clone(),
                    quote!(#handle<C>),
                    quote!(<#handle<D>>::map(|column| map(#columns::#variant(column)))),
                ));

                variants.push(quote!(#variant(#ty)));
                names.push(quote!(Self::#variant(inner) => ::dbkit::Column::name(inner)));
                types.push(quote!(Self::#variant(inner) => ::dbkit::Column::pg_type(inner)));
                defaults.push(quote!(Self::#variant(inner) => ::dbkit::Column::pg_default(inner)));
                nullability.push(quote!(Self::#variant(inner) => ::dbkit::Column::nullable(inner)));
                operators.push(quote!(Self::#variant(inner) => ::dbkit::Column::query_ops(inner)));
                flattened.push((variant, ty));
            }
        }
    }

    let nested_lookups: Vec<TokenStream> = flattened
        .iter()
        .map(|(variant, ty)| {
            quote! {
                if let ::core::option::Option::Some(inner) =
                    <#ty as ::dbkit::Column>::from_name(name)
                {
                    return ::core::option::Option::Some(Self::#variant(inner));
                }
            }
        })
        .collect();

    let all = all_function(&columns, &annotations.columns);

    let doc = format!("A selectable column of [`{name}`].");
    let all_doc = "Every column, in declaration order, with a flattened field's columns at the \
                   field's position.";
    let name_doc = "The column name as the database spells it.";
    let from_name_doc = "The column of that name, or `None` when the model has no such column.";
    let type_doc = "The column type as DDL spells it.";
    let default_doc = "The column default as DDL spells it, or `None` when the column has none.";
    let nullable_doc = "Whether the column is nullable.";
    let ops_doc = "The comparisons the column may be queried with, in declaration order.";

    let handles_type = format_ident!("{name}Columns");
    let handle_doc =
        format!("The typed handles of [`{name}`], one per column, for building a `Query`.");
    let map_doc = "The same handles, with every column replaced by `map(column)`; a model that \
                   flattens this one splices its own column enum in.";
    let handle_fields: Vec<&Ident> = handles.iter().map(|(field, _, _)| field).collect();
    let handle_types: Vec<&TokenStream> = handles.iter().map(|(_, ty, _)| ty).collect();
    let handle_inits: Vec<&TokenStream> = handles.iter().map(|(_, _, init)| init).collect();

    Ok(quote! {
        #(#tags)*

        #[doc = #handle_doc]
        #visibility struct #handles_type<C: ::dbkit::Column = #columns> {
            #(pub #handle_fields: #handle_types),*
        }

        impl<C: ::dbkit::Column> #handles_type<C> {
            #[doc = #map_doc]
            pub fn map<D: ::dbkit::Column>(
                map: impl ::core::ops::Fn(#columns) -> D,
            ) -> #handles_type<D> {
                #handles_type::<D> {
                    #(#handle_fields: #handle_inits),*
                }
            }
        }

        impl #name {
            #[doc = #handle_doc]
            pub fn columns() -> #handles_type<#columns> {
                <#handles_type<#columns>>::map(|column| column)
            }

            #[doc = "A query of this table, with no filters."]
            pub const fn query() -> ::dbkit::Query<#columns> {
                ::dbkit::Query::new()
            }
        }

        #[doc = #doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #visibility enum #columns {
            #(#variants),*
        }

        impl #columns {
            #[doc = #all_doc]
            #all

            #[doc = #name_doc]
            pub fn name(self) -> &'static str {
                match self {
                    #(#names),*
                }
            }

            #[doc = #from_name_doc]
            pub fn from_name(name: &str) -> ::core::option::Option<Self> {
                #(#nested_lookups)*

                match name {
                    #(#literals,)*
                    _ => ::core::option::Option::None,
                }
            }

            #[doc = #type_doc]
            pub fn pg_type(self) -> &'static str {
                match self {
                    #(#types),*
                }
            }

            #[doc = #default_doc]
            pub fn pg_default(self) -> ::core::option::Option<&'static str> {
                match self {
                    #(#defaults),*
                }
            }

            #[doc = #nullable_doc]
            pub fn nullable(self) -> bool {
                match self {
                    #(#nullability),*
                }
            }

            #[doc = #ops_doc]
            pub fn query_ops(self) -> &'static [::dbkit::FilterOperator] {
                match self {
                    #(#operators),*
                }
            }
        }

        impl ::dbkit::Column for #columns {
            type Model = #name;

            fn all() -> &'static [Self] {
                #columns::all()
            }

            fn name(self) -> &'static str {
                #columns::name(self)
            }

            fn from_name(name: &str) -> ::core::option::Option<Self> {
                #columns::from_name(name)
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

        impl ::serde::Serialize for #columns {
            fn serialize<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for #columns {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                struct ColumnVisitor;

                impl<'de> ::serde::de::Visitor<'de> for ColumnVisitor {
                    type Value = #columns;

                    fn expecting(
                        &self,
                        formatter: &mut ::core::fmt::Formatter<'_>,
                    ) -> ::core::fmt::Result {
                        formatter.write_str("a column name")
                    }

                    fn visit_str<E: ::serde::de::Error>(
                        self,
                        name: &str,
                    ) -> ::core::result::Result<#columns, E> {
                        #columns::from_name(name).ok_or_else(|| {
                            let known: ::std::vec::Vec<&'static str> = #columns::all()
                                .iter()
                                .map(|column| ::dbkit::Column::name(*column))
                                .collect();

                            E::custom(::std::format_args!(
                                "unknown column `{name}`; expected one of: {}",
                                known.join(", "),
                            ))
                        })
                    }
                }

                deserializer.deserialize_str(ColumnVisitor)
            }
        }
    })
}

/// `all()` is a promoted slice when the model has no flattened field, and a
/// cached `Vec` when it does: a nested enum's variants cannot be listed at
/// expansion, so those columns are appended at run time.
fn all_function(columns: &Ident, annotations: &[Column]) -> TokenStream {
    let flattened = annotations
        .iter()
        .any(|column| matches!(column, Column::Flattened { .. }));

    if !flattened {
        let variants: Vec<TokenStream> = annotations
            .iter()
            .filter_map(|column| match column {
                Column::Scalar { name, .. } => Some(variant(name)),
                Column::Flattened { .. } => None,
            })
            .map(|variant| quote!(#columns::#variant))
            .collect();

        return quote! {
            pub fn all() -> &'static [Self] {
                &[#(#variants),*]
            }
        };
    }

    let entries: Vec<TokenStream> = annotations
        .iter()
        .map(|column| match column {
            Column::Scalar { name, .. } => {
                let variant = variant(name);
                quote!(all.push(#columns::#variant);)
            }
            Column::Flattened { name, ty } => {
                let variant = variant(name);
                let ty = column_type(ty);
                quote! {
                    all.extend(
                        <#ty as ::dbkit::Column>::all().iter().copied().map(#columns::#variant),
                    );
                }
            }
        })
        .collect();

    quote! {
        pub fn all() -> &'static [Self] {
            static ALL: ::std::sync::OnceLock<::std::vec::Vec<#columns>> =
                ::std::sync::OnceLock::new();

            ALL.get_or_init(|| {
                let mut all: ::std::vec::Vec<#columns> = ::std::vec::Vec::new();
                #(#entries)*
                all
            })
        }
    }
}

/// The handle struct of a flattened field's type: the type with `Columns`
/// appended to its last path segment, so `size: Size` becomes `SizeColumns`.
fn handle_type(ty: &syn::Type) -> syn::Type {
    rename(ty, "Columns")
}

/// The zero-sized tag type of one column, unique per model, so
/// `Order` and `status` name `OrderStatusTag`.
fn tag(model: &Ident, variant: &Ident) -> Ident {
    format_ident!("{model}{variant}Tag")
}

/// The capability trait an operator's handle method is gated on, or `None`
/// when the operator takes no value and so needs no declaration.
fn capability(operator: annotations::FilterOperator) -> Option<&'static str> {
    use annotations::FilterOperator::*;

    Some(match operator {
        Eq => "HasEq",
        Neq => "HasNeq",
        Gt => "HasGt",
        Gte => "HasGte",
        Lt => "HasLt",
        Lte => "HasLte",
        Like => "HasLike",
        ILike => "HasILike",
        NotLike => "HasNotLike",
        NotILike => "HasNotILike",
        Any => "HasAny",
        NotAny => "HasNotAny",
        Contains => "HasContains",
        NotContains => "HasNotContains",
        Overlaps => "HasOverlaps",
        NotOverlaps => "HasNotOverlaps",
        ContainsAll => "HasContainsAll",
        NotContainsAll => "HasNotContainsAll",
        IsNull | IsNotNull => return None,
        Between => "HasBetween",
        NotBetween => "HasNotBetween",
    })
}

/// The column enum of a flattened field's type: the type with `Column`
/// appended to its last path segment, so `size: Size` becomes `SizeColumn`.
fn column_type(ty: &syn::Type) -> syn::Type {
    let syn::Type::Path(path) = ty else {
        return ty.clone();
    };

    rename(&syn::Type::Path(path.clone()), "Column")
}

/// The type with `suffix` appended to its last path segment, when it is a path.
fn rename(ty: &syn::Type, suffix: &str) -> syn::Type {
    let syn::Type::Path(path) = ty else {
        return ty.clone();
    };

    let mut path = path.clone();
    if let Some(segment) = path.path.segments.last_mut() {
        segment.ident = format_ident!("{}{}", segment.ident, suffix);
    }

    syn::Type::Path(path)
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
