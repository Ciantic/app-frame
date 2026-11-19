use email_service::db::models::email::Email;
use email_service::db::models::email::EmailId;
use email_service::db::schema::SCHEMA;
use facet::Facet;
use facet::FieldFlags;
use facet::Shape;
use facet::Type;
use facet::UserType;
use pgtest_server::PgtestServer;
use sqlx::FromRow;
use sqlx::Postgres;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgRow;
use sqlx::query::Query;
use std::collections::BTreeSet;
use uuid::Uuid;

type Decode = fn(&PgRow) -> Result<(), String>;
type Insert = fn() -> Query<'static, Postgres, PgArguments>;

struct Model {
    table: &'static str,
    shape: &'static Shape,
    insert: Insert,
    decode: Decode,
}

const MODELS: &[Model] = &[Model {
    table: "email",
    shape: Email::SHAPE,
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

#[tokio::test]
async fn models_and_schema_share_columns() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();
    let mut problems = Vec::new();

    for model in MODELS {
        let Type::User(UserType::Struct(struct_type)) = model.shape.ty else {
            panic!("{} is not a struct", model.shape.type_identifier);
        };
        let model_columns: BTreeSet<&str> = struct_type
            .fields
            .iter()
            .filter(|field| !field.flags.contains(FieldFlags::SKIP))
            .map(|field| field.rename.unwrap_or(field.name))
            .collect();

        let schema_columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(model.table)
        .fetch_all(&db)
        .await
        .unwrap();
        let schema_columns: BTreeSet<&str> = schema_columns.iter().map(String::as_str).collect();

        for column in &schema_columns - &model_columns {
            problems.push(format!(
                "table \"{}\" has column \"{column}\" without a model field",
                model.table
            ));
        }
        for column in &model_columns - &schema_columns {
            problems.push(format!("{}: field \"{column}\" has no column", model.table));
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
        if !MODELS.iter().any(|model| model.table == table) {
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

    for model in MODELS {
        let row = (model.insert)()
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", model.table));

        (model.decode)(&row).unwrap_or_else(|error| {
            panic!("{}: inserted row does not decode: {error}", model.table)
        });
    }
}

#[tokio::test]
async fn models_select() {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(SCHEMA).execute(&db).await.unwrap();

    for model in MODELS {
        (model.insert)()
            .execute(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: insert failed: {error}", model.table));

        let sql = format!("SELECT * FROM \"{}\"", model.table);
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_one(&db)
            .await
            .unwrap_or_else(|error| panic!("{}: select failed: {error}", model.table));

        (model.decode)(&row).unwrap_or_else(|error| {
            panic!("{}: selected row does not decode: {error}", model.table)
        });
    }
}
