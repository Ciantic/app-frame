use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::Filter;
use dbkit::Query;
use pgtest_server::PgtestServer;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "orders_live")]
struct OrderLive {
    #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
    #[db(query_ops(eq))]
    id: Uuid,

    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq, in, not_in, ilike))]
    status: String,

    #[db(pgtype = "int8")]
    #[db(query_ops(eq, gte, lt, between))]
    total: i64,

    #[db(pgtype = "bool")]
    #[db(query_ops(eq))]
    vip: bool,

    #[db(pgtype = "text[]")]
    #[db(query_ops(contains, overlaps, contains_all, not_contains))]
    tags: Vec<String>,

    #[db(pgtype = "timestamptz")]
    #[db(query_ops(eq, neq))]
    deleted_at: Option<DateTime<Utc>>,
}

const SCHEMA: &str = r#"
    create table "orders_live" (
        "id" uuid not null default uuidv7(),
        "status" varchar(20) not null,
        "total" int8 not null,
        "vip" bool not null default false,
        "tags" text[] not null default '{}',
        "deleted_at" timestamptz
    )
"#;

async fn database() -> PgPool {
    let db = PgtestServer::start().await.database().await;

    sqlx::raw_sql(AssertSqlSafe(SCHEMA.to_owned()))
        .execute(&db)
        .await
        .unwrap();

    db
}

/// The rows the fixtures insert, in the order a test expects them back.
async fn seeded() -> PgPool {
    let db = database().await;

    let statements = [
        r#"insert into "orders_live" ("status", "total", "vip", "tags", "deleted_at") values
            ('open', 100, true, '{rust,sql}', null),
            ('open', 250, false, '{rust}', null),
            ('paid', 500, true, '{go}', now()),
            ('void', 500, false, '{}', null)"#,
    ];

    for statement in statements {
        sqlx::raw_sql(AssertSqlSafe(statement.to_owned()))
            .execute(&db)
            .await
            .unwrap();
    }

    db
}

/// Runs the query and returns the `total` of every row, in table order.
async fn totals(db: &PgPool, query: &Query<OrderLiveColumn>) -> Vec<i64> {
    let built = query.build().unwrap();
    let rows = sqlx::query_with(AssertSqlSafe(built.sql), built.arguments)
        .fetch_all(db)
        .await
        .unwrap();

    rows.iter().map(|row| row.get("total")).collect()
}

fn c() -> OrderLiveColumns {
    OrderLive::columns()
}

#[tokio::test]
async fn a_filter_returns_the_expected_rows() {
    let db = seeded().await;

    let query =
        OrderLive::query().filters([c().status.in_(["open", "paid"]), c().deleted_at.is_null()]);

    assert_eq!(totals(&db, &query).await, [100, 250]);
}

#[tokio::test]
async fn a_disjunction_is_not_flattened() {
    let db = seeded().await;

    let query = OrderLive::query().filters([
        c().vip.eq(true),
        Filter::or([c().total.lt(200), c().status.eq("void")]),
    ]);

    // `vip` is true for 100 and 500, but only the void row is `status = void`
    // and it is not a vip, so `total < 200` is the only way in.
    assert_eq!(totals(&db, &query).await, [100]);
}

#[tokio::test]
async fn a_list_length_does_not_change_the_statement() {
    let db = seeded().await;

    let two = OrderLive::query().filters([c().status.in_(["open", "paid"])]);
    let fifty: Vec<String> = (0..50).map(|index| format!("status{index}")).collect();
    let many = OrderLive::query().filters([c().status.in_(fifty)]);

    assert_eq!(
        two.build().unwrap().sql,
        many.build().unwrap().sql,
        "one statement shape, whatever the list holds"
    );
    // Both open rows and the paid one.
    assert_eq!(totals(&db, &two).await, [100, 250, 500]);
    assert!(totals(&db, &many).await.is_empty());
}

#[tokio::test]
async fn paging_binds_its_parameters() {
    let db = seeded().await;

    let first = OrderLive::query()
        .filters([c().status.in_(["open", "paid"])])
        .limit(2);
    let second = OrderLive::query()
        .filters([c().status.in_(["open", "paid"])])
        .limit(2)
        .offset(2);

    assert_eq!(totals(&db, &first).await, [100, 250]);
    // The `in` list holds two statuses, so the third matching row is the void
    // one, which the list excludes.
    assert_eq!(totals(&db, &second).await, [500]);

    let none = OrderLive::query().limit(0);

    assert!(totals(&db, &none).await.is_empty());
}

