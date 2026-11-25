//! Session permissions and the tenant scope derived from a query payload.

use crate::ApiError;
use crate::ApiResult;
use dbkit::Column;
use dbkit::Combinator;
use dbkit::Filter;
use dbkit::FilterOperator;
use dbkit::Operand;
use dbkit::Query;
use dbkit::TenantRow;
use serde::de::DeserializeOwned;

/// A session that can answer whether one permission is granted.
pub trait Session {
    type TenantId;

    fn has_permission(&self, permission: &Permission<Self::TenantId>) -> bool;
}

/// One permission check: global, or scoped to a tenant.
pub enum Permission<'a, T> {
    Global {
        scope: &'a str,
        action: &'a str,
    },
    Tenant {
        tenant_id: T,
        scope: &'a str,
        action: &'a str,
    },
}

impl<'a, T> Permission<'a, T> {
    pub const fn global(scope: &'a str, action: &'a str) -> Self {
        Self::Global { scope, action }
    }

    pub const fn tenant(tenant_id: T, scope: &'a str, action: &'a str) -> Self {
        Self::Tenant {
            tenant_id,
            scope,
            action,
        }
    }
}

/// Turns a denied permission into `403`.
pub fn authorize<S: Session>(
    session: &S,
    permission: &Permission<'_, S::TenantId>,
) -> ApiResult<()> {
    if session.has_permission(permission) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// What a query's filters say about the tenant scope.
#[derive(Debug, PartialEq, Eq)]
pub enum TenantScope<T> {
    Global,
    Tenants(Vec<T>),
}

/// Reads the tenant scope from the top-level `AND` part of the filter tree.
pub fn tenant_scope<C: Column, T: DeserializeOwned>(
    filters: &[Filter<C>],
    tenant_column: &str,
) -> ApiResult<TenantScope<T>> {
    let mut tenants = Vec::new();
    let mut found = false;

    collect_tenants(filters, tenant_column, false, &mut tenants, &mut found)?;

    if found && tenants.is_empty() {
        return Err(ApiError::bad_request("the tenant filter has no value"));
    }

    if found {
        Ok(TenantScope::Tenants(tenants))
    } else {
        Ok(TenantScope::Global)
    }
}

fn collect_tenants<C: Column, T: DeserializeOwned>(
    filters: &[Filter<C>],
    tenant_column: &str,
    under_or: bool,
    tenants: &mut Vec<T>,
    found: &mut bool,
) -> ApiResult<()> {
    for filter in filters {
        match filter {
            Filter::Leaf(leaf) => {
                if leaf.column.name() != tenant_column {
                    continue;
                }
                if under_or {
                    return Err(ApiError::bad_request(
                        "a tenant filter cannot appear under `or`",
                    ));
                }

                *found = true;

                match leaf.operator {
                    FilterOperator::Eq => {
                        let value = leaf.operand.value().ok_or_else(|| {
                            ApiError::bad_request("the tenant filter has no value")
                        })?;

                        tenants.push(deserialize(value.as_json())?);
                    }
                    FilterOperator::Any => {
                        let values = match &leaf.operand {
                            Operand::List { values, .. } => values.as_json(),
                            _ => {
                                return Err(ApiError::bad_request(
                                    "the tenant `any` filter needs a list",
                                ));
                            }
                        };
                        let values = values.as_array().ok_or_else(|| {
                            ApiError::bad_request("the tenant `any` filter needs a list")
                        })?;

                        for value in values {
                            tenants.push(deserialize(value.clone())?);
                        }
                    }
                    _ => {
                        return Err(ApiError::bad_request(
                            "the tenant filter must use `eq` or `any`",
                        ));
                    }
                }
            }
            Filter::Group(group) => {
                let under_or = under_or || group.combinator == Combinator::Or;

                collect_tenants(&group.filters, tenant_column, under_or, tenants, found)?;
            }
        }
    }

    Ok(())
}

fn deserialize<T: DeserializeOwned>(value: serde_json::Value) -> ApiResult<T> {
    serde_json::from_value(value)
        .map_err(|error| ApiError::bad_request(format!("invalid tenant filter value: {error}")))
}

/// Checks the permission a query selects: tenant-scoped when it names a tenant,
/// otherwise global.
pub fn authorize_query<C, S>(
    session: &S,
    query: &Query<C>,
    tenant_column: Option<&str>,
    scope: &str,
    action: &str,
) -> ApiResult<()>
where
    C: Column,
    S: Session,
    S::TenantId: DeserializeOwned,
{
    let Some(tenant_column) = tenant_column else {
        return authorize(session, &Permission::global(scope, action));
    };

    match tenant_scope(&query.filters, tenant_column)? {
        TenantScope::Global => authorize(session, &Permission::global(scope, action)),
        TenantScope::Tenants(tenants) => {
            for tenant in tenants {
                authorize(session, &Permission::tenant(tenant, scope, action))?;
            }

            Ok(())
        }
    }
}

/// Checks every insert, patch or delete input against the tenant it carries.
pub fn authorize_rows<S, R>(session: &S, rows: &[R], scope: &str, action: &str) -> ApiResult<()>
where
    S: Session,
    S::TenantId: DeserializeOwned,
    R: TenantRow,
{
    for row in rows {
        match row.tenant() {
            Some(value) => {
                let tenant: S::TenantId =
                    serde_json::from_value(value.as_json()).map_err(ApiError::internal)?;

                authorize(session, &Permission::tenant(tenant, scope, action))?;
            }
            None => authorize(session, &Permission::global(scope, action))?,
        }
    }

    Ok(())
}
