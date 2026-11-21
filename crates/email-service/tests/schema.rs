use dbkit::Model;
use dbkit::TableMeta;
use email_service::db::models::MODELS;
use email_service::db::models::email::Email;
use email_service::db::models::email::EmailId;
use email_service::db::schema::SCHEMA;
use pgtest_server::PgtestServer;
use sqlx::FromRow;
use sqlx::Postgres;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgRow;
use sqlx::query::Query;
use std::collections::BTreeMap;
use uuid::Uuid;

type Decode = fn(&PgRow) -> Result<(), String>;
type Insert = fn() -> Query<'static, Postgres, PgArguments>;

struct Fixture {
    meta: TableMeta,
    insert: Insert,
    decode: Decode,
}

const FIXTURES: &[Fixture] = &[Fixture {
    meta: Email::META,
    insert: || {
        let email = Email {
            id: EmailId(Uuid::from_u128(1)),
            from: "sender@example.com".to_owned(),
            to: "recipient@example.com".to_owned(),
            subject: "Sample".to_owned(),
            body: "Hello".to_owned(),
            sent_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            version: 0,
        };

        sqlx::query::<Postgres>(
            r#"INSERT INTO "email"
               (id, "from", "to", subject, body, sent_at, created_at, updated_at, version)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING *"#,
        )
        .bind(email.id)
        .bind(email.from)
        .bind(email.to)
        .bind(email.subject)
        .bind(email.body)
        .bind(email.sent_at)
        .bind(email.created_at)
        .bind(email.updated_at)
        .bind(email.version)
    },
    decode: |row| {
        Email::from_row(row)
            .map(drop)
            .map_err(|error| error.to_string())
    },
}];

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
