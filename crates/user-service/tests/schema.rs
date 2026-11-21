use chrono::Utc;
use dbkit::Model;
use dbkit::TableMeta;
use pgtest_server::PgtestServer;
use sqlx::FromRow;
use sqlx::Postgres;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgRow;
use sqlx::query::Query;
use std::collections::BTreeMap;
use user_service::db::models::MODELS;
use user_service::db::models::user::User;
use user_service::db::models::user::UserId;
use user_service::db::models::user_session::UserSession;
use user_service::db::models::user_session::UserSessionId;
use user_service::db::schema::SCHEMA;
use uuid::Uuid;

// Fixed ids, so the session can reference the user without a round trip.
const USER_ID: Uuid = Uuid::from_u128(1);

type Decode = fn(&PgRow) -> Result<(), String>;
type Insert = fn() -> Query<'static, Postgres, PgArguments>;

struct Fixture {
    meta: TableMeta,
    insert: Insert,
    decode: Decode,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        meta: User::META,
        insert: || {
            let user = User {
                id: UserId(USER_ID),
                email: "sample@example.com".to_owned(),
                email_verified_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                deleted_at: None,
                version: 0,
            };

            sqlx::query::<Postgres>(
                r#"INSERT INTO "user"
                   (id, email, email_verified_at, created_at, updated_at, deleted_at, version)
                   VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *"#,
            )
            .bind(user.id)
            .bind(user.email)
            .bind(user.email_verified_at)
            .bind(user.created_at)
            .bind(user.updated_at)
            .bind(user.deleted_at)
            .bind(user.version)
        },
        decode: |row| {
            User::from_row(row)
                .map(drop)
                .map_err(|error| error.to_string())
        },
    },
    Fixture {
        meta: UserSession::META,
        insert: || {
            let session = UserSession {
                id: UserSessionId(Uuid::from_u128(2)),
                user_id: UserId(USER_ID),
                token_hash: "sample".to_owned(),
                expires_at: Utc::now(),
                revoked_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                version: 0,
            };

            sqlx::query::<Postgres>(
                r#"INSERT INTO "user_session"
                   (id, user_id, token_hash, expires_at, revoked_at, created_at, updated_at, version)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *"#,
            )
            .bind(session.id)
            .bind(session.user_id)
            .bind(session.token_hash)
            .bind(session.expires_at)
            .bind(session.revoked_at)
            .bind(session.created_at)
            .bind(session.updated_at)
            .bind(session.version)
        },
        decode: |row| {
            UserSession::from_row(row)
                .map(drop)
                .map_err(|error| error.to_string())
        },
    },
];

#[test]
fn every_model_has_an_insert_fixture() {
    for meta in MODELS {
        assert!(
            FIXTURES
                .iter()
                .any(|fixture| fixture.meta.table == meta.table),
            "no insert fixture for table \"{}\"",
            meta.table
        );
    }
}

#[tokio::test]
async fn models_and_schema_share_columns() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
    let mut problems = Vec::new();

    for meta in MODELS {
        let model_columns: BTreeMap<&str, bool> = meta
            .columns
            .iter()
            .map(|column| (column.name, column.nullable))
            .collect();

        let schema_columns: Vec<(String, String)> = sqlx::query_as(
            "SELECT column_name, is_nullable FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(meta.table)
        .fetch_all(&db)
        .await
        .unwrap();
        let schema_columns: BTreeMap<String, bool> = schema_columns
            .into_iter()
            .map(|(name, nullable)| (name, nullable == "YES"))
            .collect();

        for (column, nullable) in &schema_columns {
            match model_columns.get(column.as_str()) {
                None => problems.push(format!(
                    "table \"{}\" has column \"{column}\" without a model field",
                    meta.table
                )),
                Some(expected) if expected != nullable => problems.push(format!(
                    "{}: column \"{column}\" is nullable = {nullable}, the model says {expected}",
                    meta.table
                )),
                Some(_) => {}
            }
        }
        for column in model_columns.keys() {
            if !schema_columns.contains_key(*column) {
                problems.push(format!("{}: field \"{column}\" has no column", meta.table));
            }
        }
    }

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    for table in tables {
        if !MODELS.iter().any(|meta| meta.table == table) {
            problems.push(format!("table \"{table}\" has no model"));
        }
    }

    assert!(
        problems.is_empty(),
        "model/schema mismatch:\n{}",
        problems.join("\n")
    );
}

#[tokio::test]
async fn models_insert() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();

    for fixture in FIXTURES {
        let row = (fixture.insert)()
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", fixture.meta.table));

        (fixture.decode)(&row).unwrap_or_else(|error| {
            panic!(
                "{}: inserted row does not decode: {error}",
                fixture.meta.table
            )
        });
    }
}

#[tokio::test]
async fn models_select() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();

    for fixture in FIXTURES {
        (fixture.insert)()
            .execute(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", fixture.meta.table));

        let sql = format!("SELECT * FROM \"{}\"", fixture.meta.table);
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: select failed: {error}", fixture.meta.table));

        (fixture.decode)(&row).unwrap_or_else(|error| {
            panic!(
                "{}: selected row does not decode: {error}",
                fixture.meta.table
            )
        });
    }
}
