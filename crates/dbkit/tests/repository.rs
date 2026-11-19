use chrono::DateTime;
use chrono::Utc;
use dbkit::Model;
use dbkit::Patch;
use dbkit::Patchable;
use dbkit::RepoError;
use dbkit::RepoResult;
use dbkit::Update;
use dbkit::find;
use dbkit::in_transaction;
use dbkit::update;
use pgtest_server::PgtestServer;
use serde::Deserialize;
use serde::Serialize;
use sqlx::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

const SCHEMA: &str = r#"
    create table "widget" (
        "id" uuid not null default uuidv7(),
        "name" text not null,
        "note" text,
        "created_at" timestamptz not null default now(),
        "updated_at" timestamptz not null default now(),
        "version" int8 not null default 0,
        constraint "widget_pkey" primary key ("id")
    )
"#;

#[derive(Debug, Clone, FromRow)]
struct Widget {
    id: Uuid,
    name: String,
    note: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

impl Model for Widget {
    type Id = Uuid;
    type Patch = WidgetPatch;

    const COLUMNS: &'static [&'static str] =
        &["id", "name", "note", "created_at", "updated_at", "version"];
    const CREATED_AT: Option<&'static str> = Some("created_at");
    const ID: &'static str = "id";
    const TABLE: &'static str = "widget";
    const UPDATED_AT: Option<&'static str> = Some("updated_at");
    const VERSION: &'static str = "version";

    fn version(&self) -> i64 {
        self.version
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WidgetPatch {
    #[serde(default, skip_serializing_if = "Patch::is_absent")]
    name: Patch<String>,
    #[serde(default, skip_serializing_if = "Patch::is_absent")]
    note: Patch<String>,
}

impl Patchable<Widget> for WidgetPatch {
    fn is_empty(&self) -> bool {
        self.name.is_absent() && self.note.is_absent()
    }

    fn bind(&self, update: &mut Update) -> RepoResult<()> {
        match &self.name {
            Patch::Absent => {}
            Patch::Set(name) => {
                update.set("name", name);
            }
            Patch::Null => return Err(RepoError::InvalidInput("name is not nullable")),
        }

        match &self.note {
            Patch::Absent => {}
            Patch::Set(note) => {
                update.set("note", note);
            }
            Patch::Null => {
                update.set("note", None::<String>);
            }
        }

        Ok(())
    }
}

async fn database() -> PgPool {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
    db
}

async fn insert_widget(pool: &PgPool, id: Uuid) -> Widget {
    sqlx::query_as::<_, Widget>(
        r#"INSERT INTO "widget" ("id", "name", "note") VALUES ($1, $2, $3) RETURNING *"#,
    )
    .bind(id)
    .bind("Thingie")
    .bind("first")
    .fetch_one(pool)
    .await
    .unwrap()
}

fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

#[tokio::test]
async fn find_returns_the_row() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;

    let found = find::<Widget>(&mut db.acquire().await.unwrap(), id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(found.name, inserted.name);
}

#[tokio::test]
async fn find_returns_none_for_a_missing_row() {
    let db = database().await;

    let found = find::<Widget>(&mut db.acquire().await.unwrap(), Uuid::from_u128(9))
        .await
        .unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn update_applies_the_patch() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;
    let patch = WidgetPatch {
        name: Patch::Set("Renamed".to_owned()),
        note: Patch::Set("second".to_owned()),
    };

    let updated = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        id,
        inserted.version,
        &patch,
    )
    .await
    .unwrap();

    assert_eq!(updated.id, id);
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.note.as_deref(), Some("second"));
    assert_eq!(updated.version, inserted.version + 1);
    assert_eq!(updated.updated_at, now());
    assert_eq!(updated.created_at, inserted.created_at);
}

#[tokio::test]
async fn update_clears_a_nullable_column() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;
    let patch = WidgetPatch {
        name: Patch::Absent,
        note: Patch::Null,
    };

    let updated = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        id,
        inserted.version,
        &patch,
    )
    .await
    .unwrap();

    assert_eq!(updated.note, None);
    assert_eq!(updated.name, "Thingie");
}

#[tokio::test]
async fn update_rejects_null_for_a_required_column() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;
    let patch = WidgetPatch {
        name: Patch::Null,
        note: Patch::Absent,
    };

    let error = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        id,
        inserted.version,
        &patch,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RepoError::InvalidInput(_)), "{error}");
}

#[tokio::test]
async fn update_rejects_an_empty_patch() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;

    let error = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        id,
        inserted.version,
        &WidgetPatch::default(),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RepoError::InvalidInput(_)), "{error}");
}

#[tokio::test]
async fn update_reports_a_stale_version_as_a_conflict() {
    let db = database().await;
    let id = Uuid::from_u128(1);
    let inserted = insert_widget(&db, id).await;
    let patch = WidgetPatch {
        name: Patch::Set("Renamed".to_owned()),
        ..WidgetPatch::default()
    };

    let error = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        id,
        inserted.version + 1,
        &patch,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RepoError::Conflict), "{error}");
}

#[tokio::test]
async fn update_reports_a_missing_row_as_not_found() {
    let db = database().await;
    let patch = WidgetPatch {
        name: Patch::Set("Renamed".to_owned()),
        ..WidgetPatch::default()
    };

    let error = update::<Widget>(
        &mut db.acquire().await.unwrap(),
        now(),
        Uuid::from_u128(9),
        0,
        &patch,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RepoError::NotFound), "{error}");
}

#[tokio::test]
async fn transaction_commits_on_success() {
    let db = database().await;
    let id = Uuid::from_u128(1);

    let inserted: RepoResult<()> = in_transaction(&db, async |conn| {
        sqlx::query(r#"INSERT INTO "widget" ("id", "name") VALUES ($1, $2)"#)
            .bind(id)
            .bind("Thingie")
            .execute(&mut *conn)
            .await?;

        Ok(())
    })
    .await;

    assert!(inserted.is_ok(), "{:?}", inserted.err());
    assert!(
        find::<Widget>(&mut db.acquire().await.unwrap(), id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn transaction_rolls_back_on_error() {
    let db = database().await;
    let id = Uuid::from_u128(1);

    let outcome: RepoResult<()> = in_transaction(&db, async |conn| {
        sqlx::query(r#"INSERT INTO "widget" ("id", "name") VALUES ($1, $2)"#)
            .bind(id)
            .bind("Thingie")
            .execute(&mut *conn)
            .await?;

        Err(RepoError::InvalidInput("abort"))
    })
    .await;

    assert!(
        matches!(outcome, Err(RepoError::InvalidInput(_))),
        "{outcome:?}"
    );
    assert!(
        find::<Widget>(&mut db.acquire().await.unwrap(), id)
            .await
            .unwrap()
            .is_none()
    );
}
