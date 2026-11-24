use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::Patch;
use dbkit::PatchError;
use pgtest_server::PgtestServer;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, DbModel, DbPatch, sqlx::FromRow)]
#[expect(dead_code, reason = "the fields exist to derive the table metadata")]
#[db(table = "widget_live")]
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

const SCHEMA: &str = r#"
    create table "widget_live" (
        "id" uuid not null,
        "name" text not null,
        "note" text,
        "updated_at" timestamptz not null,
        "version" bigint not null default 0,
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

    for (number, name, note) in [(1, "one", None), (2, "two", Some("keep"))] {
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

    db
}

async fn row(db: &mut sqlx::PgConnection, id: Uuid) -> WidgetLive {
    sqlx::query_as(r#"select * from "widget_live" where "id" = $1"#)
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_batch_updates_every_row() {
    let db = seeded().await;

    let first = WidgetLivePatch {
        id: id(1),
        name: Patch::Set("one updated".to_owned()),
        note: Patch::Absent,
        version: 0,
    };
    let second = WidgetLivePatch {
        id: id(2),
        name: Patch::Absent,
        note: Patch::Null,
        version: 0,
    };

    WidgetLive::patch([first, second])
        .execute(&db)
        .await
        .unwrap();

    let mut connection = db.acquire().await.unwrap();
    let first = row(&mut connection, id(1)).await;
    let second = row(&mut connection, id(2)).await;

    assert_eq!(first.name, "one updated");
    assert_eq!(first.note, None);
    assert_eq!(first.version, 1);
    assert!(first.updated_at > old_time());

    assert_eq!(second.name, "two");
    assert_eq!(second.note, None, "the patch cleared it");
    assert_eq!(second.version, 1);
}

#[tokio::test]
async fn a_stale_version_rolls_back_the_earlier_entries() {
    let db = seeded().await;

    let first = WidgetLivePatch {
        id: id(1),
        name: Patch::Set("one updated".to_owned()),
        note: Patch::Absent,
        version: 0,
    };
    let second = WidgetLivePatch {
        id: id(2),
        name: Patch::Set("two updated".to_owned()),
        note: Patch::Absent,
        version: 99,
    };

    let error = WidgetLive::patch([first, second])
        .execute(&db)
        .await
        .unwrap_err();

    assert!(
        matches!(error, PatchError::NotUpdated { index: 1 }),
        "{error}"
    );

    let mut connection = db.acquire().await.unwrap();
    let first = row(&mut connection, id(1)).await;
    let second = row(&mut connection, id(2)).await;

    assert_eq!(first.name, "one");
    assert_eq!(first.version, 0);
    assert_eq!(second.name, "two");
    assert_eq!(second.version, 0);
}

#[tokio::test]
async fn a_nested_batch_uses_a_savepoint() {
    let db = seeded().await;
    let mut transaction = db.begin().await.unwrap();

    let applied = WidgetLivePatch {
        id: id(1),
        name: Patch::Set("one updated".to_owned()),
        note: Patch::Absent,
        version: 0,
    };
    let stale = WidgetLivePatch {
        id: id(2),
        name: Patch::Set("two updated".to_owned()),
        note: Patch::Absent,
        version: 99,
    };

    WidgetLive::patch([applied])
        .execute(&mut *transaction)
        .await
        .unwrap();

    let error = WidgetLive::patch([stale])
        .execute(&mut *transaction)
        .await
        .unwrap_err();
    assert!(
        matches!(error, PatchError::NotUpdated { index: 0 }),
        "{error}"
    );

    let within = row(&mut transaction, id(1)).await;
    assert_eq!(within.name, "one updated");
    assert_eq!(within.version, 1);

    transaction.rollback().await.unwrap();

    let mut connection = db.acquire().await.unwrap();
    let after = row(&mut connection, id(1)).await;
    assert_eq!(after.name, "one");
    assert_eq!(after.version, 0);
}

#[tokio::test]
async fn a_deleted_row_is_not_updated() {
    let db = seeded().await;

    sqlx::query(r#"delete from "widget_live" where "id" = $1"#)
        .bind(id(2))
        .execute(&db)
        .await
        .unwrap();

    let patch = WidgetLivePatch {
        id: id(2),
        name: Patch::Set("two updated".to_owned()),
        note: Patch::Absent,
        version: 0,
    };

    let error = WidgetLive::patch([patch]).execute(&db).await.unwrap_err();

    assert!(
        matches!(error, PatchError::NotUpdated { index: 0 }),
        "{error}"
    );
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() {
    let db = seeded().await;

    WidgetLive::patch([]).execute(&db).await.unwrap();
}
