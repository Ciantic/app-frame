use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use dbkit::DbDelete;
use dbkit::DbInsert;
use dbkit::DbModel;
use dbkit::DbPatch;
use httpkit::DbState;
use httpkit::Permission;
use httpkit::Session;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, garde::Validate,
)]
#[sqlx(transparent)]
pub struct WidgetId(#[garde(skip)] pub Uuid);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, garde::Validate,
)]
#[sqlx(transparent)]
pub struct TenantId(#[garde(skip)] pub Uuid);

#[derive(
    Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel, DbInsert, DbPatch, DbDelete,
)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(query_ops(eq, any))]
    pub id: WidgetId,

    #[db(tenant)]
    #[db(pgtype = "uuid")]
    pub tenant_id: TenantId,

    #[db(pgtype = "text")]
    pub name: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}

#[derive(Clone)]
struct TestState {
    db: sqlx::PgPool,
}

impl DbState for TestState {
    fn pool(&self) -> &sqlx::PgPool {
        &self.db
    }
}

#[derive(Clone)]
struct TestSession;

impl Session for TestSession {
    type TenantId = TenantId;

    fn has_permission(&self, _permission: &Permission<TenantId>) -> bool {
        true
    }
}

impl FromRequestParts<TestState> for TestSession {
    type Rejection = httpkit::ApiError;

    async fn from_request_parts(
        _parts: &mut Parts,
        _state: &TestState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self)
    }
}

httpkit::crud! {
    model = Widget,
    path = "/widgets",
    state = TestState,
    session = TestSession,

    query { scope = "widget", action = "read" },
    insert { scope = "widget", action = "create" },
    patch { scope = "widget", action = "update" },
    delete { scope = "widget", action = "delete" },
}

httpkit::crud! {
    model = Widget,
    path = "/widgets-readonly",
    state = TestState,
    session = TestSession,

    query { scope = "widget", action = "read" },
}

fn state() -> TestState {
    TestState {
        db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/none")
            .unwrap(),
    }
}

#[tokio::test]
async fn the_full_router_builds() {
    let _: axum::Router = httpkit_routes_widgets::routes(state());
}

#[tokio::test]
async fn an_omitted_operation_generates_only_the_declared_route() {
    let _: axum::Router = httpkit_routes_widgets_readonly::routes(state());
}
