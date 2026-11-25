//! The `crud!` macro: generates axum route wrappers for one dbkit model.

use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use quote::quote;
use syn::Ident;
use syn::LitStr;
use syn::Path;
use syn::Token;
use syn::Type;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse_macro_input;

#[derive(Clone, Copy)]
enum OperationKind {
    Query,
    Insert,
    Patch,
    Delete,
}

struct Operation {
    kind: OperationKind,
    scope: LitStr,
    action: LitStr,
    tenant_column: Option<LitStr>,
}

struct Config {
    model: Path,
    path: LitStr,
    state: Type,
    session: Type,
    tenant_column: Option<LitStr>,
    operations: Vec<Operation>,
}

impl Parse for Config {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut model = None;
        let mut path = None;
        let mut state = None;
        let mut session = None;
        let mut tenant_column = None;
        let mut operations = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;

            match key.to_string().as_str() {
                "model" => {
                    input.parse::<Token![=]>()?;
                    model = Some(input.parse()?);
                }
                "path" => {
                    input.parse::<Token![=]>()?;
                    path = Some(input.parse()?);
                }
                "state" => {
                    input.parse::<Token![=]>()?;
                    state = Some(input.parse()?);
                }
                "session" => {
                    input.parse::<Token![=]>()?;
                    session = Some(input.parse()?);
                }
                "tenant_column" => {
                    input.parse::<Token![=]>()?;
                    tenant_column = Some(input.parse()?);
                }
                "query" | "insert" | "patch" | "delete" => {
                    let kind = match key.to_string().as_str() {
                        "query" => OperationKind::Query,
                        "insert" => OperationKind::Insert,
                        "patch" => OperationKind::Patch,
                        "delete" => OperationKind::Delete,
                        _ => unreachable!(),
                    };

                    operations.push(operation(input, kind)?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown `crud!` key `{other}`; expected `model`, `path`, `state`, \
                             `session`, `tenant_column`, `query`, `insert`, `patch` or `delete`"
                        ),
                    ));
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let Some(model) = model else {
            return Err(syn::Error::new(Span::call_site(), "missing `model = ...`"));
        };
        let Some(path) = path else {
            return Err(syn::Error::new(Span::call_site(), "missing `path = ...`"));
        };
        let Some(state) = state else {
            return Err(syn::Error::new(Span::call_site(), "missing `state = ...`"));
        };
        let Some(session) = session else {
            return Err(syn::Error::new(
                Span::call_site(),
                "missing `session = ...`",
            ));
        };
        if operations.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "declare at least one of `query`, `insert`, `patch` or `delete`",
            ));
        }

        Ok(Self {
            model,
            path,
            state,
            session,
            tenant_column,
            operations,
        })
    }
}

fn operation(input: ParseStream<'_>, kind: OperationKind) -> syn::Result<Operation> {
    let content;
    syn::braced!(content in input);

    let mut scope = None;
    let mut action = None;
    let mut tenant_column = None;

    while !content.is_empty() {
        let key: Ident = content.parse()?;
        content.parse::<Token![=]>()?;

        match key.to_string().as_str() {
            "scope" => scope = Some(content.parse()?),
            "action" => action = Some(content.parse()?),
            "tenant_column" => tenant_column = Some(content.parse()?),
            other => {
                return Err(syn::Error::new(
                    key.span(),
                    format!(
                        "unknown operation key `{other}`; expected `scope`, `action` or \
                         `tenant_column`"
                    ),
                ));
            }
        }

        if content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
        }
    }

    let Some(scope) = scope else {
        return Err(syn::Error::new(
            Span::call_site(),
            "missing `scope = \"...\"`",
        ));
    };
    let Some(action) = action else {
        return Err(syn::Error::new(
            Span::call_site(),
            "missing `action = \"...\"`",
        ));
    };

    Ok(Operation {
        kind,
        scope,
        action,
        tenant_column,
    })
}

