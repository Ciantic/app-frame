use chrono::Utc;
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

struct Model {
    table: &'static str,
    shape: &'static Shape,
    insert: Insert,
    decode: Decode,
}

const MODELS: &[Model] = &[
    Model {
        table: "user",
        shape: User::SHAPE,
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
    Model {
        table: "user_session",
        shape: UserSession::SHAPE,
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
