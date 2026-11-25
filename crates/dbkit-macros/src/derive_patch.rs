//! Derive of `<Model>Patch`: key and version as-is, other columns as `Patch<T>` so a missing key leaves them alone.

use crate::annotations;
use crate::annotations::Column;
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
            "`DbPatch` cannot be derived for a `#[db(subquery = \"...\")]` struct; a subquery is \
             read-only",
        ));
    }

    let name = &input.ident;
    let visibility = &input.vis;
    let version = annotations.version.as_ref();
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let patch = format_ident!("{name}Patch");
    let doc = format!(
        "Patch input of [`{name}`]. The primary key is required, as is `version` when the model \
         has one; every other column is a [`dbkit::Patch`] field. `new` takes the required fields \
         and starts every other column absent; `set_` and `clear_` methods change one column."
    );
    let generics = &input.generics;

    let mut definitions = Vec::new();
    let mut assignments = Vec::new();
    let mut keys = Vec::new();
    let mut expected_version = quote!(::core::option::Option::None);
    let mut initializers = Vec::new();
    let mut setters = Vec::new();
    let mut key_parameters = Vec::new();
    let mut tenant_parameter = None;
    let mut version_parameter = None;

    for column in &annotations.columns {
        let (field_name, ty, nullable, generated, triggers) = match column {
            Column::Scalar {
                name,
                ty,
                nullable,
                generated,
                triggers,
                ..
            } => (name, ty, *nullable, generated, triggers),
            Column::Flattened { name, ty } => {
                let nested = patch_type(ty);
                definitions.push(quote! {
                    #[serde(flatten)]
                    #[garde(dive)]
                    pub #name: #nested
                });
                assignments.push(quote! {
                    assignments.extend(
                        <#nested as ::dbkit::PatchRow>::assignments(&self.#name),
                    );
                });
                initializers.push(quote!(#name: <#nested>::new()));
                let field = field_text(name);
                let setter = format_ident!("set_{field}");
                let doc = format!("Sets the flattened `{field}` patch.");
                setters.push(quote! {
                    #[doc = #doc]
                    pub fn #setter(mut self, value: #nested) -> Self {
                        self.#name = value;
                        self
                    }
                });
                continue;
            }
        };

        if generated.is_some() || !triggers.is_empty() {
            continue;
        }

        if annotations.created_at.as_ref() == Some(field_name)
            || annotations.updated_at.as_ref() == Some(field_name)
        {
            continue;
        }

        if annotations.primary_key.contains(field_name) {
            definitions.push(quote! {
                #[garde(skip)]
                pub #field_name: #ty
            });
            keys.push(quote!(&self.#field_name));
            initializers.push(quote!(#field_name));
            key_parameters.push(quote!(#field_name: #ty));
            continue;
        }

        if version == Some(field_name) {
            definitions.push(quote! {
                #[garde(skip)]
                pub #field_name: #ty
            });
            expected_version = quote!(::core::option::Option::Some(&self.#field_name));
            initializers.push(quote!(#field_name));
            version_parameter = Some(quote!(#field_name: #ty));
            continue;
        }

        if annotations.tenant.as_ref() == Some(field_name) {
            definitions.push(quote! {
                #[garde(skip)]
                pub #field_name: #ty
            });
            initializers.push(quote!(#field_name));
            tenant_parameter = Some(quote!(#field_name: #ty));
            continue;
        }

        let inner = non_optional(ty);
        let garde = if carries_rules(inner) {
            quote!(#[garde(dive)])
        } else {
            quote!(#[garde(skip)])
        };
        definitions.push(quote! {
            #[serde(default, skip_serializing_if = "::dbkit::Patch::is_absent")]
            #garde
            pub #field_name: ::dbkit::Patch<#inner>
        });
        initializers.push(quote!(#field_name: ::dbkit::Patch::Absent));

        let field = field_text(field_name);
        let column = syn::LitStr::new(&field, field_name.span());
        assignments.push(quote! {
            match &self.#field_name {
                ::dbkit::Patch::Absent => {}
                ::dbkit::Patch::Set(value) => assignments.push(::dbkit::Assignment {
                    column: #column,
                    value: ::core::option::Option::Some(value),
                }),
                ::dbkit::Patch::Null => assignments.push(::dbkit::Assignment {
                    column: #column,
                    value: ::core::option::Option::None,
                }),
            }
        });

        let setter = format_ident!("set_{field}");
        let doc = format!("Sets `{field}` on this patch.");
        setters.push(quote! {
            #[doc = #doc]
            pub fn #setter(mut self, value: impl ::core::convert::Into<#inner>) -> Self {
                self.#field_name = ::dbkit::Patch::Set(::core::convert::Into::into(value));
                self
            }
        });

        if nullable {
            let clear = format_ident!("clear_{field}");
            let doc = format!("Clears `{field}` on this patch by setting it to `NULL`.");
            setters.push(quote! {
                #[doc = #doc]
                pub fn #clear(mut self) -> Self {
                    self.#field_name = ::dbkit::Patch::Null;
                    self
                }
            });
        }
    }

    let parameters = key_parameters
        .into_iter()
        .chain(tenant_parameter)
        .chain(version_parameter);
    let builder = quote! {
        impl #impl_generics #patch #type_generics #where_clause {
            #[doc = "Starts a patch with the required key and version fields; every other column \
                     is absent."]
            pub fn new(#(#parameters),*) -> Self {
                Self { #(#initializers),* }
            }

            #(#setters)*
        }
    };

    let tenant_row = match &annotations.tenant {
        Some(tenant) => quote! {
            #[automatically_derived]
            impl #impl_generics ::dbkit::TenantRow for #patch #type_generics #where_clause {
                fn tenant(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                    ::core::option::Option::Some(&self.#tenant)
                }
            }
        },
        None => quote! {
            #[automatically_derived]
            impl #impl_generics ::dbkit::TenantRow for #patch #type_generics #where_clause {
                fn tenant(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                    ::core::option::Option::None
                }
            }
        },
    };

    let patch_method = match annotations.shape {
        annotations::Shape::Table(_) => quote! {
            impl #impl_generics #name #type_generics #where_clause {
                #[doc = "A transaction that applies the given patches."]
                pub fn patch(
                    entries: impl ::core::iter::IntoIterator<Item = #patch #type_generics>,
                ) -> ::dbkit::PatchQuery<#patch #type_generics> {
                    ::dbkit::PatchQuery::new(entries)
                }
            }
        },
        annotations::Shape::SubQuery(_) | annotations::Shape::Fragment => quote!(),
    };

    Ok(quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize, ::garde::Validate)]
        #visibility struct #patch #generics {
            #(#definitions),*
        }

        #builder

        #[automatically_derived]
        impl #impl_generics ::dbkit::PatchRow for #patch #type_generics #where_clause {
            type Model = #name #type_generics;

            fn assignments(&self) -> ::std::vec::Vec<::dbkit::Assignment<'_>> {
                let mut assignments = ::std::vec::Vec::new();
                #(#assignments)*
                assignments
            }

            fn key(&self) -> ::std::vec::Vec<&dyn ::dbkit::Bind> {
                ::std::vec![#(#keys),*]
            }

            fn version(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                #expected_version
            }
        }

        #tenant_row

        #patch_method
    })
}

/// The field name as a column and method suffix: a raw identifier loses `r#`.
fn field_text(name: &syn::Ident) -> String {
    name.to_string().trim_start_matches("r#").to_owned()
}

/// The patch type of a flattened field: its last path segment plus `Patch`.
fn patch_type(ty: &Type) -> Type {
    let Type::Path(path) = ty else {
        return ty.clone();
    };

    let mut path = path.clone();
    if let Some(segment) = path.path.segments.last_mut() {
        segment.ident = format_ident!("{}Patch", segment.ident);
    }

    Type::Path(path)
}

/// The type inside `Option<...>`, or the type itself; a nullable column's patch is `Patch<T>`, not `Patch<Option<T>>`.
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

/// Whether a payload type can carry rules: a local type with `garde::Validate`, so `Patch<T>` validates or not.
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
