//! Generation of `{Model}PK` and, with a tenant or version column, `{Model}PKV`, decoded by column name.

use crate::annotations;
use crate::annotations::Column;
use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::DeriveInput;
use syn::Ident;
use syn::Type;
use syn::Visibility;

pub(crate) fn expand(
    input: &DeriveInput,
    annotations: &annotations::Annotations,
) -> syn::Result<TokenStream> {
    if !matches!(annotations.shape, annotations::Shape::Table(_)) {
        return Ok(quote!());
    }

    let name = &input.ident;
    let visibility = &input.vis;

    let mut key = Vec::new();
    for field in &annotations.primary_key {
        key.push(column(annotations, field)?);
    }

    let pk = format_ident!("{name}PK");
    let pk_doc = format!("The primary key of [`{name}`].");

    let tenant = annotations.tenant.as_ref();
    let mut identity = key.clone();
    if let Some(tenant) = tenant
        && !annotations.primary_key.contains(tenant)
    {
        identity.push(column(annotations, tenant)?);
    }
    if let Some(version) = &annotations.version {
        identity.push(column(annotations, version)?);
    }

    let pkv_ident = format_ident!("{name}PKV");
    let pkv = if tenant.is_some() || annotations.version.is_some() {
        let doc = match (tenant.is_some(), annotations.version.is_some()) {
            (true, true) => format!(
                "The primary key, tenant and version of [`{name}`], for operations that identify \
                 a row and check its tenant and version."
            ),
            (true, false) => format!(
                "The primary key and tenant of [`{name}`], for operations that identify a row \
                 within a tenant."
            ),
            (false, true) => format!(
                "The primary key and version of [`{name}`], for operations that identify a row \
                 and check its version."
            ),
            (false, false) => unreachable!("the key struct exists only with a tenant or version"),
        };

        key_struct(&pkv_ident, visibility, &identity, doc)
    } else {
        let doc = format!(
            "The primary key of [`{name}`]; the model has no tenant or version, so this is \
             [`{pk}`]."
        );

        let pkv = &pkv_ident;

        quote! {
            #[doc = #doc]
            #[allow(dead_code)]
            #visibility type #pkv = #pk;
        }
    };

    let tenant_row = match tenant {
        Some(tenant) => quote! {
            #[automatically_derived]
            impl ::dbkit::TenantRow for #pkv_ident {
                fn tenant(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                    ::core::option::Option::Some(&self.#tenant)
                }
            }
        },
        None => quote! {
            #[automatically_derived]
            impl ::dbkit::TenantRow for #pkv_ident {
                fn tenant(&self) -> ::core::option::Option<&dyn ::dbkit::Bind> {
                    ::core::option::Option::None
                }
            }
        },
    };

    let pk = key_struct(&pk, visibility, &key, pk_doc);

    Ok(quote! {
        #pk
        #pkv
        #tenant_row
    })
}

/// The key fields as written in the model: primary key, then a separate tenant column, then version.
fn key_struct(
    name: &Ident,
    visibility: &Visibility,
    fields: &[(&Ident, &Type)],
    doc: String,
) -> TokenStream {
    let mut definitions = Vec::new();
    let mut decoders = Vec::new();

    for (field, ty) in fields {
        let literal = syn::LitStr::new(field.to_string().trim_start_matches("r#"), field.span());

        definitions.push(quote!(pub #field: #ty));
        decoders.push(quote!(#field: row.try_get(#literal)?));
    }

    quote! {
        #[doc = #doc]
        #[allow(dead_code)]
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        #visibility struct #name {
            #(#definitions),*
        }

        impl<'r> ::sqlx::FromRow<'r, ::sqlx::postgres::PgRow> for #name {
            fn from_row(
                row: &'r ::sqlx::postgres::PgRow,
            ) -> ::core::result::Result<Self, ::sqlx::Error> {
                use ::sqlx::Row as _;

                ::core::result::Result::Ok(Self {
                    #(#decoders),*
                })
            }
        }
    }
}

/// Looks up the scalar field behind a role annotation.
fn column<'a>(
    annotations: &'a annotations::Annotations,
    field: &Ident,
) -> syn::Result<(&'a Ident, &'a Type)> {
    annotations
        .columns
        .iter()
        .find_map(|column| match column {
            Column::Scalar { name, ty, .. } if name == field => Some((name, ty)),
            _ => None,
        })
        .ok_or_else(|| {
            syn::Error::new_spanned(
                field,
                "a primary key or version column must be a scalar field",
            )
        })
}
