use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbType;
use dbkit::FilterOperator;
use dbkit::Model;
use dbkit::TableMeta;
use std::sync::LazyLock;
use uuid::Uuid;

#[derive(DbModel)]
#[db(table = "widget")]
struct Widget {
    #[db(primary_key, pgtype = "uuid", pgdefault = "uuidv7()")]
    id: Uuid,
    #[db(pgtype = "text")]
    #[db(query_ops(eq, neq, like, any))]
    name: String,
    #[db(created_at, pgtype = "timestamptz", pgdefault = "now()")]
    created_at: DateTime<Utc>,
    #[db(updated_at, pgtype = "timestamptz", pgdefault = "now()")]
    updated_at: DateTime<Utc>,
    #[db(version, pgtype = "int8", pgdefault = "0")]
    version: i64,
}

#[derive(DbModel)]
#[db(table = "membership")]
struct Membership {
    #[db(primary_key, pgtype = "uuid")]
    user_id: Uuid,
    #[db(primary_key, pgtype = "uuid")]
    team_id: Uuid,
}

/// Proves a model without a version column derives and stays usable.
#[derive(DbModel)]
#[db(table = "widget_log")]
struct WidgetLog {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(pgtype = "text")]
    message: String,
    #[db(pgtype = "text")]
    note: Option<String>,
}

/// A read-only row source that is a query rather than a table.
#[derive(DbModel)]
#[db(subquery = "SELECT status, total FROM orders WHERE vip")]
#[expect(dead_code, reason = "the fields exist only to derive the row metadata")]
struct VipOrders {
    #[db(pgtype = "varchar(20)")]
    #[db(query_ops(eq))]
    status: String,
    #[db(pgtype = "int8")]
    total: i64,
}

/// A column group embedded in another model rather than a table of its own.
#[derive(DbModel)]
#[db(fragment)]
struct Size {
    #[db(pgtype = "int4")]
    width: i32,
    #[db(pgtype = "int4")]
    height: i32,
}

/// A subquery splices a fragment's columns the same way a table does.
#[derive(DbModel)]
#[db(subquery = "SELECT width, height FROM sizes")]
#[expect(dead_code, reason = "the field exists only to derive the row metadata")]
struct SizeRow {
    #[db(flatten)]
    size: Size,
}

/// A value type that carries its column type and default.
#[derive(DbType)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[expect(dead_code, reason = "the type exists only to derive column metadata")]
struct GadgetId(Uuid);

#[derive(DbModel)]
#[db(table = "gadget")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct Gadget {
    #[db(primary_key)]
    id: GadgetId,
    /// The type's default is not this column's default.
    #[db(pgdefault)]
    parent_id: GadgetId,
    #[db(pgtype = "text")]
    label: String,
    deleted_id: Option<GadgetId>,
}

#[derive(DbModel)]
#[db(table = "frame")]
struct Frame {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(flatten)]
    size: Size,
    #[db(pgtype = "text")]
    label: String,
}

/// Flattening a table is a mistake the derive cannot see; `meta()` rejects it.
#[derive(DbModel)]
#[db(table = "wrong_flatten")]
#[expect(
    dead_code,
    reason = "the fields exist only to derive the table metadata"
)]
struct WrongFlatten {
    #[db(primary_key, pgtype = "uuid")]
    id: Uuid,
    #[db(flatten)]
    widget: Widget,
}

static MODELS: LazyLock<Vec<TableMeta>> = LazyLock::new(|| {
    vec![
        Widget::meta().into_table(),
        WidgetLog::meta().into_table(),
        Frame::meta().into_table(),
    ]
});

#[test]
fn derive_generates_table_metadata() {
    let meta = Widget::meta().into_table();

    assert_eq!(meta.table, "widget");
    assert_eq!(meta.primary_key, ["id"]);
    assert_eq!(meta.version, Some("version"));
    assert_eq!(meta.created_at, Some("created_at"));
    assert_eq!(meta.updated_at, Some("updated_at"));

    let columns: Vec<(&str, bool, &str, Option<&str>)> = meta
        .columns
        .iter()
        .map(|column| {
            (
                column.name(),
                column.nullable(),
                column.pg_type(),
                column.pg_default(),
            )
        })
        .collect();

    assert_eq!(
        columns,
        [
            ("id", false, "uuid", Some("uuidv7()")),
            ("name", false, "text", None),
            ("created_at", false, "timestamptz", Some("now()")),
            ("updated_at", false, "timestamptz", Some("now()")),
            ("version", false, "int8", Some("0")),
        ]
    );

    assert_eq!(
        meta.columns[1].query_ops(),
        [
            FilterOperator::Eq,
            FilterOperator::Neq,
            FilterOperator::Like,
            FilterOperator::Any,
        ]
    );
    assert!(meta.columns[0].query_ops().is_empty());
}

