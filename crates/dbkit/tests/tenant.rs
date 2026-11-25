use dbkit::DbDelete;
use dbkit::DbInsert;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::DeleteKey;
use dbkit::Model;
use dbkit::TenantOwned;
use dbkit::TenantRow;
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

#[derive(Debug, Clone, Serialize, Deserialize, DbModel, DbInsert, DbPatch, DbDelete)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
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

#[derive(Debug, Clone, Serialize, Deserialize, DbModel, DbPatch, DbDelete)]
#[db(table = "membership")]
pub struct Membership {
    #[db(primary_key)]
    #[db(tenant)]
    #[db(pgtype = "uuid")]
    pub tenant_id: TenantId,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    pub key: String,

    #[db(pgtype = "text")]
    pub value: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}

fn id() -> WidgetId {
    WidgetId(Uuid::from_u128(1))
}

fn tenant() -> TenantId {
    TenantId(Uuid::from_u128(2))
}

#[test]
fn metadata_names_the_tenant_column() {
    let meta = Widget::meta();

    assert_eq!(meta.tenant(), Some("tenant_id"));
    assert_eq!(Widget::TENANT_COLUMN, "tenant_id");
}

#[test]
fn the_key_carries_the_tenant() {
    let key = WidgetPKV {
        id: id(),
        tenant_id: tenant(),
        version: 3,
    };

    assert_eq!(key.key().len(), 1);
    assert!(key.tenant().is_some());
}

#[test]
fn insert_carries_the_tenant() {
    let insert = WidgetInsert::new(id(), tenant(), "sample");

    assert!(insert.tenant().is_some());
}

#[test]
fn patch_requires_the_tenant_and_renders_a_predicate() {
    let patch = WidgetPatch::new(id(), tenant(), 3).set_name("sample");
    let rendered = Widget::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"widget\" SET \"name\" = $1, \"version\" = \"version\" + 1 WHERE \"id\" = $2 AND \
         \"tenant_id\" = $3 AND \"version\" = $4"
    );
    assert_eq!(
        rendered[0].values,
        vec![
            serde_json::json!("sample"),
            serde_json::json!(id()),
            serde_json::json!(tenant()),
            serde_json::json!(3),
        ]
    );
}

#[test]
fn delete_renders_a_tenant_predicate() {
    let key = WidgetPKV {
        id: id(),
        tenant_id: tenant(),
        version: 3,
    };
    let rendered = Widget::delete([key]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"widget\" WHERE \"id\" = $1 AND \"tenant_id\" = $2 AND \"version\" = $3"
    );
    assert_eq!(
        rendered[0].values,
        vec![
            serde_json::json!(id()),
            serde_json::json!(tenant()),
            serde_json::json!(3),
        ]
    );
}

#[test]
fn a_tenant_primary_key_is_not_rendered_twice() {
    let key = MembershipPKV {
        tenant_id: tenant(),
        key: "sample".to_owned(),
        version: 3,
    };
    let rendered = Membership::delete([key]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "DELETE FROM \"membership\" WHERE \"tenant_id\" = $1 AND \"key\" = $2 AND \"version\" = $3"
    );
}

#[test]
fn a_tenant_primary_key_patch_does_not_repeat_the_predicate() {
    let patch = MembershipPatch::new(tenant(), "sample".to_owned(), 3).set_value("new");
    let rendered = Membership::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"membership\" SET \"value\" = $1, \"version\" = \"version\" + 1 WHERE \
         \"tenant_id\" = $2 AND \"key\" = $3 AND \"version\" = $4"
    );
}
