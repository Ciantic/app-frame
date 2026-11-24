use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dbkit::DbDelete;
use dbkit::DbModel;
use dbkit::DeleteError;
use pgtest_server::PgtestServer;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, DbModel, DbDelete, sqlx::FromRow)]
#[db(table = "widget_live")]
#[expect(dead_code, reason = "the fields exist to derive the table metadata")]
struct WidgetLive {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "text")]
    name: String,

    #[db(pgtype = "text")]
    note: Option<String>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[derive(Debug, Clone, DbModel, DbDelete, sqlx::FromRow)]
#[db(table = "note_live")]
#[expect(dead_code, reason = "the fields exist to derive the table metadata")]
struct NoteLive {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "text")]
    body: String,
}

const SCHEMA: &str = r#"
    create table "widget_live" (
        "id" uuid not null,
        "name" text not null,
        "note" text,
        "updated_at" timestamptz not null,
        "version" bigint not null default 0,
        primary key ("id")
    );

    create table "note_live" (
        "id" uuid not null,
        "body" text not null,
        primary key ("id")
    )
"#;

fn id(number: u128) -> Uuid {
    Uuid::from_u128(number)
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

async fn seeded() -> PgPool {
    let db = database().await;

    for (number, name, note) in [
        (1, "one", None),
        (2, "two", Some("keep")),
        (3, "three", None),
    ] {
        sqlx::query(
            r#"insert into "widget_live" ("id", "name", "note", "updated_at", "version")
               values ($1, $2, $3, $4, 0)"#,
        )
        .bind(id(number))
        .bind(name)
        .bind(note)
        .bind(old_time())
        .execute(&db)
        .await
        .unwrap();
    }

    for (number, body) in [(1, "first"), (2, "second")] {
        sqlx::query(r#"insert into "note_live" ("id", "body") values ($1, $2)"#)
            .bind(id(number))
            .bind(body)
            .execute(&db)
            .await
            .unwrap();
    }

    db
}

async fn count(db: &PgPool) -> i64 {
    sqlx::query_scalar(r#"select count(*) from "widget_live""#)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn note_count(db: &PgPool) -> i64 {
    sqlx::query_scalar(r#"select count(*) from "note_live""#)
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_batch_deletes_every_row() {
    let db = seeded().await;

    WidgetLive::delete([
        WidgetLivePKV {
            id: id(1),
            version: 0,
        },
        WidgetLivePKV {
            id: id(2),
            version: 0,
        },
        WidgetLivePKV {
            id: id(3),
            version: 0,
        },
    ])
    .execute(&db)
    .await
    .unwrap();

    assert_eq!(count(&db).await, 0);
}

#[tokio::test]
async fn a_missing_row_rolls_back_the_earlier_deletes() {
    let db = seeded().await;

    let error = WidgetLive::delete([
        WidgetLivePKV {
            id: id(1),
            version: 0,
        },
        WidgetLivePKV {
            id: id(2),
            version: 0,
        },
        WidgetLivePKV {
            id: id(3),
            version: 0,
        },
        WidgetLivePKV {
            id: id(4),
            version: 0,
        },
    ])
    .execute(&db)
    .await
    .unwrap_err();

    assert!(
        matches!(error, DeleteError::NotDeleted { index: 3 }),
        "{error}"
    );
    assert_eq!(
        count(&db).await,
        3,
        "the three earlier deletes were rolled back"
    );
}

#[tokio::test]
async fn a_stale_version_rolls_back_the_earlier_deletes() {
    let db = seeded().await;

    let error = WidgetLive::delete([
        WidgetLivePKV {
            id: id(1),
            version: 0,
        },
        WidgetLivePKV {
            id: id(2),
            version: 99,
        },
    ])
    .execute(&db)
    .await
    .unwrap_err();

    assert!(
        matches!(error, DeleteError::NotDeleted { index: 1 }),
        "{error}"
    );
    assert_eq!(count(&db).await, 3);
}

#[tokio::test]
async fn a_duplicate_key_rolls_back_the_batch() {
    let db = seeded().await;

    let error = WidgetLive::delete([
        WidgetLivePKV {
            id: id(1),
            version: 0,
        },
        WidgetLivePKV {
            id: id(1),
            version: 0,
        },
    ])
    .execute(&db)
    .await
    .unwrap_err();

    assert!(
        matches!(error, DeleteError::NotDeleted { index: 1 }),
        "{error}"
    );
    assert_eq!(count(&db).await, 3);
}

#[tokio::test]
async fn a_nested_batch_uses_a_savepoint() {
    let db = seeded().await;
    let mut transaction = db.begin().await.unwrap();

    WidgetLive::delete([WidgetLivePKV {
        id: id(1),
        version: 0,
    }])
    .execute(&mut *transaction)
    .await
    .unwrap();

    let error = WidgetLive::delete([WidgetLivePKV {
        id: id(2),
        version: 99,
    }])
    .execute(&mut *transaction)
    .await
    .unwrap_err();
    assert!(
        matches!(error, DeleteError::NotDeleted { index: 0 }),
        "{error}"
    );

    let within: i64 = sqlx::query_scalar(r#"select count(*) from "widget_live""#)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    assert_eq!(
        within, 2,
        "the first delete survived the savepoint rollback"
    );

    transaction.rollback().await.unwrap();

    assert_eq!(count(&db).await, 3);
}

#[tokio::test]
async fn an_unversioned_model_deletes_by_key() {
    let db = seeded().await;

    NoteLive::delete([NoteLivePKV { id: id(1) }])
        .execute(&db)
        .await
        .unwrap();

    assert_eq!(note_count(&db).await, 1);
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() {
    let db = seeded().await;

    WidgetLive::delete([]).execute(&db).await.unwrap();

    assert_eq!(count(&db).await, 3);
}
