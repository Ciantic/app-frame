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
    #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
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

#[derive(DbModel)]
#[db(table = "broken_default")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct BrokenDefault {
    #[db(primary_key, pgtype = "uuid", pgdefault = "no_such_function()")]
    id: Uuid,
}

#[derive(DbModel)]
#[db(table = "generated")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Generated {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "int8")]
    net: i64,

    #[db(pgtype = "int8")]
    tax: i64,

    #[db(pgtype = "int8")]
    #[db(pg_virtual = "\"net\" + \"tax\"")]
    total: i64,
}

#[derive(DbModel)]
#[db(fragment)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the column metadata"
)]
struct Size {
    #[db(pgtype = "int4")]
    width: i32,
    #[db(pgtype = "int4")]
    height: i32,
}

#[derive(DbModel)]
#[db(table = "frame")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Frame {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(flatten)]
    size: Size,
    #[db(pgtype = "text")]
    label: String,
}

#[derive(DbModel)]
#[db(fragment)]
#[expect(
    dead_code,
    reason = "the field exists only to derive the column metadata"
)]
struct Left {
    #[db(pgtype = "text")]
    note: String,
}

#[derive(DbModel)]
#[db(fragment)]
#[expect(
    dead_code,
    reason = "the field exists only to derive the column metadata"
)]
struct Right {
    #[db(pgtype = "text")]
    note: String,
}

#[derive(DbModel)]
#[db(table = "duplicated")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Duplicated {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(flatten)]
    left: Left,
    #[db(flatten)]
    right: Right,
}

#[derive(DbModel)]
#[db(table = "parent")]
#[db(unique(columns = [tenant_id, id]))]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Parent {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "uuid")]
    tenant_id: Uuid,

    #[db(pgtype = "text")]
    #[db(unique)]
    name: String,
}

#[derive(DbModel)]
#[db(table = "child")]
#[db(pg_trigger = "child_trigger")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Child {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "uuid")]
    tenant_id: Uuid,

    #[db(pgtype = "uuid")]
    #[db(foreign_key(table = "parent", column = "id", scope = "tenant_id"))]
    parent_id: Uuid,
}

/// Returns the problems `check_live_schema_against_models` reports for `schema`.
async fn problems(schema: &str, meta: &[dbkit::TableMeta]) -> Vec<String> {
    let db = PgtestServer::start().await.database().await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(schema.to_owned()))
        .execute(&db)
        .await
        .unwrap();
    let mut connection = db.acquire().await.unwrap();

    dbkit::schema::check_live_schema_against_models(&mut connection, meta)
        .await
        .unwrap()
}

const MATCHING: &str = r#"
    create table "widget" (
        "id" uuid not null default uuidv7(),
        "name" varchar(255) not null,
        "note" text
    )
"#;

#[tokio::test]
async fn matching_schema_has_no_problems() {
    let problems: Vec<String> = problems(MATCHING, &[Widget::meta().into_table()]).await;

    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
async fn reports_type_mismatch() {
    let schema = MATCHING.replace("varchar(255)", "varchar(50)");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["widget: column \"name\" is character varying(50), the model says varchar(255)"]
    );
}

#[tokio::test]
async fn reports_nullability_mismatch() {
    let schema = MATCHING.replace("\"note\" text", "\"note\" text not null");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["widget: column \"note\" is nullable = false, the model says true"]
    );
}

#[tokio::test]
async fn reports_a_default_mismatch() {
    let schema = MATCHING.replace("default uuidv7()", "default gen_random_uuid()");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["widget: column \"id\" default is gen_random_uuid(), the model says uuidv7()"]
    );
}

#[tokio::test]
async fn reports_a_missing_default() {
    let schema = MATCHING.replace(" default uuidv7()", "");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["widget: column \"id\" default is none, the model says uuidv7()"]
    );
}

#[tokio::test]
async fn reports_an_undeclared_default() {
    let schema = MATCHING.replace("\"name\" varchar(255)", "\"name\" varchar(255) default ''");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["widget: column \"name\" default is ''::character varying, the model says none"]
    );
}

#[tokio::test]
async fn reports_columns_without_a_model_field() {
    let schema = MATCHING.replace("\"note\" text", "\"note\" text, \"extra\" int4");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["table \"widget\" has column \"extra\" without a model field"]
    );
}

#[tokio::test]
async fn reports_model_fields_without_a_column() {
    let schema = MATCHING.replace("\"note\" text", "\"other\" text");

    assert_eq!(
        problems(&schema, &[Widget::meta().into_table()]).await,
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
        problems(&schema, &[Widget::meta().into_table()]).await,
        ["table \"gadget\" has no model"]
    );
}

#[tokio::test]
async fn reports_an_invalid_pgtype() {
    let schema = r#"create table "broken" ("id" uuid not null, "name" text not null)"#;

    let problems = problems(schema, &[Broken::meta().into_table()]).await;

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("broken: `pgtype` is not valid in a column definition: "),
        "{problems:?}"
    );
}

#[tokio::test]
async fn reports_an_invalid_pgdefault() {
    let schema = r#"create table "broken_default" ("id" uuid not null)"#;

    let problems = problems(schema, &[BrokenDefault::meta().into_table()]).await;

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0]
            .starts_with("broken_default: `pgdefault` is not valid in a column definition: "),
        "{problems:?}"
    );
}

const FRAME: &str = r#"
    create table "frame" (
        "id" uuid not null,
        "width" int4 not null,
        "height" int4 not null,
        "label" text not null
    )
"#;

