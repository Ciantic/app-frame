use chrono::DateTime;
use chrono::Utc;
use dbkit::Column;
use dbkit::Columns;
use dbkit::DbModel;
use dbkit::DbType;
use dbkit::ErasedColumn;
use dbkit::FilterOperator;
use dbkit::Model;
use uuid::Uuid;

#[derive(DbType)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
pub struct WidgetId(pub Uuid);

#[derive(DbModel)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    #[db(query_ops(eq, neq, like))]
    pub name: String,

    #[db(pgtype = "text")]
    pub email_verified_at: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

#[derive(DbModel)]
#[db(fragment)]
pub struct Size {
    #[db(pgtype = "int4")]
    pub width: i32,

    #[db(pgtype = "int4")]
    pub height: i32,
}

#[derive(DbModel)]
#[db(table = "frame")]
pub struct Frame {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: Uuid,

    #[db(flatten)]
    pub size: Size,

    #[db(pgtype = "text")]
    pub label: String,
}

#[test]
fn the_column_trait_mirrors_the_inherent_items() {
    assert_eq!(
        <WidgetColumn as Column>::all().len(),
        WidgetColumn::all().len()
    );
    assert_eq!(Column::name(WidgetColumn::Name), "name");
}

#[test]
fn all_lists_every_column_in_declaration_order() {
    assert_eq!(
        WidgetColumn::all(),
        &[
            WidgetColumn::Id,
            WidgetColumn::Name,
            WidgetColumn::EmailVerifiedAt,
            WidgetColumn::CreatedAt,
            WidgetColumn::UpdatedAt,
            WidgetColumn::Version,
        ]
    );
}

#[test]
fn names_are_the_column_names() {
    let names: Vec<&str> = WidgetColumn::all()
        .iter()
        .map(|column| column.name())
        .collect();

    assert_eq!(
        names,
        [
            "id",
            "name",
            "email_verified_at",
            "created_at",
            "updated_at",
            "version"
        ]
    );
}

#[test]
fn a_fragment_derives_columns_too() {
    assert_eq!(
        SizeColumn::all()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        ["width", "height"]
    );
}

#[test]
fn all_selects_every_column() {
    let columns = Columns::all();

    assert_eq!(columns.len(), WidgetColumn::all().len());
    assert!(columns.contains(WidgetColumn::Name));
}

