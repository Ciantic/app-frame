use chrono::DateTime;
use chrono::Utc;
use dbkit::Bind;
use dbkit::Column;
use dbkit::Columns;
use dbkit::DbModel;
use dbkit::DbSelect;
use dbkit::DbType;
use dbkit::DbValue;
use dbkit::Filter;
use dbkit::FilterOperator;
use dbkit::Operand;
use dbkit::OrderDirection;
use dbkit::OrderTerm;
use dbkit::Query;
use dbkit::query::Error as QueryError;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, DbValue, DbType)]
#[sqlx(transparent)]
#[db(pgtype = "uuid")]
struct OrderId(pub Uuid);

#[derive(DbModel, DbSelect, sqlx::FromRow)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "orders")]
struct Order {
    #[db(primary_key)]
    #[db(query_ops(eq, any))]
    id: OrderId,

    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq, any, not_any, ilike))]
    #[db(orderable)]
    status: String,

    #[db(pgtype = "int8")]
    #[db(query_ops(eq, gte, lt, between))]
    #[db(orderable)]
    total: i64,

    #[db(pgtype = "bool")]
    #[db(query_ops(eq))]
    vip: bool,

    #[db(pgtype = "text[]")]
    #[db(query_ops(
        contains,
        overlaps,
        contains_all,
        not_contains,
        not_overlaps,
        not_contains_all
    ))]
    tags: Vec<String>,

    #[db(pgtype = "timestamptz")]
    #[db(query_ops(eq, neq))]
    #[db(orderable)]
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "odd")]
struct Odd {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "text")]
    #[db(query_ops(contains))]
    title: String,

    #[db(pgtype = "text[]")]
    #[db(query_ops(any))]
    tags: Vec<String>,

    #[db(pgtype = "numeric(10,2)")]
    #[db(query_ops(eq))]
    weight: f64,
}

/// A model whose row source is a query, written with a `WITH` clause.
#[derive(DbModel, DbSelect, sqlx::FromRow)]
#[expect(dead_code, reason = "the fields exist only to derive the row metadata")]
#[db(
    subquery = "WITH vip AS (SELECT status, total FROM orders WHERE vip) SELECT status, total \
                FROM vip"
)]
struct VipOrder {
    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq, any))]
    status: String,

    #[db(pgtype = "int8")]
    #[db(query_ops(gte))]
    total: i64,
}

fn vip() -> VipOrderColumns {
    VipOrder::columns()
}

fn c() -> OrderColumns {
    Order::columns()
}

fn built(query: &Query<OrderColumn>) -> (String, Vec<serde_json::Value>) {
    let built = query.render().expect("the query renders");
    let values = built.values_json();

    (built.sql, values)
}

/// Parses a `filters` object on its own, which is what most tests exercise.
fn parse<C: Column>(filters: &str) -> Result<Query<C>, QueryError> {
    Query::<C>::parse(&format!("{{\"filters\": {filters}}}"))
}

fn parse_error(filters: &str) -> String {
    parse::<OrderColumn>(filters)
        .expect_err("the request is rejected")
        .to_string()
}

fn parse_error_of<C: Column>(filters: &str) -> String {
    parse::<C>(filters)
        .expect_err("the request is rejected")
        .to_string()
}

fn build_error(query: &Query<OrderColumn>) -> String {
    query
        .render()
        .expect_err("the query is rejected")
        .to_string()
}

#[test]
fn a_subquery_model_renders_its_query_as_a_derived_table() {
    let query = VipOrder::query()
        .select(vip().total)
        .filters([vip().status.eq("open"), vip().total.gte(100)])
        .limit(5);
    let rendered = query.render().expect("the query renders");

    assert_eq!(
        rendered.sql,
        "SELECT \"total\" FROM (WITH vip AS (SELECT status, total FROM orders WHERE vip) SELECT \
         status, total FROM vip) AS \"subquery\" WHERE \"status\" = $1 AND \"total\" >= $2 LIMIT \
         $3"
    );
    assert_eq!(
        rendered.values_json(),
        vec![json!("open"), json!(100), json!(5)]
    );
}

