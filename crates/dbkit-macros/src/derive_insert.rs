//! Derive of a model's insert input type.
//!
//! Required columns are plain fields; columns with a default or a nullable
//! column are `Option` fields, so `None` omits the column. The constructor
//! takes the required fields and each flattened fragment insert. `set_` methods
//! change one column.
//!
//! An inherited default is verified with a const assertion: the macro cannot
//! read `DbType::PG_DEFAULT`, but the compiler can.

use crate::annotations;
use crate::annotations::Column;
use crate::annotations::PgDefault;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Type;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let annotations = annotations::parse(input)?;

    if let annotations::Shape::SubQuery(_) = annotations.shape {
        return Err(syn::Error::new_spanned(
            input,
            "`DbInsert` cannot be derived for a `#[db(subquery = \"...\")]` struct; a subquery is \
             read-only",
        ));
    }

    let name = &input.ident;
    let visibility = &input.vis;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let generics = &input.generics;

    let insert = format_ident!("{name}Insert");
    let key = match &annotations.shape {
        annotations::Shape::Table(_) => {
            let key = format_ident!("{name}PK");
            quote!(#key)
        }
        annotations::Shape::SubQuery(_) | annotations::Shape::Fragment => quote!(()),
    };
    let doc = format!(
        "Insert input of [`{name}`]. Required columns are plain fields, and columns with a \
         default or a nullable column are `Option` fields. `new` takes the required fields and \
         every flattened insert; `set_` methods change one column."
    );

    let mut definitions = Vec::new();
    let mut initializers = Vec::new();
    let mut parameters = Vec::new();
    let mut setters = Vec::new();
    let mut columns = Vec::new();
    let mut assertions = Vec::new();

    for column in &annotations.columns {
        match column {
            Column::Flattened { name: field, ty } => {
                let nested = insert_type(ty);
                definitions.push(quote! {
                    #[serde(flatten)]
                    #[garde(dive)]
                    pub #field: #nested
                });
                initializers.push(quote!(#field));
                parameters.push(quote!(#field: #nested));

                let text = field_text(field);
                let setter = format_ident!("set_{text}");
                let doc = format!("Sets the flattened `{text}` insert.");
                setters.push(quote! {
                    #[doc = #doc]
                    pub fn #setter(mut self, value: #nested) -> Self {
                        self.#field = value;
                        self
                    }
                });
                columns.push(quote! {
                    columns.extend(
                        <#nested as ::dbkit::InsertRow>::columns(&self.#field),
                    );
                });
                continue;
            }
            Column::Scalar {
                name: field,
                ty,
                nullable,
                pg_default,
                ..
            } => {
                let text = field_text(field);
                let column = syn::LitStr::new(&text, field.span());
                let role = annotations.primary_key.contains(field)
                    || annotations.version.as_ref() == Some(field)
                    || annotations.created_at.as_ref() == Some(field)
                    || annotations.updated_at.as_ref() == Some(field);
                let optional = match pg_default {
                    PgDefault::Declared(_) | PgDefault::Inherited => true,
                    PgDefault::None => *nullable,
                };
                let inner = non_optional(ty);
                let garde = garde(inner, role);
                let setter = format_ident!("set_{text}");
                let doc = format!("Sets `{text}` on this insert.");

                if optional {
                    definitions.push(quote! {
                        #[serde(
                            default,
                            skip_serializing_if = "::core::option::Option::is_none"
                        )]
                        #garde
                        pub #field: ::core::option::Option<#inner>
                    });
                    initializers.push(quote!(#field: ::core::option::Option::None));
                    setters.push(quote! {
                        #[doc = #doc]
                        pub fn #setter(
                            mut self,
                            value: impl ::core::convert::Into<#inner>,
                        ) -> Self {
                            self.#field =
                                ::core::option::Option::Some(::core::convert::Into::into(value));
                            self
                        }
                    });
                    columns.push(quote! {
                        if let ::core::option::Option::Some(value) = &self.#field {
                            columns.push(::dbkit::Assignment {
                                column: #column,
                                value: ::core::option::Option::Some(value),
                            });
                        }
                    });

                    if matches!(pg_default, PgDefault::Inherited)
                        && !*nullable
                        && input.generics.params.is_empty()
                    {
                        let message = format!(
                            "field `{text}` says `pgdefault = inherit`, but `{}` has no default; \
                             remove the annotation or give the type a default",
                            quote!(#inner),
                        );
                        let message = syn::LitStr::new(&message, field.span());
                        assertions.push(quote! {
                            const _: () = assert!(
                                <#inner as ::dbkit::DbType>::PG_DEFAULT.is_some(),
                                #message
                            );
                        });
                    }
                } else {
                    definitions.push(quote! {
                        #garde
                        pub #field: #ty
                    });
                    parameters.push(quote!(#field: impl ::core::convert::Into<#ty>));
                    initializers.push(quote!(#field: ::core::convert::Into::into(#field)));
                    setters.push(quote! {
                        #[doc = #doc]
                        pub fn #setter(
                            mut self,
                            value: impl ::core::convert::Into<#ty>,
                        ) -> Self {
                            self.#field = ::core::convert::Into::into(value);
                            self
                        }
                    });
                    columns.push(quote! {
                        columns.push(::dbkit::Assignment {
                            column: #column,
                            value: ::core::option::Option::Some(&self.#field),
                        });
                    });
                }
            }
        }
    }

    let insert_method = match annotations.shape {
        annotations::Shape::Table(_) => quote! {
            impl #impl_generics #name #type_generics #where_clause {
                #[doc = "A transaction that inserts the given rows and returns their primary keys."]
                pub fn insert(
                    entries: impl ::core::iter::IntoIterator<Item = #insert #type_generics>,
                ) -> ::dbkit::InsertQuery<#insert #type_generics> {
                    ::dbkit::InsertQuery::new(entries)
                }
            }
        },
        annotations::Shape::SubQuery(_) | annotations::Shape::Fragment => quote!(),
    };

    Ok(quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::garde::Validate)]
        #visibility struct #insert #generics {
            #(#definitions),*
        }

        impl #impl_generics #insert #type_generics #where_clause {
            #[doc = "Starts an insert with the required fields; every optional column is absent."]
            pub fn new(#(#parameters),*) -> Self {
                Self { #(#initializers),* }
            }

            #(#setters)*
        }

        #(#assertions)*

        #[automatically_derived]
        impl #impl_generics ::dbkit::InsertRow for #insert #type_generics #where_clause {
            type Model = #name #type_generics;
            type Key = #key;

            fn columns(&self) -> ::std::vec::Vec<::dbkit::Assignment<'_>> {
                let mut columns = ::std::vec::Vec::new();
                #(#columns)*
                columns
            }
        }

        #insert_method
    })
}