#[proc_macro]
pub fn crud(input: TokenStream) -> TokenStream {
    let config = parse_macro_input!(input as Config);

    match expand(config) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(config: Config) -> syn::Result<TokenStream2> {
    let model = &config.model;
    let state = &config.state;
    let session = &config.session;
    let path = &config.path;
    let module = module_name(&path.value());
    let column = suffixed(model, "Column");
    let insert = suffixed(model, "Insert");
    let patch = suffixed(model, "Patch");
    let pkv = suffixed(model, "PKV");

    let mut handlers = Vec::new();
    let mut methods = Vec::new();

    for operation in &config.operations {
        let scope = &operation.scope;
        let action = &operation.action;

        match operation.kind {
            OperationKind::Query => {
                let tenant_column = operation
                    .tenant_column
                    .as_ref()
                    .or(config.tenant_column.as_ref());
                let tenant_column = match tenant_column {
                    Some(column) => quote!(::core::option::Option::Some(#column)),
                    None => quote!(<#model as ::dbkit::Model>::meta().tenant()),
                };

                handlers.push(quote! {
                    async fn query(
                        ::axum::extract::State(state): ::axum::extract::State<#state>,
                        session: #session,
                        ::httpkit::ApiQuery(params): ::httpkit::ApiQuery<::httpkit::ListParams>,
                    ) -> ::httpkit::ApiResult<::axum::Json<Vec<#model>>> {
                        let query = ::httpkit::parse_query::<#column>(params.q.as_deref())?;
                        let tenant_column = #tenant_column;

                        ::httpkit::authorize_query(
                            &session,
                            &query,
                            tenant_column,
                            #scope,
                            #action,
                        )?;

                        ::httpkit::query::<#state, #column>(&state, query).await
                    }
                });
                methods.push(quote!(get(query)));
            }
            OperationKind::Insert => {
                handlers.push(quote! {
                    async fn insert(
                        ::axum::extract::State(state): ::axum::extract::State<#state>,
                        session: #session,
                        ::httpkit::ApiJson(entries): ::httpkit::ApiJson<Vec<#insert>>,
                    ) -> ::httpkit::ApiResult<(
                        ::axum::http::StatusCode,
                        ::axum::Json<Vec<<#insert as ::dbkit::InsertRow>::Key>>,
                    )> {
                        ::httpkit::authorize_rows(&session, &entries, #scope, #action)?;
                        ::httpkit::insert::<#state, #insert>(&state, entries).await
                    }
                });
                methods.push(quote!(post(insert)));
            }
            OperationKind::Patch => {
                handlers.push(quote! {
                    async fn patch(
                        ::axum::extract::State(state): ::axum::extract::State<#state>,
                        session: #session,
                        ::httpkit::ApiJson(entries): ::httpkit::ApiJson<Vec<#patch>>,
                    ) -> ::httpkit::ApiResult<::axum::http::StatusCode> {
                        ::httpkit::authorize_rows(&session, &entries, #scope, #action)?;
                        ::httpkit::patch::<#state, #patch>(&state, entries).await
                    }
                });
                methods.push(quote!(patch(patch)));
            }
            OperationKind::Delete => {
                handlers.push(quote! {
                    async fn delete(
                        ::axum::extract::State(state): ::axum::extract::State<#state>,
                        session: #session,
                        ::httpkit::ApiJson(keys): ::httpkit::ApiJson<Vec<#pkv>>,
                    ) -> ::httpkit::ApiResult<::axum::http::StatusCode> {
                        ::httpkit::authorize_rows(&session, &keys, #scope, #action)?;
                        ::httpkit::delete::<#state, #pkv>(&state, keys).await
                    }
                });
                methods.push(quote!(delete(delete)));
            }
        }
    }

    Ok(quote! {
        pub mod #module {
            #[allow(unused_imports)]
            use super::*;

            #(#handlers)*

            pub fn routes(state: #state) -> ::axum::Router {
                let method_router = ::axum::routing::MethodRouter::new();
                #(let method_router = method_router.#methods;)*

                ::axum::Router::new()
                    .route(#path, method_router)
                    .with_state(state)
            }
        }
    })
}

fn suffixed(path: &Path, suffix: &str) -> Path {
    let mut path = path.clone();
    let Some(segment) = path.segments.last_mut() else {
        return path;
    };
    segment.ident = format_ident!("{}{}", segment.ident, suffix);

    path
}

fn module_name(path: &str) -> Ident {
    let mut name = String::from("httpkit_routes");

    for character in path.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
        } else {
            name.push('_');
        }
    }

    while name.ends_with('_') {
        name.pop();
    }

    format_ident!("{}", name)
}