#[tokio::test]
async fn flattened_columns_match_the_schema() {
    let problems: Vec<String> = problems(FRAME, &[Frame::meta().into_table()]).await;

    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
async fn reports_a_mismatch_in_a_flattened_column() {
    let schema = FRAME.replace("\"width\" int4", "\"width\" int8");

    assert_eq!(
        problems(&schema, &[Frame::meta().into_table()]).await,
        ["frame: column \"width\" is bigint, the model says int4"]
    );
}

#[tokio::test]
async fn reports_a_column_declared_twice() {
    let schema = r#"create table "duplicated" ("id" uuid not null, "note" text not null)"#;

    assert_eq!(
        problems(schema, &[Duplicated::meta().into_table()]).await,
        ["duplicated: column \"note\" is declared more than once"]
    );
}

const GENERATED: &str = r#"
    create table "generated" (
        "id" uuid not null,
        "net" int8 not null,
        "tax" int8 not null,
        "total" int8 generated always as ("net" + "tax") virtual not null
    )
"#;

#[tokio::test]
async fn generated_columns_match_the_schema() {
    let problems: Vec<String> = problems(GENERATED, &[Generated::meta().into_table()]).await;

    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
async fn reports_a_generated_expression_mismatch() {
    let schema = GENERATED.replace("\"net\" + \"tax\"", "\"net\" + \"tax\" + 1");

    let problems = problems(&schema, &[Generated::meta().into_table()]).await;

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("\"total\" generated expression is"),
        "{problems:?}"
    );
}

#[tokio::test]
async fn reports_a_stored_generated_column_as_virtual_mismatch() {
    let schema = GENERATED.replace(") virtual not null", ") stored not null");

    assert_eq!(
        problems(&schema, &[Generated::meta().into_table()]).await,
        ["generated: column \"total\" generated is stored, the model says virtual"]
    );
}

#[tokio::test]
async fn reports_an_ordinary_column_as_generated() {
    let schema = GENERATED.replace(
        "\"total\" int8 generated always as (\"net\" + \"tax\") virtual",
        "\"total\" int8 not null",
    );

    assert_eq!(
        problems(&schema, &[Generated::meta().into_table()]).await,
        ["generated: column \"total\" generated is none, the model says virtual"]
    );
}

const CONSTRAINTS: &str = r#"
    create table "parent" (
        "id" uuid not null,
        "tenant_id" uuid not null,
        "name" text not null,
        constraint "parent_pkey" primary key ("id"),
        constraint "parent_tenant_id_id_key" unique ("tenant_id", "id"),
        constraint "parent_name_key" unique ("name")
    );

    create table "child" (
        "id" uuid not null,
        "tenant_id" uuid not null,
        "parent_id" uuid not null,
        constraint "child_pkey" primary key ("id"),
        constraint "child_parent_fkey" foreign key ("tenant_id", "parent_id")
            references "parent" ("tenant_id", "id")
    );

    create function "child_trigger"() returns trigger language plpgsql as $$
    begin
        return new;
    end;
    $$;

    create trigger "child_trigger"
    before insert on "child"
    for each row execute function "child_trigger"();
"#;

fn constraint_models() -> [dbkit::TableMeta; 2] {
    [Parent::meta().into_table(), Child::meta().into_table()]
}

#[tokio::test]
async fn constraints_match_the_schema() {
    let problems: Vec<String> = problems(CONSTRAINTS, &constraint_models()).await;

    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
async fn reports_a_missing_foreign_key() {
    let schema =
        CONSTRAINTS.replace(
            ",\n        constraint \"child_parent_fkey\" foreign key (\"tenant_id\", \
             \"parent_id\")\n            references \"parent\" (\"tenant_id\", \"id\")",
            "",
        );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["child: foreign key (tenant_id, parent_id) is declared but no constraint exists"]
    );
}

#[tokio::test]
async fn reports_a_missing_trigger() {
    let schema = CONSTRAINTS.replace(
        "\n    create trigger \"child_trigger\"\n    before insert on \"child\"\n    for each row \
         execute function \"child_trigger\"();",
        "",
    );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["child: trigger `child_trigger` is declared but does not exist"]
    );
}

#[tokio::test]
async fn reports_a_missing_unique_key() {
    let schema = CONSTRAINTS.replace(
        ",\n        constraint \"parent_name_key\" unique (\"name\")",
        "",
    );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["parent: unique key (name) is declared but no index exists"]
    );
}

#[tokio::test]
async fn reports_an_extra_foreign_key() {
    let schema = format!(
        "{CONSTRAINTS}\nalter table \"child\" add constraint \"child_extra_fkey\"\nforeign key \
         (\"id\") references \"parent\" (\"id\");"
    );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["child: foreign key constraint `child_extra_fkey` on (id) has no model declaration"]
    );
}

#[tokio::test]
async fn reports_an_extra_trigger() {
    let schema = format!(
        "{CONSTRAINTS}\ncreate trigger \"child_extra_trigger\"\nbefore insert on \"child\"\nfor \
         each row execute function \"child_trigger\"();"
    );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["child: trigger `child_extra_trigger` has no model declaration"]
    );
}

#[tokio::test]
async fn reports_an_extra_unique_key() {
    let schema = format!(
        "{CONSTRAINTS}\ncreate unique index \"parent_tenant_id_key\" on \"parent\" \
         (\"tenant_id\");"
    );

    assert_eq!(
        problems(&schema, &constraint_models()).await,
        ["parent: unique index `parent_tenant_id_key` on (tenant_id) has no model declaration"]
    );
}