/// The field name as a column and method suffix: a raw identifier loses `r#`.
fn field_text(name: &syn::Ident) -> String {
    name.to_string().trim_start_matches("r#").to_owned()
}

/// The insert type of a flattened field: the field type with `Insert` appended
/// to its last path segment, so `size: Size` becomes `SizeInsert`.
fn insert_type(ty: &Type) -> Type {
    let Type::Path(path) = ty else {
        return ty.clone();
    };

    let mut path = path.clone();
    if let Some(segment) = path.path.segments.last_mut() {
        segment.ident = format_ident!("{}Insert", segment.ident);
    }

    Type::Path(path)
}

/// The type inside `Option<...>`, or the type itself.
fn non_optional(ty: &Type) -> &Type {
    let Type::Path(path) = ty else {
        return ty;
    };
    let Some(segment) = path.path.segments.last() else {
        return ty;
    };
    if segment.ident != "Option" {
        return ty;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return ty;
    };
    match arguments.args.first() {
        Some(syn::GenericArgument::Type(inner)) => inner,
        _ => ty,
    }
}

/// Whether a payload type can carry validation rules.
///
/// Only a local type can implement `garde::Validate`, so a primitive or a
/// foreign type is skipped. Role fields are skipped too: an id, version or
/// timestamp is not request payload.
fn garde(ty: &Type, role: bool) -> TokenStream {
    if role || !carries_rules(ty) {
        quote!(#[garde(skip)])
    } else {
        quote!(#[garde(dive)])
    }
}

/// Whether a payload type is likely to implement `garde::Validate`.
fn carries_rules(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return true;
    };
    let Some(segment) = path.path.segments.last() else {
        return true;
    };

    !matches!(
        segment.ident.to_string().as_str(),
        "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "isize"
            | "usize"
            | "f32"
            | "f64"
            | "Uuid"
            | "DateTime"
    )
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
}
