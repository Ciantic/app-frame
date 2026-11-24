use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbSelect;
use dbkit::DbType;
use dbkit::Selected;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, DbType)]
#[sqlx(transparent)]
#[db(pgtype = "uuid")]
struct OrderId(pub Uuid);

#[derive(DbModel, DbSelect)]
#[expect(dead_code, reason = "the fields exist only to derive the metadata")]
#[db(table = "orders")]
struct Order {
    #[db(primary_key)]
    id: OrderId,

    #[db(pgtype = "varchar(20)")]
    status: String,

    #[db(pgtype = "int8")]
    total: i64,

    #[db(pgtype = "bool")]
    vip: bool,

    #[db(pgtype = "text[]")]
    tags: Vec<String>,

    #[db(pgtype = "timestamptz")]
    deleted_at: Option<DateTime<Utc>>,

    #[db(created_at, pgtype = "timestamptz", pgdefault = "now()")]
    created_at: DateTime<Utc>,
}

#[test]
fn every_column_becomes_a_selected_field() {
    let row = OrderSelect {
        id: Selected::Set(OrderId(Uuid::nil())),
        status: Selected::Absent,
        total: Selected::Set(7),
        vip: Selected::Absent,
        tags: Selected::Absent,
        deleted_at: Selected::Set(None),
        created_at: Selected::Absent,
    };

    assert_eq!(
        serde_json::to_string(&row).unwrap(),
        r#"{"id":"00000000-0000-0000-0000-000000000000","total":7,"deleted_at":null}"#
    );
}

#[test]
fn a_selected_null_is_not_an_absent_column() {
    let cleared = OrderSelect {
        id: Selected::Absent,
        status: Selected::Absent,
        total: Selected::Absent,
        vip: Selected::Absent,
        tags: Selected::Absent,
        deleted_at: Selected::Set(None),
        created_at: Selected::Absent,
    };
    let unselected = OrderSelect {
        deleted_at: Selected::Absent,
        ..OrderSelect {
            id: Selected::Absent,
            status: Selected::Absent,
            total: Selected::Absent,
            vip: Selected::Absent,
            tags: Selected::Absent,
            deleted_at: Selected::Absent,
            created_at: Selected::Absent,
        }
    };

    assert_eq!(
        serde_json::to_string(&cleared).unwrap(),
        r#"{"deleted_at":null}"#
    );
    assert_eq!(serde_json::to_string(&unselected).unwrap(), "{}");
}

#[derive(DbModel, DbSelect)]
#[expect(dead_code, reason = "the fields exist only to derive the metadata")]
#[db(fragment)]
struct Size {
    #[db(pgtype = "int4")]
    width: i32,

    #[db(pgtype = "int4")]
    height: i32,
}

#[derive(DbModel, DbSelect)]
#[expect(dead_code, reason = "the fields exist only to derive the metadata")]
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
fn a_row_round_trips_and_missing_keys_are_absent() {
    let json = r#"{"id":"00000000-0000-0000-0000-000000000000","total":7,"deleted_at":null}"#;
    let row: OrderSelect = serde_json::from_str(json).unwrap();

    assert_eq!(row.total, Selected::Set(7));
    assert_eq!(row.deleted_at, Selected::Set(None));
    assert!(row.status.is_absent());
    assert_eq!(serde_json::to_string(&row).unwrap(), json);
}

#[test]
fn an_empty_projection_reads_as_an_empty_row() {
    let row: OrderSelect = serde_json::from_str("{}").unwrap();

    assert!(::dbkit::SelectRow::is_empty(&row));
    assert!(row.id.is_absent());
}

#[test]
fn a_null_for_a_non_nullable_column_is_rejected() {
    // The column cannot hold `NULL`, so neither can the decoded row.
    assert!(serde_json::from_str::<OrderSelect>(r#"{"total":null}"#).is_err());
}

#[test]
fn an_unselected_fragment_writes_nothing() {
    let row: FrameSelect = serde_json::from_str(r#"{"label":"frame"}"#).unwrap();

    assert!(::dbkit::SelectRow::is_empty(&row.size));
    assert_eq!(serde_json::to_string(&row).unwrap(), r#"{"label":"frame"}"#);
}

#[test]
fn a_flattened_field_nests_its_select_type() {
    let row = FrameSelect {
        id: Selected::Absent,
        size: SizeSelect {
            width: Selected::Set(3),
            height: Selected::Absent,
        },
        label: Selected::Set("frame".to_owned()),
    };

    assert_eq!(
        serde_json::to_string(&row).unwrap(),
        r#"{"size":{"width":3},"label":"frame"}"#
    );
}