#[test]
fn derive_inherits_pgtype_and_pgdefault_from_the_field_type() {
    let meta = Gadget::meta().into_table();

    let columns: Vec<(&str, bool, &str, Option<&str>)> = meta
        .columns
        .iter()
        .map(|column| {
            (
                column.name(),
                column.nullable(),
                column.pg_type(),
                column.pg_default(),
            )
        })
        .collect();

    assert_eq!(
        columns,
        [
            ("id", false, "uuid", Some("uuidv7()")),
            ("parent_id", false, "uuid", None),
            ("label", false, "text", None),
            ("deleted_id", true, "uuid", Some("uuidv7()")),
        ]
    );
}

#[test]
fn derive_splices_flattened_columns_in_place() {
    let meta = Frame::meta();

    let columns: Vec<&str> = meta.columns().iter().map(|column| column.name()).collect();
    assert_eq!(columns, ["id", "width", "height", "label"]);
    assert_eq!(&meta.columns()[1..3], Size::meta().columns());
}

/// A fragment is a `Model` too, but describes no table.
#[test]
fn fragment_metadata_has_no_table() {
    let meta = Size::meta();

    assert_eq!(meta.table(), None);
    assert_eq!(
        meta.columns()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        ["width", "height"]
    );
}

/// A subquery is a `Model` too, but describes no table and carries its SQL.
#[test]
fn subquery_metadata_has_sql_and_no_table() {
    let meta = VipOrders::meta();
    let subquery = meta.subquery().expect("a subquery");

    assert_eq!(meta.table(), None);
    assert_eq!(subquery.sql, "SELECT status, total FROM orders WHERE vip");
    assert_eq!(
        meta.columns()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        ["status", "total"]
    );
}

#[test]
fn a_subquery_splices_flattened_columns_in_place() {
    let meta = SizeRow::meta();

    assert_eq!(
        meta.columns()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        ["width", "height"]
    );
    assert_eq!(meta.columns(), Size::meta().columns());
}

#[test]
fn metadata_is_built_once() {
    let first = Widget::meta().table().expect("a table");
    let second = Widget::meta().table().expect("a table");

    assert!(std::ptr::eq(first, second));
}

#[test]
fn derive_omits_absent_roles_and_marks_optionals() {
    let meta = <WidgetLog as Model>::meta().into_table();

    assert_eq!(meta.table, "widget_log");
    assert_eq!(meta.version, None);
    assert_eq!(meta.created_at, None);
    assert_eq!(meta.updated_at, None);
    assert!(
        meta.columns
            .iter()
            .any(|column| column.name() == "note" && column.nullable())
    );
    assert!(
        meta.columns
            .iter()
            .all(|column| column.query_ops().is_empty())
    );
}

#[test]
fn models_lists_every_model() {
    let tables: Vec<&str> = MODELS.iter().map(|meta| meta.table).collect();

    assert_eq!(tables, ["widget", "widget_log", "frame"]);
}

/// Proves the derive runs on a model shape and leaves the struct usable.
#[test]
fn derive_accepts_the_annotations() {
    let now = Utc::now();
    let widget = Widget {
        id: Uuid::from_u128(1),
        name: "Thingie".to_owned(),
        created_at: now,
        updated_at: now,
        version: 0,
    };

    assert_eq!(widget.id, Uuid::from_u128(1));
    assert_eq!(widget.name, "Thingie");
    assert_eq!(widget.created_at, widget.updated_at);
    assert_eq!(widget.version, 0);
}

/// Proves a composite primary key needs no extra ceremony beyond repeating the
/// role.
#[test]
fn derive_accepts_a_composite_primary_key() {
    let membership = Membership {
        user_id: Uuid::from_u128(1),
        team_id: Uuid::from_u128(2),
    };

    assert_eq!(membership.user_id, Uuid::from_u128(1));
    assert_eq!(membership.team_id, Uuid::from_u128(2));
}

/// Proves a model with no version column derives.
#[test]
fn derive_accepts_a_model_without_version() {
    let log = WidgetLog {
        id: Uuid::from_u128(1),
        message: "created".to_owned(),
        note: None,
    };

    assert_eq!(log.id, Uuid::from_u128(1));
    assert_eq!(log.message, "created");
    assert!(log.note.is_none());
}

/// Proves a flattened field stays an ordinary struct field on the row type.
#[test]
fn derive_accepts_a_flattened_field() {
    let frame = Frame {
        id: Uuid::from_u128(1),
        size: Size {
            width: 640,
            height: 480,
        },
        label: "photo".to_owned(),
    };

    assert_eq!(frame.id, Uuid::from_u128(1));
    assert_eq!(frame.size.width, 640);
    assert_eq!(frame.size.height, 480);
    assert_eq!(frame.label, "photo");
}

#[test]
#[should_panic(expected = "flattened fields must be `#[db(fragment)]` structs")]
fn flattening_a_table_panics() {
    let _ = WrongFlatten::meta();
}

#[test]
#[should_panic(expected = "is not a table and cannot be listed in `MODELS`")]
fn listing_a_fragment_in_models_panics() {
    let _ = Size::meta().into_table();
}

#[test]
#[should_panic(expected = "is not a table and cannot be listed in `MODELS`")]
fn listing_a_subquery_in_models_panics() {
    let _ = VipOrders::meta().into_table();
}
