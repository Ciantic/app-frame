use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbSelect;
use dbkit::Filter;
use dbkit::Query;
use dbkit::Selected;
use pgtest_server::PgtestServer;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(DbModel, DbSelect)]
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
    #[db(query_ops(eq, any, not_any, ilike))]
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
    query
        .build()
        .unwrap()
        .fetch_all(db)
        .await
        .unwrap()
        .iter()
        .map(|row| row.total.into_set().expect("every total was selected"))
        .collect()
}

fn c() -> OrderLiveColumns {
    OrderLive::columns()
}

#[tokio::test]
async fn a_filter_returns_the_expected_rows() {
    let db = seeded().await;

    let query =
        OrderLive::query().filters([c().status.any(["open", "paid"]), c().deleted_at.is_null()]);

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

    let two = OrderLive::query().filters([c().status.any(["open", "paid"])]);
    let fifty: Vec<String> = (0..50).map(|index| format!("status{index}")).collect();
    let many = OrderLive::query().filters([c().status.any(fifty)]);

    assert_eq!(
        two.render().unwrap().sql,
        many.render().unwrap().sql,
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
        .filters([c().status.any(["open", "paid"])])
        .limit(2);
    let second = OrderLive::query()
        .filters([c().status.any(["open", "paid"])])
        .limit(2)
        .offset(2);

    assert_eq!(totals(&db, &first).await, [100, 250]);
    // The `any` list holds two statuses, so the third matching row is the void
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

    assert_eq!(
        query.render().unwrap().sql,
        "SELECT \"status\", \"total\" FROM \"orders_live\" WHERE \"total\" = $1"
    );

    let row: OrderLiveSelect = query.build().unwrap().fetch_one(&db).await.unwrap();

    assert_eq!(row.status, Selected::Set("open".to_owned()));
    assert_eq!(row.total, Selected::Set(100));
}

#[tokio::test]
async fn the_handles_build_a_projection_that_runs() {
    let db = seeded().await;

    let query = OrderLive::query()
        .select(c().status)
        .filters([c().total.eq(100)]);

    assert_eq!(
        query.render().unwrap().sql,
        "SELECT \"status\" FROM \"orders_live\" WHERE \"total\" = $1"
    );

    let row: OrderLiveSelect = query.build().unwrap().fetch_one(&db).await.unwrap();

    assert_eq!(row.status, Selected::Set("open".to_owned()));
}

#[tokio::test]
async fn a_projected_row_reports_the_columns_it_lacks() {
    let db = seeded().await;

    let row: OrderLiveSelect = OrderLive::query()
        .select([c().status.column(), c().total.column()])
        .build()
        .unwrap()
        .fetch_one(&db)
        .await
        .unwrap();

    assert!(row.status.is_set());
    assert!(row.total.is_set());
    assert!(row.id.is_absent(), "the projection left it out");
    assert!(row.tags.is_absent());
}

#[tokio::test]
async fn the_json_face_and_the_rust_face_agree() {
    let db = seeded().await;

    let payload =
        r#"{"filters":{"status__any":["open","paid"],"or":[{"total__lt":200},{"vip":true}]}}"#;
    let parsed = Query::<OrderLiveColumn>::parse(payload).unwrap();
    let rust = OrderLive::query().filters([
        c().status.any(["open", "paid"]),
        Filter::or([c().total.lt(200), c().vip.eq(true)]),
    ]);

    assert_eq!(parsed, rust);
    assert_eq!(totals(&db, &parsed).await, [100, 500]);
    assert_eq!(totals(&db, &rust).await, [100, 500]);
}

#[tokio::test]
async fn build_returns_a_query_that_decodes_selected_rows() {
    let db = seeded().await;

    let rows: Vec<OrderLiveSelect> = OrderLive::query()
        .select([c().status.column(), c().total.column()])
        .filters([c().status.eq("open")])
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].total, Selected::Set(100));
    assert_eq!(rows[1].total, Selected::Set(250));
    assert!(rows[0].id.is_absent(), "the projection left it out");
    assert!(rows[0].tags.is_absent());
}

#[tokio::test]
async fn a_select_star_row_sets_every_field() {
    let db = seeded().await;

    let rows: Vec<OrderLiveSelect> = OrderLive::query()
        .limit(1)
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert!(rows[0].id.is_set());
    assert!(rows[0].deleted_at.is_set(), "selected, and `NULL`");
    assert_eq!(rows[0].deleted_at, Selected::Set(None));
}

#[tokio::test]
async fn the_query_can_be_held_before_it_runs() {
    let db = seeded().await;

    let query = OrderLive::query()
        .filters([c().total.eq(250)])
        .build()
        .unwrap();

    // The value owns its SQL and arguments, so it outlives the builder.
    let row: OrderLiveSelect = query.fetch_one(&db).await.unwrap();

    assert_eq!(row.total, Selected::Set(250));
}

#[tokio::test]
async fn fetch_one_errors_and_fetch_optional_reports_none() {
    let db = seeded().await;

    let missing = OrderLive::query()
        .filters([c().total.eq(-1)])
        .build()
        .unwrap()
        .fetch_one(&db)
        .await;

    assert!(
        matches!(missing, Err(sqlx::Error::RowNotFound)),
        "{missing:?}"
    );

    let optional: Option<OrderLiveSelect> = OrderLive::query()
        .filters([c().total.eq(-1)])
        .build()
        .unwrap()
        .fetch_optional(&db)
        .await
        .unwrap();

    assert!(optional.is_none());
}

#[tokio::test]
async fn the_query_runs_a_filter_and_paging() {
    let db = seeded().await;

    let rows: Vec<OrderLiveSelect> = OrderLive::query()
        .select(c().total)
        .filters([c().status.any(["open", "paid"])])
        .limit(2)
        .offset(1)
        .build()
        .unwrap()
        .fetch_all(&db)
        .await
        .unwrap();

    assert_eq!(
        rows.iter().map(|row| row.total).collect::<Vec<_>>(),
        [Selected::Set(250), Selected::Set(500)]
    );
}

#[tokio::test]
async fn build_surfaces_the_render_error_before_any_sql_runs() {
    let db = seeded().await;

    // `QueryAs` is not `Debug`, so the error is matched rather than unwrapped.
    let Err(error) = OrderLive::query().select(dbkit::Columns::default()).build() else {
        panic!("the projection is empty");
    };

    assert_eq!(
        error.to_string(),
        "a selection needs at least one column; omit `select` to read every column"
    );

    let _ = db;
}

#[tokio::test]
async fn the_query_takes_a_transaction_and_a_connection() {
    let db = seeded().await;

    let mut transaction = db.begin().await.unwrap();
    let in_transaction: Vec<OrderLiveSelect> = OrderLive::query()
        .select(c().total)
        .filters([c().total.eq(100)])
        .build()
        .unwrap()
        .fetch_all(&mut *transaction)
        .await
        .unwrap();
    transaction.rollback().await.unwrap();

    assert_eq!(in_transaction[0].total, Selected::Set(100));

    let mut connection = db.acquire().await.unwrap();
    let on_connection: Vec<OrderLiveSelect> = OrderLive::query()
        .select(c().total)
        .filters([c().total.eq(250)])
        .build()
        .unwrap()
        .fetch_all(&mut *connection)
        .await
        .unwrap();

    assert_eq!(on_connection[0].total, Selected::Set(250));
}
