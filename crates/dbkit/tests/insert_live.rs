use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dbkit::DbInsert;
use dbkit::DbModel;
use dbkit::DbType;
use dbkit::InsertError;
use pgtest_server::PgtestServer;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, DbType,
)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "gen_random_uuid()")]
#[sqlx(transparent)]
pub struct WidgetLiveId(pub Uuid);

#[derive(Debug, Clone, DbModel, DbInsert, sqlx::FromRow)]
#[db(table = "translation_live")]
struct TranslationLive {
    #[db(primary_key)]
    #[db(pgtype = "text")]
    lang: String,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    key: String,

    #[db(pgtype = "text")]
    value: String,
}

#[derive(Debug, Clone, DbModel, DbInsert, sqlx::FromRow)]
#[db(table = "widget_live")]
struct WidgetLive {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    id: WidgetLiveId,

    #[db(pgtype = "text")]
    name: String,

    #[db(pgtype = "text")]
    note: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    version: i64,
}

const SCHEMA: &str = r#"
    create table "widget_live" (
        "id" uuid not null default gen_random_uuid(),
        "name" text not null,
        "note" text,
        "created_at" timestamptz not null default now(),
        "updated_at" timestamptz not null default now(),
        "version" bigint not null default 0,
        primary key ("id")
    );

    create table "translation_live" (
        "lang" text not null,
        "key" text not null,
        "value" text not null,
        primary key ("lang", "key")
    )
"#;

fn id(number: u128) -> WidgetLiveId {
    WidgetLiveId(Uuid::from_u128(number))
}

fn old_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()
}

async fn database() -> PgPool {
    let db = PgtestServer::start().await.database().await;

    sqlx::raw_sql(AssertSqlSafe(SCHEMA.to_owned()))
        .execute(&db)
        .await
        .unwrap();

    db
}

async fn count(db: &PgPool) -> i64 {
    sqlx::query_scalar(r#"select count(*) from "widget_live""#)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn row(db: &PgPool, id: WidgetLiveId) -> WidgetLive {
    sqlx::query_as(r#"select * from "widget_live" where "id" = $1"#)
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn one_row_takes_the_database_defaults() {
    let db = database().await;

    let keys = WidgetLive::insert([WidgetLiveInsert::new("one")])
        .execute(&db)
        .await
        .unwrap();

    assert_eq!(keys.len(), 1);
    let inserted = row(&db, keys[0].id).await;

    assert_ne!(inserted.id.0, Uuid::nil());
    assert_eq!(inserted.name, "one");
    assert_eq!(inserted.note, None);
    assert_eq!(inserted.version, 0);
    assert!(inserted.created_at <= Utc::now());
    assert!(inserted.updated_at <= Utc::now());
}

#[tokio::test]
async fn several_keys_return_in_input_order() {
    let db = database().await;

    let keys = WidgetLive::insert([
        WidgetLiveInsert::new("one"),
        WidgetLiveInsert::new("two").set_note("kept"),
    ])
    .execute(&db)
    .await
    .unwrap();

    assert_eq!(keys.len(), 2);
    let first = row(&db, keys[0].id).await;
    let second = row(&db, keys[1].id).await;
    assert_eq!(first.name, "one");
    assert_eq!(first.note, None);
    assert_eq!(second.name, "two");
    assert_eq!(second.note.as_deref(), Some("kept"));
}

#[tokio::test]
async fn explicit_values_are_preserved() {
    let db = database().await;
    let id = id(1);
    let at = old_time();

    let keys = WidgetLive::insert([WidgetLiveInsert::new("one")
        .set_id(id)
        .set_created_at(at)
        .set_updated_at(at)
        .set_version(7)])
    .execute(&db)
    .await
    .unwrap();

    assert_eq!(keys[0].id, id);

    let inserted = row(&db, id).await;
    assert_eq!(inserted.created_at, at);
    assert_eq!(inserted.updated_at, at);
    assert_eq!(inserted.version, 7);
}

#[tokio::test]
async fn a_supplied_created_at_also_supplies_updated_at() {
    let db = database().await;
    let at = old_time();

    let keys = WidgetLive::insert([WidgetLiveInsert::new("one").set_created_at(at)])
        .execute(&db)
        .await
        .unwrap();

    let inserted = row(&db, keys[0].id).await;
    assert_eq!(inserted.created_at, at);
    assert_eq!(inserted.updated_at, at);
}

#[tokio::test]
async fn a_unique_violation_rolls_back_the_earlier_entries() {
    let db = database().await;
    let id = id(1);

    let error = WidgetLive::insert([
        WidgetLiveInsert::new("one").set_id(id),
        WidgetLiveInsert::new("two").set_id(id),
    ])
    .execute(&db)
    .await
    .unwrap_err();

    assert!(matches!(error, InsertError::Sqlx(_)), "{error}");
    assert_eq!(count(&db).await, 0);
}

#[tokio::test]
async fn a_nested_batch_uses_a_savepoint() {
    let db = database().await;
    let mut transaction = db.begin().await.unwrap();
    let id = id(1);

    WidgetLive::insert([WidgetLiveInsert::new("one").set_id(id)])
        .execute(&mut *transaction)
        .await
        .unwrap();

    let error = WidgetLive::insert([WidgetLiveInsert::new("two").set_id(id)])
        .execute(&mut *transaction)
        .await
        .unwrap_err();
    assert!(matches!(error, InsertError::Sqlx(_)), "{error}");

    let within: i64 = sqlx::query_scalar(r#"select count(*) from "widget_live""#)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        within, 1,
        "the first insert survived the savepoint rollback"
    );

    transaction.rollback().await.unwrap();

    assert_eq!(count(&db).await, 0);
}

#[tokio::test]
async fn a_composite_key_inserts_and_returns_its_key() {
    let db = database().await;

    let keys = TranslationLive::insert([TranslationLiveInsert::new("en", "greeting", "hi")])
        .execute(&db)
        .await
        .unwrap();

    assert_eq!(keys[0].lang, "en");
    assert_eq!(keys[0].key, "greeting");

    let inserted: TranslationLive =
        sqlx::query_as(r#"select * from "translation_live" where "lang" = $1 and "key" = $2"#)
            .bind(&keys[0].lang)
            .bind(&keys[0].key)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(inserted.lang, "en");
    assert_eq!(inserted.key, "greeting");
    assert_eq!(inserted.value, "hi");
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() {
    let db = database().await;

    let rows = WidgetLive::insert([]).execute(&db).await.unwrap();

    assert!(rows.is_empty());
    assert_eq!(count(&db).await, 0);
}
