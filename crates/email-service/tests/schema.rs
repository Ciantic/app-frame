use dbkit::Model;
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
use uuid::Uuid;

type Decode = fn(&PgRow) -> Result<(), String>;
type Insert = fn() -> Query<'static, Postgres, PgArguments>;

struct Fixture {
    meta: fn() -> dbkit::Meta,
    insert: Insert,
    decode: Decode,
}

const FIXTURES: &[Fixture] = &[Fixture {
    meta: Email::meta,
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
    for meta in MODELS.iter() {
        assert!(
            FIXTURES
                .iter()
                .any(|fixture| (fixture.meta)().into_table().table == meta.table),
            "no insert fixture for table \"{}\"",
            meta.table
        );
    }
}

#[tokio::test]
async fn models_and_schema_share_columns() {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();
    let mut connection = db.acquire().await.unwrap();

    let problems = SCHEMA.check(&mut connection).await.unwrap();

    assert!(
        problems.is_empty(),
        "model/schema mismatch:\n{}",
        problems.join("\n")
    );
}

#[tokio::test]
async fn models_insert() {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();

    for fixture in FIXTURES {
        let meta = (fixture.meta)().into_table();
        let row = (fixture.insert)()
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", meta.table));

        (fixture.decode)(&row).unwrap_or_else(|error| {
            panic!("{}: inserted row does not decode: {error}", meta.table)
        });
    }
}

#[tokio::test]
async fn models_select() {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();

    for fixture in FIXTURES {
        let meta = (fixture.meta)().into_table();
        (fixture.insert)()
            .execute(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", meta.table));

        let sql = format!("SELECT * FROM \"{}\"", meta.table);
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: select failed: {error}", meta.table));

        (fixture.decode)(&row).unwrap_or_else(|error| {
            panic!("{}: selected row does not decode: {error}", meta.table)
        });
    }
}