#[test]
fn a_subquery_model_supports_both_query_faces() {
    let parsed =
        Query::<VipOrderColumn>::parse(r#"{"filters": {"status": "open"}, "limit": 3}"#).unwrap();
    let rust = VipOrder::query()
        .filters([vip().status.eq("open")])
        .limit(3);

    assert_eq!(parsed, rust);
    let _ = VipOrder::query().build().unwrap();
    let _ = VipOrder::query()
        .select(vip().status)
        .build_selected()
        .unwrap();
}

#[test]
fn the_goal_example_builds_the_documented_sql() {
    let query = Order::query()
        .filters([
            c().status.any(["open", "paid"]),
            c().deleted_at.is_null(),
            Filter::or([
                c().total.between(100, 500),
                Filter::and([c().vip.eq(true), c().tags.contains("gift")]),
            ]),
        ])
        .limit(20)
        .offset(40);

    let (sql, values) = built(&query);

    assert_eq!(
        sql,
        "SELECT * FROM \"orders\" WHERE \"status\" = ANY($1) AND \"deleted_at\" IS NULL AND \
         (\"total\" BETWEEN $2 AND $3 OR (\"vip\" = $4 AND $5 = ANY(\"tags\"))) LIMIT $6 OFFSET $7"
    );
    assert_eq!(
        values,
        [
            json!(["open", "paid"]),
            json!(100),
            json!(500),
            json!(true),
            json!("gift"),
            json!(20),
            json!(40),
        ]
    );
}

#[test]
fn the_json_payload_builds_the_same_query() {
    // A raw string, because `json!` builds a sorted map and key order is parameter order.
    let payload = r#"{
        "filters": {
            "status__any": ["open", "paid"],
            "deleted_at": null,
            "or": [
                {"total__between": [100, 500]},
                {"and": [{"vip": true}, {"tags__contains": "gift"}]}
            ]
        },
        "limit": 20,
        "offset": 40
    }"#;

    let parsed = Query::<OrderColumn>::parse(payload).expect("the payload parses");

    let rust = Order::query()
        .filters([
            c().status.any(["open", "paid"]),
            c().deleted_at.is_null(),
            Filter::or([
                c().total.between(100, 500),
                Filter::and([c().vip.eq(true), c().tags.contains("gift")]),
            ]),
        ])
        .limit(20)
        .offset(40);

    assert_eq!(parsed, rust);
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#"{"filters":{"status__any":["open","paid"],"deleted_at":null,"or":[{"total__between":[100,500]},{"and":[{"vip":true},{"tags__contains":"gift"}]}]},"limit":20,"offset":40}"#
    );
}

#[test]
fn a_key_without_a_suffix_is_eq() {
    let bare = parse::<OrderColumn>(r#"{"vip": true}"#).unwrap();
    let explicit = parse::<OrderColumn>(r#"{"vip__eq": true}"#).unwrap();

    assert_eq!(bare, explicit);
    assert_eq!(
        built(&bare).0,
        "SELECT * FROM \"orders\" WHERE \"vip\" = $1"
    );
}

#[test]
fn several_keys_are_a_conjunction() {
    let query = parse::<OrderColumn>(r#"{"status": "open", "vip": true}"#).unwrap();

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"status\" = $1 AND \"vip\" = $2".to_owned(),
            vec![json!("open"), json!(true)],
        )
    );
}

#[test]
fn key_order_is_parameter_order() {
    let query = parse::<OrderColumn>(r#"{"vip": true, "status": "open"}"#).unwrap();

    assert_eq!(built(&query).1, [json!(true), json!("open")]);
}

#[test]
fn null_is_the_short_form_of_is_null() {
    let short = parse::<OrderColumn>(r#"{"deleted_at": null}"#).unwrap();
    let long = parse::<OrderColumn>(r#"{"deleted_at__is_null": true}"#).unwrap();

    assert_eq!(short, long);
    assert_eq!(
        built(&short).0,
        "SELECT * FROM \"orders\" WHERE \"deleted_at\" IS NULL"
    );
    assert!(built(&short).1.is_empty());
}

#[test]
fn neq_with_null_is_not_null() {
    for payload in [
        r#"{"deleted_at__neq": null}"#,
        r#"{"deleted_at__is_not_null": true}"#,
    ] {
        let query = parse::<OrderColumn>(payload).unwrap();

        assert_eq!(
            built(&query).0,
            "SELECT * FROM \"orders\" WHERE \"deleted_at\" IS NOT NULL"
        );
        assert!(built(&query).1.is_empty());
    }
}

#[test]
fn a_list_is_one_array_parameter() {
    let query = Order::query().filters([c().status.any(["open", "paid"])]);

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"status\" = ANY($1)".to_owned(),
            vec![json!(["open", "paid"])],
        )
    );

    let fifty: Vec<String> = (0..50).map(|index| index.to_string()).collect();
    let query = Order::query().filters([c().status.any(fifty)]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE \"status\" = ANY($1)"
    );
}

#[test]
fn not_any_is_all() {
    let query = parse::<OrderColumn>(r#"{"status__not_any": ["open"]}"#).unwrap();

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE \"status\" <> ALL($1)"
    );
}

#[test]
fn between_binds_two_parameters() {
    let query = Order::query().filters([c().total.between(100, 500)]);

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"total\" BETWEEN $1 AND $2".to_owned(),
            vec![json!(100), json!(500)],
        )
    );
}

#[test]
fn an_open_range_is_two_leaves() {
    let query = Order::query().filters([c().total.gte(100), c().total.lt(500)]);

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"total\" >= $1 AND \"total\" < $2".to_owned(),
            vec![json!(100), json!(500)],
        )
    );
}