#[tokio::test]
async fn the_array_operators_compare_elements() {
    let db = seeded().await;

    assert_eq!(
        totals(
            &db,
            &OrderLive::query().filters([c().tags.contains("rust")])
        )
        .await,
        [100, 250]
    );
    assert_eq!(
        totals(
            &db,
            &OrderLive::query().filters([c().tags.overlaps(["sql", "go"])])
        )
        .await,
        [100, 500]
    );
    assert_eq!(
        totals(
            &db,
            &OrderLive::query().filters([c().tags.contains_all(["rust", "sql"])])
        )
        .await,
        [100]
    );
    assert_eq!(
        totals(
            &db,
            &OrderLive::query().filters([c().tags.not_contains("rust")])
        )
        .await,
        [500, 500]
    );
}

#[tokio::test]
async fn every_scalar_type_binds() {
    let db = seeded().await;

    let id = Uuid::now_v7();
    sqlx::raw_sql(AssertSqlSafe(format!(
        r#"insert into "orders_live" ("id", "status", "total", "tags") values
            ('{id}', 'refunded', 700, '{{x}}')"#
    )))
    .execute(&db)
    .await
    .unwrap();

    assert_eq!(
        totals(&db, &OrderLive::query().filters([c().id.eq(id)])).await,
        [700]
    );
    assert_eq!(
        totals(&db, &OrderLive::query().filters([c().status.ilike("REF%")])).await,
        [700]
    );
    assert_eq!(
        totals(
            &db,
            &OrderLive::query().filters([c().deleted_at.neq(
                DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )])
        )
        .await,
        [500]
    );
    assert_eq!(
        totals(&db, &OrderLive::query().filters([c().vip.eq(true)])).await,
        [100, 500]
    );
}

#[tokio::test]
async fn a_sparse_selection_returns_its_columns() {
    let db = seeded().await;

    let query = OrderLive::query()
        .select([OrderLiveColumn::Status, OrderLiveColumn::Total])
        .filters([c().total.eq(100)]);
    let built = query.build().unwrap();

    assert_eq!(
        built.sql,
        "SELECT \"status\", \"total\" FROM \"orders_live\" WHERE \"total\" = $1"
    );

    let row = sqlx::query_with(AssertSqlSafe(built.sql), built.arguments)
        .fetch_one(&db)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "open");
    assert_eq!(row.get::<i64, _>("total"), 100);
}

#[tokio::test]
async fn the_handles_build_a_projection_that_runs() {
    let db = seeded().await;

    let query = OrderLive::query()
        .select(c().status)
        .filters([c().total.eq(100)]);
    let built = query.build().unwrap();

    assert_eq!(
        built.sql,
        "SELECT \"status\" FROM \"orders_live\" WHERE \"total\" = $1"
    );

    let row = sqlx::query_with(AssertSqlSafe(built.sql), built.arguments)
        .fetch_one(&db)
        .await
        .unwrap();

    assert_eq!(row.get::<String, _>("status"), "open");
}

#[tokio::test]
async fn a_projected_row_reports_the_columns_it_lacks() {
    use sqlx::Column as _;
    use sqlx::Row as _;

    let db = seeded().await;

    let query = OrderLive::query().select([c().status.column(), c().total.column()]);
    let built = query.build().unwrap();
    let row = sqlx::query_with(AssertSqlSafe(built.sql), built.arguments)
        .fetch_one(&db)
        .await
        .unwrap();

    let names: Vec<&str> = row.columns().iter().map(|column| column.name()).collect();

    assert_eq!(names, ["status", "total"]);
    assert!(row.try_get::<Uuid, _>("id").is_err());
}

#[tokio::test]
async fn the_json_face_and_the_rust_face_agree() {
    let db = seeded().await;

    let payload =
        r#"{"filters":{"status__in":["open","paid"],"or":[{"total__lt":200},{"vip":true}]}}"#;
    let parsed = Query::<OrderLiveColumn>::parse(payload).unwrap();
    let rust = OrderLive::query().filters([
        c().status.in_(["open", "paid"]),
        Filter::or([c().total.lt(200), c().vip.eq(true)]),
    ]);

    assert_eq!(parsed, rust);
    assert_eq!(totals(&db, &parsed).await, [100, 500]);
    assert_eq!(totals(&db, &rust).await, [100, 500]);
}
