use dbkit::DbModel;
use dbkit::Model;
use pgtest_server::PgtestServer;
use uuid::Uuid;

#[derive(DbModel)]
#[db(table = "widget")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Widget {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(pgtype = "varchar(255)")]
    name: String,
    #[db(pgtype = "text")]
    note: Option<String>,
}

#[derive(DbModel)]
#[db(table = "broken")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Broken {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(pgtype = "no_such_type")]
    name: String,
}

/// Returns the problems `mismatches` reports for `schema`.
async fn problems(schema: &str, meta: &[dbkit::TableMeta]) -> Vec<String> {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(schema.to_owned()))
        .execute(&db)
        .await
        .unwrap();
    let mut connection = db.acquire().await.unwrap();

    dbkit::schema::mismatches(&mut connection, meta)
        .await
        .unwrap()
}

const MATCHING: &str = r#"
    create table "widget" (
        "id" uuid not null,
        "name" varchar(255) not null,
        "note" text
    )
"#;

#[tokio::test]
async fn matching_schema_has_no_problems() {
    let problems: Vec<String> = problems(MATCHING, &[Widget::META]).await;

    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
async fn reports_type_mismatch() {
    let schema = MATCHING.replace("varchar(255)", "varchar(50)");

    assert_eq!(
        problems(&schema, &[Widget::META]).await,
        ["widget: column \"name\" is character varying(50), the model says varchar(255)"]
    );
}

#[tokio::test]
async fn reports_nullability_mismatch() {
    let schema = MATCHING.replace("\"note\" text", "\"note\" text not null");

    assert_eq!(
        problems(&schema, &[Widget::META]).await,
        ["widget: column \"note\" is nullable = false, the model says true"]
    );
}

#[tokio::test]
async fn reports_columns_without_a_model_field() {
    let schema = MATCHING.replace("\"note\" text", "\"note\" text, \"extra\" int4");

    assert_eq!(
        problems(&schema, &[Widget::META]).await,
        ["table \"widget\" has column \"extra\" without a model field"]
    );
}

#[tokio::test]
async fn reports_model_fields_without_a_column() {
    let schema = MATCHING.replace("\"note\" text", "\"other\" text");

    assert_eq!(
        problems(&schema, &[Widget::META]).await,
        [
            "table \"widget\" has column \"other\" without a model field",
            "widget: field \"note\" has no column",
        ]
    );
}

#[tokio::test]
async fn reports_tables_without_a_model() {
    let schema = format!("{MATCHING}; create table \"gadget\" (\"id\" uuid not null)");

    assert_eq!(
        problems(&schema, &[Widget::META]).await,
        ["table \"gadget\" has no model"]
    );
}

#[tokio::test]
async fn reports_an_invalid_pgtype() {
    let schema = r#"create table "broken" ("id" uuid not null, "name" text not null)"#;

    let problems = problems(schema, &[Broken::META]).await;

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("broken: `pgtype` is not a valid column type: "),
        "{problems:?}"
    );
}