#[test]
fn a_pattern_operator_renders_its_keyword() {
    let query = parse::<OrderColumn>(r#"{"status__ilike": "op%"}"#).unwrap();

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"status\" ILIKE $1".to_owned(),
            vec![json!("op%")],
        )
    );
}

#[test]
fn the_array_operators_mirror_in() {
    assert_eq!(
        built(&Order::query().filters([c().tags.contains("rust")])),
        (
            "SELECT * FROM \"orders\" WHERE $1 = ANY(\"tags\")".to_owned(),
            vec![json!("rust")],
        )
    );
    assert_eq!(
        built(&Order::query().filters([c().tags.overlaps(["rust", "sql"])])),
        (
            "SELECT * FROM \"orders\" WHERE \"tags\" && $1".to_owned(),
            vec![json!(["rust", "sql"])],
        )
    );
    assert_eq!(
        built(&Order::query().filters([c().tags.contains_all(["rust", "sql"])])),
        (
            "SELECT * FROM \"orders\" WHERE \"tags\" @> $1".to_owned(),
            vec![json!(["rust", "sql"])],
        )
    );
    assert_eq!(
        built(&Order::query().filters([c().tags.not_contains("rust")])).0,
        "SELECT * FROM \"orders\" WHERE NOT ($1 = ANY(\"tags\"))"
    );
    assert_eq!(
        built(&Order::query().filters([c().tags.not_contains_all(["rust"])])).0,
        "SELECT * FROM \"orders\" WHERE NOT (\"tags\" @> $1)"
    );
    assert_eq!(
        built(&Order::query().filters([c().tags.not_overlaps(["rust"])])).0,
        "SELECT * FROM \"orders\" WHERE NOT (\"tags\" && $1)"
    );
}

#[test]
fn a_projection_lists_the_selected_columns_in_the_order_they_were_given() {
    let query = Order::query()
        .select([OrderColumn::Total, OrderColumn::Id])
        .filters([c().status.eq("open")]);

    assert_eq!(
        built(&query).0,
        "SELECT \"total\", \"id\" FROM \"orders\" WHERE \"status\" = $1"
    );
}

#[test]
fn every_selection_shape_builds_its_columns_in_its_own_order() {
    let projected = "SELECT \"id\", \"total\" FROM \"orders\"";

    assert_eq!(
        built(&Order::query().select(c().id)).0,
        "SELECT \"id\" FROM \"orders\""
    );
    assert_eq!(
        built(&Order::query().select(OrderColumn::Id)).0,
        "SELECT \"id\" FROM \"orders\""
    );
    assert_eq!(
        built(&Order::query().select([c().id.column(), c().total.column()])).0,
        projected
    );
    // A list of variants keeps its own order.
    assert_eq!(
        built(&Order::query().select([OrderColumn::Total, OrderColumn::Id])).0,
        "SELECT \"total\", \"id\" FROM \"orders\""
    );
    assert_eq!(
        built(&Order::query().select(Columns::all())).0,
        "SELECT \"id\", \"status\", \"total\", \"vip\", \"tags\", \"deleted_at\" FROM \"orders\""
    );
    assert_eq!(built(&Order::query()).0, "SELECT * FROM \"orders\"");
}

#[test]
fn a_handle_list_keeps_one_element_type() {
    // Two handles are two types, so the list holds the column each names.
    let handles = [c().id.column(), c().total.column()];

    assert_eq!(handles.len(), 2);
}

#[test]
fn a_named_list_can_be_selected_by_reference() {
    let projected = "SELECT \"id\", \"total\" FROM \"orders\"";
    let array = [OrderColumn::Id, OrderColumn::Total];
    let owned = vec![OrderColumn::Id, OrderColumn::Total];
    let set: Columns<OrderColumn> = array.into_iter().collect();

    // Each of these leaves its binding usable afterwards.
    assert_eq!(built(&Order::query().select(&array[..])).0, projected);
    assert_eq!(built(&Order::query().select(&owned)).0, projected);
    assert_eq!(built(&Order::query().select(&set)).0, projected);

    assert_eq!(array.len(), 2);
    assert_eq!(owned.len(), 2);
    assert_eq!(set.len(), 2);
}

#[test]
fn a_second_select_replaces_the_first() {
    let query = Order::query().select(c().id).select(c().total);

    assert_eq!(built(&query).0, "SELECT \"total\" FROM \"orders\"");
}