#[test]
fn a_selection_deserializes_from_column_names() {
    let columns: Columns<WidgetColumn> = serde_json::from_str(r#"["name","id"]"#).unwrap();

    assert_eq!(
        columns.names().collect::<Vec<_>>(),
        ["id", "name"],
        "the set iterates in declaration order, not request order"
    );
}

#[test]
fn a_selection_serializes_to_column_names() {
    let columns: Columns<WidgetColumn> = [WidgetColumn::Version, WidgetColumn::Id]
        .into_iter()
        .collect();

    assert_eq!(
        serde_json::to_string(&columns).unwrap(),
        r#"["id","version"]"#
    );
}

#[test]
fn an_unknown_column_fails_to_deserialize() {
    assert!(serde_json::from_str::<Columns<WidgetColumn>>(r#"["password"]"#).is_err());
}

#[test]
fn an_unknown_column_reports_the_known_columns() {
    let error = serde_json::from_str::<Columns<WidgetColumn>>(r#"["password"]"#).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("unknown column `password`"), "{message}");
    assert!(message.contains("email_verified_at"), "{message}");
}

#[test]
fn a_flattened_column_is_named_in_an_unknown_column_error() {
    let error = serde_json::from_str::<Columns<FrameColumn>>(r#"["password"]"#).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("width"), "{message}");
    assert!(message.contains("height"), "{message}");
}

#[test]
fn an_empty_selection_is_empty() {
    let columns = Columns::<WidgetColumn>::default();

    assert!(columns.is_empty());
    assert_eq!(columns.names().count(), 0);
}

#[test]
fn insert_remove_and_contains() {
    let mut columns = Columns::default();
    assert!(columns.insert(WidgetColumn::Name));
    assert!(!columns.insert(WidgetColumn::Name));
    assert!(columns.contains(WidgetColumn::Name));

    assert!(columns.remove(WidgetColumn::Name));
    assert!(!columns.contains(WidgetColumn::Name));
}

#[test]
fn a_column_carries_its_declared_type() {
    assert_eq!(WidgetColumn::Name.pg_type(), "text");
    assert_eq!(WidgetColumn::CreatedAt.pg_type(), "timestamptz");
}

#[test]
fn a_column_carries_the_type_inherited_from_its_field_type() {
    assert_eq!(WidgetColumn::Id.pg_type(), "uuid");
}

#[test]
fn a_column_carries_its_default() {
    assert_eq!(WidgetColumn::Id.pg_default(), Some("uuidv7()"));
    assert_eq!(WidgetColumn::Name.pg_default(), None);
}

#[test]
fn a_column_knows_its_nullability() {
    assert!(WidgetColumn::EmailVerifiedAt.nullable());
    assert!(!WidgetColumn::Name.nullable());
}

#[test]
fn a_column_carries_its_query_operators() {
    assert_eq!(
        WidgetColumn::Name.query_ops(),
        [
            FilterOperator::Eq,
            FilterOperator::Neq,
            FilterOperator::Like
        ]
    );
    assert!(WidgetColumn::EmailVerifiedAt.query_ops().is_empty());
}

#[test]
fn pg_types_follow_the_selection_order() {
    let columns: Columns<WidgetColumn> = [WidgetColumn::Version, WidgetColumn::Id]
        .into_iter()
        .collect();

    assert_eq!(columns.pg_types().collect::<Vec<_>>(), ["uuid", "bigint"]);
}

#[test]
fn metadata_columns_are_the_enum_variants() {
    let meta = Widget::meta();

    assert_eq!(meta.columns().len(), WidgetColumn::all().len());
    for (column, variant) in meta.columns().iter().zip(WidgetColumn::all()) {
        assert_eq!(column.name(), variant.name());
        assert_eq!(column.pg_type(), variant.pg_type());
        assert_eq!(column.pg_default(), variant.pg_default());
        assert_eq!(column.nullable(), variant.nullable());
        assert_eq!(column.query_ops(), variant.query_ops());
    }
}

#[test]
fn a_column_is_found_by_name() {
    let expected: &dyn ErasedColumn = &WidgetColumn::EmailVerifiedAt;

    let column = Widget::meta()
        .column("email_verified_at")
        .expect("a column");
    assert_eq!(column, expected);
    assert!(Widget::meta().column("password").is_none());
}

#[test]
fn a_flattened_table_names_the_fragment_columns_through_its_variant() {
    assert_eq!(
        FrameColumn::all(),
        &[
            FrameColumn::Id,
            FrameColumn::Size(SizeColumn::Width),
            FrameColumn::Size(SizeColumn::Height),
            FrameColumn::Label,
        ]
    );
    assert_eq!(
        FrameColumn::all()
            .iter()
            .map(|column| column.name())
            .collect::<Vec<_>>(),
        ["id", "width", "height", "label"]
    );
}

#[test]
fn a_flattened_table_matches_the_metadata_columns() {
    let meta = Frame::meta();

    assert_eq!(meta.columns().len(), FrameColumn::all().len());
    for (column, variant) in meta.columns().iter().zip(FrameColumn::all()) {
        assert_eq!(column.name(), variant.name());
        assert_eq!(column.pg_type(), variant.pg_type());
        assert_eq!(column.nullable(), variant.nullable());
    }
}

#[test]
fn a_flattened_column_is_selectable() {
    let columns: Columns<FrameColumn> = serde_json::from_str(r#"["width","label"]"#).unwrap();

    assert_eq!(columns.names().collect::<Vec<_>>(), ["width", "label"]);
    assert!(columns.contains(FrameColumn::Size(SizeColumn::Width)));
    assert!(!columns.contains(FrameColumn::Size(SizeColumn::Height)));
    assert_eq!(
        serde_json::to_string(&columns).unwrap(),
        r#"["width","label"]"#
    );
}

#[test]
fn a_flattened_column_resolves_by_name() {
    assert_eq!(
        FrameColumn::from_name("height"),
        Some(FrameColumn::Size(SizeColumn::Height))
    );
    assert_eq!(FrameColumn::from_name("depth"), None);
}

#[test]
fn a_column_keeps_its_fragment_metadata() {
    let column = FrameColumn::Size(SizeColumn::Width);

    assert_eq!(column.pg_type(), "int4");
    assert_eq!(column.pg_default(), None);
    assert!(!column.nullable());
    assert!(column.query_ops().is_empty());
}
