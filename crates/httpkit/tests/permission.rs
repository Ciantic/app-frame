use dbkit::Bind;
use dbkit::DbModel;
use dbkit::Filter;
use dbkit::TenantRow;
use httpkit::ApiError;
use httpkit::Permission;
use httpkit::Session;
use httpkit::TenantScope;
use httpkit::authorize_query;
use httpkit::authorize_rows;
use httpkit::parse_query;
use httpkit::tenant_scope;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct TenantId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, DbModel)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(query_ops(eq, any))]
    pub id: WidgetId,

    #[db(tenant)]
    #[db(pgtype = "uuid")]
    #[db(query_ops(eq, any))]
    pub tenant_id: TenantId,

    #[db(pgtype = "text")]
    #[db(query_ops(eq))]
    pub name: String,
}

fn tenant(value: u128) -> TenantId {
    TenantId(Uuid::from_u128(value))
}

fn widget() -> WidgetId {
    WidgetId(Uuid::from_u128(99))
}

struct TestSession {
    tenants: Vec<TenantId>,
}

impl Session for TestSession {
    type TenantId = TenantId;

    fn has_permission(&self, permission: &Permission<TenantId>) -> bool {
        match permission {
            Permission::Global { .. } => true,
            Permission::Tenant { tenant_id, .. } => self.tenants.contains(tenant_id),
        }
    }
}

#[test]
fn no_tenant_filter_is_global() {
    let query = Widget::query().filters([Widget::columns().name.eq("sample")]);

    assert_eq!(
        tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").unwrap(),
        TenantScope::Global
    );
}

#[test]
fn tenant_eq_is_a_tenant_scope() {
    let query = Widget::query().filters([Widget::columns().tenant_id.eq(tenant(1))]);

    assert_eq!(
        tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").unwrap(),
        TenantScope::Tenants(vec![tenant(1)])
    );
}

#[test]
fn tenant_any_is_every_named_tenant() {
    let query = Widget::query().filters([Widget::columns().tenant_id.any([tenant(1), tenant(2)])]);

    assert_eq!(
        tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").unwrap(),
        TenantScope::Tenants(vec![tenant(1), tenant(2)])
    );
}

#[test]
fn tenant_under_or_is_rejected() {
    let query = Widget::query().filters([Filter::or([
        Widget::columns().tenant_id.eq(tenant(1)),
        Widget::columns().id.eq(widget()),
    ])]);

    assert!(tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").is_err());
}

#[test]
fn tenant_with_a_non_scope_operator_is_rejected() {
    let query = Widget::query().filters([Widget::columns().tenant_id.is_null()]);

    assert!(tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").is_err());
}

#[test]
fn an_empty_tenant_any_is_rejected() {
    let query = Widget::query().filters([Widget::columns().tenant_id.any(Vec::<TenantId>::new())]);

    assert!(tenant_scope::<WidgetColumn, TenantId>(&query.filters, "tenant_id").is_err());
}

#[test]
fn every_tenant_must_be_allowed() {
    let query = Widget::query().filters([Widget::columns().tenant_id.any([tenant(1), tenant(2)])]);
    let session = TestSession {
        tenants: vec![tenant(1)],
    };

    let error = authorize_query(&session, &query, Some("tenant_id"), "widget", "read").unwrap_err();

    assert!(matches!(error, ApiError::Forbidden));
}

#[test]
fn a_query_without_a_tenant_filter_uses_global_permission() {
    let query = Widget::query().filters([Widget::columns().name.eq("sample")]);
    let session = TestSession { tenants: vec![] };

    assert!(authorize_query(&session, &query, Some("tenant_id"), "widget", "read",).is_ok());
}

struct Row {
    tenant_id: TenantId,
}

impl TenantRow for Row {
    fn tenant(&self) -> Option<&dyn Bind> {
        Some(&self.tenant_id)
    }
}

#[test]
fn rows_outside_the_session_tenant_are_forbidden() {
    let session = TestSession {
        tenants: vec![tenant(1)],
    };

    let error = authorize_rows(
        &session,
        &[Row {
            tenant_id: tenant(2),
        }],
        "widget",
        "update",
    )
    .unwrap_err();

    assert!(matches!(error, ApiError::Forbidden));
}

#[test]
fn rows_inside_the_session_tenant_are_allowed() {
    let session = TestSession {
        tenants: vec![tenant(1)],
    };

    assert!(
        authorize_rows(
            &session,
            &[Row {
                tenant_id: tenant(1),
            }],
            "widget",
            "update",
        )
        .is_ok()
    );
}

#[test]
fn a_query_gets_the_default_limit() {
    let query = parse_query::<WidgetColumn>(None).unwrap();

    assert_eq!(query.limit, Some(httpkit::DEFAULT_LIMIT));
}

#[test]
fn a_projection_is_rejected() {
    assert!(matches!(
        parse_query::<WidgetColumn>(Some(r#"{"select":["id"]}"#)),
        Err(ApiError::BadRequest { .. })
    ));
}