#[test]
fn an_empty_selection_is_an_error() {
    assert_eq!(
        build_error(&Order::query().select(Columns::default())),
        "a selection needs at least one column; omit `select` to read every column"
    );
    // Only `build()` knows the projection is rendered; the JSON face parses it and rejects it the same way.
    let query = Query::<OrderColumn>::parse(r#"{"select": []}"#).unwrap();

    assert_eq!(
        build_error(&query),
        "a selection needs at least one column; omit `select` to read every column"
    );
}

#[test]
fn the_select_key_parses_and_serializes() {
    let query = Query::<OrderColumn>::parse(r#"{"select": ["id", "total"]}"#).unwrap();

    assert_eq!(built(&query).0, "SELECT \"id\", \"total\" FROM \"orders\"");
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        r#"{"select":["id","total"]}"#
    );

    // Request order is kept, and it is part of equality: the two render different SQL.
    let reversed = Query::<OrderColumn>::parse(r#"{"select": ["total", "id"]}"#).unwrap();

    assert_ne!(reversed, query);
    assert_eq!(
        serde_json::to_string(&reversed).unwrap(),
        r#"{"select":["total","id"]}"#
    );
    assert_eq!(
        built(&reversed).0,
        "SELECT \"total\", \"id\" FROM \"orders\""
    );
}

#[test]
fn select_must_be_a_list_of_names() {
    let message = Query::<OrderColumn>::parse(r#"{"select": "id"}"#)
        .expect_err("the projection is not a list")
        .to_string();

    assert!(message.contains("invalid type: string"), "{message}");
}

#[test]
fn an_unknown_selected_column_names_the_columns() {
    let message = Query::<OrderColumn>::parse(r#"{"select": ["nope"]}"#)
        .expect_err("the column is unknown")
        .to_string();

    assert!(
        message.starts_with(
            "unknown column `nope`; expected one of: id, status, total, vip, tags, deleted_at"
        ),
        "{message}"
    );
}

#[test]
fn a_duplicate_selected_column_is_one_column() {
    let query = Query::<OrderColumn>::parse(r#"{"select": ["id", "id"]}"#).unwrap();

    assert_eq!(built(&query).0, "SELECT \"id\" FROM \"orders\"");
}

#[test]
fn select_serializes_before_filters_and_paging() {
    let query = Order::query()
        .select(c().total)
        .filters([c().status.eq("open")])
        .limit(20);

    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        r#"{"select":["total"],"filters":{"status":"open"},"limit":20}"#
    );
}

#[test]
fn ordering_renders_after_the_filters_and_before_paging() {
    let query = Order::query()
        .filters([c().status.eq("open")])
        .order_by([c().total.desc(), c().status.asc()])
        .limit(20)
        .offset(40);

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"status\" = $1 ORDER BY \"total\" DESC, \"status\" \
             ASC LIMIT $2 OFFSET $3"
                .to_owned(),
            vec![json!("open"), json!(20), json!(40)],
        ),
        "a direction is a keyword, so ordering adds no parameter"
    );
}

#[test]
fn ordering_appends_across_calls() {
    let query = Order::query()
        .order_by([c().status.asc()])
        .order_by([c().total.desc()]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" ORDER BY \"status\" ASC, \"total\" DESC"
    );
}

#[test]
fn an_order_term_is_the_column_and_its_direction() {
    let term = c().deleted_at.desc();

    assert_eq!(term.column, OrderColumn::DeletedAt);
    assert_eq!(term.direction, OrderDirection::Desc);
    assert_eq!(OrderDirection::Asc.as_str(), "asc");
    assert_eq!(
        OrderDirection::from_name("desc"),
        Some(OrderDirection::Desc)
    );
}

#[test]
fn ordering_round_trips_through_json() {
    let payload = r#"{"filters":{"status":"open"},"order_by":["total desc","status"],"limit":5}"#;
    let parsed = Query::<OrderColumn>::parse(payload).unwrap();

    assert_eq!(
        parsed,
        Order::query()
            .filters([c().status.eq("open")])
            .order_by([c().total.desc(), c().status.asc()])
            .limit(5)
    );
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#"{"filters":{"status":"open"},"order_by":["total desc","status asc"],"limit":5}"#
    );
}

#[test]
fn an_empty_order_list_is_no_ordering() {
    let query = Query::<OrderColumn>::parse(r#"{"order_by": []}"#).unwrap();

    assert_eq!(query, Query::new());
    assert_eq!(built(&query).0, "SELECT * FROM \"orders\"");
}

#[test]
fn an_unknown_ordered_column_names_the_columns() {
    let message = Query::<OrderColumn>::parse(r#"{"order_by": ["nope"]}"#)
        .expect_err("the column is unknown")
        .to_string();

    assert!(
        message.starts_with(
            "unknown column `nope`; expected one of: id, status, total, vip, tags, deleted_at"
        ),
        "{message}"
    );
}

#[test]
fn a_column_without_orderable_is_rejected() {
    let message = Query::<OrderColumn>::parse(r#"{"order_by": ["vip desc"]}"#)
        .expect_err("the column is not orderable")
        .to_string();

    assert_eq!(
        message,
        "column `vip` is not orderable; add `#[db(orderable)]` to it"
    );
    assert_eq!(
        build_error(&Order::query().order_by([OrderTerm::desc(OrderColumn::Tags)])),
        "column `tags` is not orderable; add `#[db(orderable)]` to it"
    );
}

#[test]
fn an_unknown_order_direction_lists_the_two() {
    let message = Query::<OrderColumn>::parse(r#"{"order_by": ["total down"]}"#)
        .expect_err("the direction is unknown")
        .to_string();

    assert_eq!(
        message,
        "order direction `down` is not known; expected one of: asc, desc"
    );
}

#[test]
fn a_malformed_order_entry_says_what_it_accepts() {
    for payload in [
        r#"{"order_by": [""]}"#,
        r#"{"order_by": ["total desc extra"]}"#,
    ] {
        let message = Query::<OrderColumn>::parse(payload)
            .expect_err("the entry is malformed")
            .to_string();

        assert!(
            message.contains("is not a column name with an optional `asc` or `desc`"),
            "{message}"
        );
    }
}

#[test]
fn the_order_cap_is_sixteen_terms() {
    let terms: Vec<OrderTerm<OrderColumn>> = (0..17)
        .map(|_| OrderTerm::desc(OrderColumn::Total))
        .collect();

    assert_eq!(
        build_error(&Order::query().order_by(terms)),
        "`order_by` takes at most 16 columns, got 17"
    );
    assert!(
        Order::query()
            .order_by((0..16).map(|_| OrderTerm::desc(OrderColumn::Total)))
            .render()
            .is_ok()
    );
}

#[test]
fn paging_binds_parameters_and_is_absent_when_unset() {
    let query = Order::query()
        .filters([c().status.eq("open")])
        .limit(20)
        .offset(40);

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" WHERE \"status\" = $1 LIMIT $2 OFFSET $3".to_owned(),
            vec![json!("open"), json!(20), json!(40)],
        )
    );

    let query = Order::query().filters([c().status.eq("open")]).limit(20);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE \"status\" = $1 LIMIT $2"
    );

    let query = Order::query().filters([c().status.eq("open")]).offset(0);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE \"status\" = $1"
    );
}

#[test]
fn a_disjunction_is_parenthesized_even_at_the_top_level() {
    let query = Order::query().filters([Filter::or([c().status.eq("open"), c().vip.eq(true)])]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE (\"status\" = $1 OR \"vip\" = $2)"
    );
}

#[test]
fn mixed_depth_keeps_its_leaves_flat() {
    let payload = r#"{"filters":{"status":"open","or":[{"vip":true},{"and":[{"total__gte":100},{"total__lt":500}]}],"tags__contains":"gift"}}"#;

    let query = Order::query().filters([
        c().status.eq("open"),
        Filter::or([
            c().vip.eq(true),
            Filter::and([c().total.gte(100), c().total.lt(500)]),
        ]),
        c().tags.contains("gift"),
    ]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE \"status\" = $1 AND (\"vip\" = $2 OR (\"total\" >= $3 AND \
         \"total\" < $4)) AND $5 = ANY(\"tags\")"
    );
    assert_eq!(serde_json::to_string(&query).unwrap(), payload);
    assert_eq!(Query::<OrderColumn>::parse(payload).unwrap(), query);
}

#[test]
fn two_or_children_fall_back_to_the_and_wrapper() {
    let query = Order::query().filters([
        Filter::or([c().status.eq("open"), c().vip.eq(true)]),
        Filter::or([c().total.lt(100), c().tags.contains("gift")]),
    ]);

    let payload = r#"{"filters":{"and":[{"or":[{"status":"open"},{"vip":true}]},{"or":[{"total__lt":100},{"tags__contains":"gift"}]}]}}"#;

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE (\"status\" = $1 OR \"vip\" = $2) AND (\"total\" < $3 OR \
         $4 = ANY(\"tags\"))"
    );
    assert_eq!(serde_json::to_string(&query).unwrap(), payload);
    assert_eq!(Query::<OrderColumn>::parse(payload).unwrap(), query);
}

#[test]
fn a_single_child_group_keeps_its_parentheses() {
    let query = Order::query().filters([Filter::and([c().status.eq("open")])]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE (\"status\" = $1)"
    );
    assert_eq!(
        serde_json::to_string(&query).unwrap(),
        r#"{"filters":{"and":[{"status":"open"}]}}"#
    );
}

#[test]
fn nesting_is_structural_until_normalized() {
    let query = Order::query().filters([Filter::and([
        c().status.eq("open"),
        Filter::and([c().vip.eq(true)]),
    ])]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE (\"status\" = $1 AND (\"vip\" = $2))"
    );
    assert_eq!(
        built(&query.clone().normalize()).0,
        "SELECT * FROM \"orders\" WHERE \"status\" = $1 AND \"vip\" = $2"
    );
}

#[test]
fn a_redundant_and_stays_nested() {
    let query = Order::query().filters([Filter::and([
        Filter::and([c().status.eq("open"), c().vip.eq(true)]),
        c().tags.contains("gift"),
    ])]);

    assert_eq!(
        built(&query).0,
        "SELECT * FROM \"orders\" WHERE ((\"status\" = $1 AND \"vip\" = $2) AND $3 = \
         ANY(\"tags\"))"
    );
}

#[test]
fn an_empty_group_is_rejected() {
    let query = Order::query().filters([Filter::and(Vec::new())]);

    assert_eq!(build_error(&query), "`and` needs at least one filter");
}

#[test]
fn a_filter_at_the_top_level_says_where_filters_go() {
    let message = Query::<OrderColumn>::parse(r#"{"vip": true}"#)
        .expect_err("the request is rejected")
        .to_string();

    assert_eq!(
        message,
        "unknown key `vip`; a query takes `select`, `filters`, `order_by`, `limit` and `offset`"
    );
}

#[test]
fn an_unknown_column_names_the_columns() {
    let message = parse_error(r#"{"statuss": "open"}"#);

    assert!(
        message.starts_with(
            "unknown column `statuss`; expected one of: id, status, total, vip, tags, deleted_at"
        ),
        "{message}"
    );
}

#[test]
fn an_unknown_operator_lists_the_vocabulary() {
    let message = parse_error(r#"{"status__betwen": [1, 2]}"#);

    assert!(
        message.starts_with("operator `betwen` is not known; expected one of: eq, neq, gt, gte"),
        "{message}"
    );
}

#[test]
fn an_operator_outside_query_ops_is_rejected() {
    let message = parse_error(r#"{"total__ilike": "5%"}"#);

    assert!(
        message.starts_with(
            "operator `ilike` is not allowed on column `total`; its query_ops are: eq, gte, lt, \
             between"
        ),
        "{message}"
    );
}

#[test]
fn an_empty_list_is_rejected() {
    assert!(
        parse_error(r#"{"status__any": []}"#).starts_with("`any` needs at least one value"),
        "{}",
        parse_error(r#"{"status__any": []}"#)
    );
}

#[test]
fn a_list_operator_with_a_scalar_says_what_it_wants() {
    let message = parse_error(r#"{"status__any": "open"}"#);

    assert!(
        message.starts_with("`any` takes a list, got a string"),
        "{message}"
    );
}

#[test]
fn a_scalar_operator_with_a_list_is_rejected() {
    let message = parse_error(r#"{"status": ["open"]}"#);

    assert!(
        message.starts_with("column `status` is varchar(20), the value is a list"),
        "{message}"
    );
}

#[test]
fn a_wrong_value_type_names_the_column() {
    let message = parse_error(r#"{"total": "500"}"#);

    assert!(
        message.starts_with("column `total` is int8, the value is a string"),
        "{message}"
    );
}

#[test]
fn a_fractional_integer_is_rejected() {
    let message = parse_error(r#"{"total": 1.5}"#);

    assert!(
        message.starts_with(
            "column `total` is int8, invalid type: floating point `1.5`, expected i64"
        ),
        "{message}"
    );
}

#[test]
fn eq_with_an_explicit_null_points_at_the_short_forms() {
    let message = parse_error(r#"{"deleted_at__eq": null}"#);

    assert!(
        message.starts_with("use `deleted_at`: null, or `deleted_at__neq`: null"),
        "{message}"
    );
}

#[test]
fn is_null_with_false_is_rejected() {
    let message = parse_error(r#"{"deleted_at__is_null": false}"#);

    assert!(
        message.starts_with("`is_null` takes `true`, not `false`"),
        "{message}"
    );
}

#[test]
fn a_duplicate_key_is_rejected() {
    let message = parse_error(r#"{"vip": true, "vip": false}"#);

    assert!(message.starts_with("duplicate key `vip`"), "{message}");
}

#[test]
fn any_on_an_array_column_suggests_overlaps() {
    let message = parse_error_of::<OddColumn>(r#"{"tags__any": ["rust"]}"#);

    assert!(
        message.starts_with("column `tags` is an array, so `any` does not apply"),
        "{message}"
    );
}

#[test]
fn contains_on_a_scalar_column_suggests_any() {
    let message = parse_error_of::<OddColumn>(r#"{"title__contains": "open"}"#);

    assert!(
        message.starts_with(
            "column `title` is not an array, so `contains` does not apply; use `any` instead"
        ),
        "{message}"
    );
}

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "reserved")]
struct Reserved {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(pgtype = "bool")]
    #[db(query_ops(eq))]
    r#or: bool,

    // Columns whose names are also wrapper keys: the nesting tells them apart.
    #[db(pgtype = "int8")]
    #[db(query_ops(eq))]
    limit: i64,

    #[db(pgtype = "text")]
    #[db(query_ops(eq))]
    filters: String,
}

fn built_reserved(query: &Query<ReservedColumn>) -> (String, Vec<serde_json::Value>) {
    let built = query.render().expect("the query renders");
    let values = built.values_json();

    (built.sql, values)
}

#[test]
fn a_column_named_or_wins_over_the_combinator() {
    let query = parse::<ReservedColumn>(r#"{"or": true}"#).unwrap();

    assert_eq!(
        query,
        Query::new().filters([Filter::leaf(
            ReservedColumn::Or,
            FilterOperator::Eq,
            Operand::one(true)
        )])
    );

    // A group therefore has no JSON form on this model, and says so.
    let message = serde_json::to_string(&Query::<ReservedColumn>::new().filters([Filter::or([
        Filter::leaf(ReservedColumn::Or, FilterOperator::Eq, Operand::one(true)),
    ])]))
    .expect_err("an `or` group cannot be written")
    .to_string();

    assert!(message.contains("has no JSON form"), "{message}");
}

#[test]
fn paging_lives_beside_the_filters_not_inside_them() {
    // `limit` is not a filter key at all, so inside `filters` it is a column name like any other.
    let message = parse_error(r#"{"limit": 5}"#);

    assert!(
        message.starts_with("unknown column `limit`; expected one of: id, status"),
        "{message}"
    );

    let query = parse::<ReservedColumn>(r#"{"limit": 5}"#).unwrap();

    assert_eq!(
        built_reserved(&query).0,
        "SELECT * FROM \"reserved\" WHERE \"limit\" = $1"
    );
    assert_eq!(
        built_reserved(&parse::<ReservedColumn>(r#"{"filters": "x"}"#).unwrap()).0,
        "SELECT * FROM \"reserved\" WHERE \"filters\" = $1"
    );
    assert_eq!(Reserved::query().limit(5).limit, Some(5));
    assert_eq!(
        serde_json::to_string(&Reserved::query().limit(5)).unwrap(),
        r#"{"limit":5}"#
    );
}

#[test]
fn filters_must_be_an_object() {
    let message = Query::<OrderColumn>::parse(r#"{"filters": 5}"#)
        .expect_err("the request is rejected")
        .to_string();

    assert!(message.contains("expected a filters object"), "{message}");
}

#[test]
fn empty_filters_match_every_row() {
    let query = Query::<OrderColumn>::parse(r#"{"filters": {}, "limit": 20}"#).unwrap();

    assert_eq!(
        built(&query),
        (
            "SELECT * FROM \"orders\" LIMIT $1".to_owned(),
            vec![json!(20)],
        )
    );
}

#[test]
fn paging_keys_must_be_non_negative_integers() {
    for payload in [
        r#"{"limit": -1}"#,
        r#"{"limit": 1.5}"#,
        r#"{"offset": "10"}"#,
    ] {
        assert!(
            Query::<OrderColumn>::parse(payload).is_err(),
            "{payload} parsed"
        );
    }
}

#[test]
fn the_limit_cap_is_a_build_error() {
    let query = Order::query().limit(1001);

    assert_eq!(build_error(&query), "`limit` must be at most 1000");
    assert!(Order::query().limit(1000).render().is_ok());
}

#[test]
fn build_takes_every_column_but_not_a_subset() {
    // A full selection in any order renders the same columns as `SELECT *`.
    let every_column: Columns<OrderColumn> = OrderColumn::all().iter().rev().copied().collect();
    let _ = Order::query().select(every_column).build().unwrap();

    let Err(error) = Order::query().select([c().total.column()]).build() else {
        panic!("the projection leaves columns out");
    };

    assert_eq!(
        error.to_string(),
        "`build` decodes every column; the query selects a subset, so call `build_selected` or \
         omit `select`"
    );
}

#[test]
fn build_reports_an_empty_selection_as_such() {
    // The render check comes first, so an empty selection is not reported as a subset of the columns.
    let Err(error) = Order::query().select(Columns::default()).build() else {
        panic!("the projection is empty");
    };

    assert_eq!(
        error.to_string(),
        "a selection needs at least one column; omit `select` to read every column"
    );
}

#[test]
fn build_selected_needs_an_explicit_selection() {
    let Err(error) = Order::query().build_selected() else {
        panic!("the query reads every column");
    };

    assert_eq!(
        error.to_string(),
        "`build_selected` decodes a projection; the query has no `select`, so call `build` or \
         name the columns"
    );

    let _ = Order::query().select(c().total).build_selected().unwrap();
}

#[test]
fn a_unit8_column_is_one_byte() {
    assert_eq!(std::mem::size_of::<OrderColumns>(), 6);
}

#[test]
fn the_depth_cap_is_eight_groups() {
    fn nested(levels: usize) -> Query<OrderColumn> {
        let mut filter = Order::query().filters([c().vip.eq(true)]).filters;
        for _ in 0..levels {
            filter = vec![Filter::and(filter)];
        }

        Order::query().filters(filter)
    }

    assert!(nested(8).render().is_ok());
    assert_eq!(
        nested(9).render().unwrap_err().to_string(),
        "filters nest deeper than 8 levels"
    );
}

#[test]
fn the_node_cap_counts_every_filter() {
    let filters: Vec<Filter<OrderColumn>> = (0..65).map(|_| c().vip.eq(true)).collect();

    assert_eq!(
        build_error(&Order::query().filters(filters)),
        "the query has more than 64 filter nodes"
    );
}

#[test]
fn the_list_cap_is_hundred() {
    let values: Vec<String> = (0..=100).map(|index| index.to_string()).collect();
    let query = Order::query().filters([c().status.any(values)]);

    assert_eq!(
        build_error(&query),
        "`any` accepts at most 100 values, got 101"
    );
}

#[test]
fn a_leaf_checks_the_operator_against_query_ops() {
    let query = Order::query().filters([Filter::leaf(
        OrderColumn::Total,
        FilterOperator::ILike,
        Operand::one("5%"),
    )]);

    assert_eq!(
        build_error(&query),
        "operator `ilike` is not allowed on column `total`; its query_ops are: eq, gte, lt, \
         between"
    );
}

#[test]
fn a_leaf_checks_the_operand_arity() {
    let query = Order::query().filters([Filter::leaf(
        OrderColumn::Total,
        FilterOperator::Between,
        Operand::one(1),
    )]);

    assert_eq!(
        build_error(&query),
        "`between` takes two values, got one value"
    );
}

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the column metadata"
)]
#[db(fragment)]
struct Size {
    #[db(pgtype = "int4")]
    #[db(query_ops(gte, lt))]
    width: i32,

    #[db(pgtype = "text")]
    height_label: String,
}

#[derive(DbModel)]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
#[db(table = "frame")]
struct Frame {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,

    #[db(flatten)]
    size: Size,

    #[db(pgtype = "text")]
    label: String,
}

#[test]
fn a_flattened_handle_builds_a_filter_of_the_parent_model() {
    let query = Frame::query().filters([Frame::columns().size.width.gte(10)]);

    assert_eq!(
        built_frame(&query),
        (
            "SELECT * FROM \"frame\" WHERE \"width\" >= $1".to_owned(),
            vec![json!(10)],
        )
    );
}

fn built_frame(query: &Query<FrameColumn>) -> (String, Vec<serde_json::Value>) {
    let built = query.render().expect("the query renders");
    let values = built.values_json();

    (built.sql, values)
}

#[test]
fn a_fragment_has_no_table() {
    let message = Query::<SizeColumn>::new()
        .render()
        .expect_err("a fragment has no table")
        .to_string();

    assert_eq!(
        message,
        "the model is a `#[db(fragment)]`, which has no table to query"
    );
}

#[test]
fn a_malformed_value_keeps_serdes_reason() {
    let message = parse_error(r#"{"id": "not-a-uuid"}"#);

    assert!(message.starts_with("column `id` is uuid, "), "{message}");
    assert!(
        !message.contains("line "),
        "the position is the object's: {message}"
    );
}

#[test]
fn a_value_of_the_wrong_json_kind_names_the_column() {
    let message = parse_error(r#"{"id": 42}"#);

    assert!(
        message.starts_with("column `id` is uuid, the value is a number"),
        "{message}"
    );
}

#[test]
fn an_unknown_column_type_is_unsupported_rather_than_guessed() {
    let message = parse_error_of::<OddColumn>(r#"{"weight": 1}"#);

    assert!(
        message.starts_with("column `weight` is numeric(10,2), which has no JSON filter value"),
        "{message}"
    );
}

#[test]
fn eq_dyn_is_type_exact() {
    let narrow = 100i32;
    let wide = 100i64;

    assert!(!narrow.eq_dyn(&wide));
    assert!(wide.eq_dyn(&100i64));
}

#[test]
fn a_newtype_takes_a_borrow() {
    let id = OrderId(Uuid::nil());
    let rust = Order::query().filters([c().id.eq(&id)]);
    let owned = Order::query().filters([c().id.eq(id.clone())]);

    assert_eq!(rust, owned);
    assert_eq!(built(&rust).1, [json!(id.0.to_string())]);
    for query in [
        Order::query().filters([c().id.any([id.clone()])]),
        Order::query().filters([c().id.any([id.clone()].iter())]),
        Order::query().filters([c().id.any(std::slice::from_ref(&id))]),
    ] {
        assert_eq!(
            query.render().unwrap().values_json(),
            [json!([id.0.to_string()])]
        );
    }
}
