use chrono::DateTime;
use chrono::TimeZone;
use chrono::Utc;
use dbkit::DbInsert;
use dbkit::DbModel;
use garde::Validate;
use serde_json::json;
use uuid::Uuid;

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, dbkit::DbType,
)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

fn id() -> WidgetId {
    WidgetId(Uuid::from_u128(1))
}

#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, garde::Validate,
)]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Label(#[garde(length(min = 3))] pub String);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    pub name: Label,

    #[db(pgtype = "text")]
    pub note: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(table = "inherited_required")]
pub struct InheritedRequired {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(table = "append_only")]
pub struct AppendOnly {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: Uuid,

    #[db(pgtype = "text")]
    pub note: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(table = "all_defaults")]
pub struct AllDefaults {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(pgdefault = "uuidv7()")]
    pub id: Uuid,

    #[db(pgtype = "text")]
    #[db(pgdefault = "'label'")]
    pub label: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(table = "translation")]
pub struct Translation {
    #[db(primary_key)]
    #[db(pgtype = "text")]
    pub lang: String,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    #[db(pgdefault = "'hello'")]
    pub key: String,

    #[db(pgtype = "text")]
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
#[db(fragment)]
pub struct Size {
    #[db(pgtype = "int4")]
    pub width: i32,

    #[db(pgtype = "text")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbInsert)]
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
fn a_required_only_insert_renders_every_required_column() {
    let insert = AppendOnlyInsert::new(Uuid::from_u128(1), "kept");

    let rendered = AppendOnly::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"append_only\" (\"id\", \"note\") VALUES ($1, $2) RETURNING \"id\""
    );
    assert_eq!(
        rendered[0].values,
        vec![json!(Uuid::from_u128(1)), json!("kept")]
    );
}

#[test]
fn omitting_pgdefault_makes_an_inherited_type_required() {
    let insert = InheritedRequiredInsert::new(id());

    let rendered = InheritedRequired::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"inherited_required\" (\"id\") VALUES ($1) RETURNING \"id\""
    );
    assert_eq!(rendered[0].values, vec![json!(id())]);
}

#[test]
fn an_optional_field_left_none_is_omitted() {
    let insert = WidgetInsert::new(Label("abc".to_owned()));

    let rendered = Widget::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"widget\" (\"name\") VALUES ($1) RETURNING \"id\""
    );
    assert_eq!(rendered[0].values, vec![json!("abc")]);
}

#[test]
fn an_optional_field_set_adds_a_column() {
    let insert = WidgetInsert::new(Label("abc".to_owned())).set_note("kept");

    let rendered = Widget::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"widget\" (\"name\", \"note\") VALUES ($1, $2) RETURNING \"id\""
    );
    assert_eq!(rendered[0].values, vec![json!("abc"), json!("kept")]);
}

#[test]
fn an_all_optional_entry_renders_default_values() {
    let rendered = AllDefaults::insert([AllDefaultsInsert::new()])
        .render()
        .unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"all_defaults\" DEFAULT VALUES RETURNING \"id\""
    );
    assert!(rendered[0].values.is_empty());
}

#[test]
fn a_composite_key_can_default_one_column() {
    let insert = TranslationInsert::new("en", "hi");

    let rendered = Translation::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"translation\" (\"lang\", \"value\") VALUES ($1, $2) RETURNING \"lang\", \
         \"key\""
    );
    assert_eq!(rendered[0].values, vec![json!("en"), json!("hi")]);
}

#[test]
fn setting_a_defaulted_key_includes_it() {
    let insert = TranslationInsert::new("en", "hi").set_key("greeting");

    let rendered = Translation::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"translation\" (\"lang\", \"key\", \"value\") VALUES ($1, $2, $3) RETURNING \
         \"lang\", \"key\""
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("en"), json!("greeting"), json!("hi")]
    );
}

#[test]
fn a_flattened_fragment_contributes_columns_in_order() {
    let size = SizeInsert::new(640).set_note("wide");
    let insert = FrameInsert::new(Uuid::from_u128(1), size, "photo");

    let rendered = Frame::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"frame\" (\"id\", \"width\", \"note\", \"label\") VALUES ($1, $2, $3, $4) \
         RETURNING \"id\""
    );
    assert_eq!(
        rendered[0].values,
        vec![
            json!(Uuid::from_u128(1)),
            json!(640),
            json!("wide"),
            json!("photo")
        ]
    );
}

#[test]
fn a_supplied_created_at_also_supplies_updated_at() {
    let at = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let insert = WidgetInsert::new(Label("abc".to_owned())).set_created_at(at);

    let rendered = Widget::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"widget\" (\"name\", \"created_at\", \"updated_at\") VALUES ($1, $2, $3) \
         RETURNING \"id\""
    );
    assert_eq!(rendered[0].values, vec![json!("abc"), json!(at), json!(at)]);
}

#[test]
fn a_synthesized_updated_at_keeps_model_order() {
    let at = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let insert = WidgetInsert::new(Label("abc".to_owned()))
        .set_created_at(at)
        .set_version(2);

    let rendered = Widget::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"widget\" (\"name\", \"created_at\", \"updated_at\", \"version\") VALUES \
         ($1, $2, $3, $4) RETURNING \"id\""
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("abc"), json!(at), json!(at), json!(2)]
    );
}

#[test]
fn an_explicit_updated_at_is_not_overwritten() {
    let created = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let updated = Utc.with_ymd_and_hms(2021, 1, 1, 0, 0, 0).unwrap();
    let insert = WidgetInsert::new(Label("abc".to_owned()))
        .set_created_at(created)
        .set_updated_at(updated);

    let rendered = Widget::insert([insert]).render().unwrap();

    assert_eq!(
        rendered[0].values,
        vec![json!("abc"), json!(created), json!(updated)]
    );
}

#[test]
fn every_entry_is_its_own_statement() {
    let first = WidgetInsert::new(Label("one".to_owned()));
    let second = WidgetInsert::new(Label("two".to_owned())).set_note("kept");

    let rendered = Widget::insert([first, second]).render().unwrap();

    assert_eq!(rendered.len(), 2);
    assert_eq!(
        rendered[0].sql,
        "INSERT INTO \"widget\" (\"name\") VALUES ($1) RETURNING \"id\""
    );
    assert_eq!(
        rendered[1].sql,
        "INSERT INTO \"widget\" (\"name\", \"note\") VALUES ($1, $2) RETURNING \"id\""
    );
    assert_eq!(rendered[0].values, vec![json!("one")]);
    assert_eq!(rendered[1].values, vec![json!("two"), json!("kept")]);
}

#[test]
fn the_key_structs_hold_only_key_and_version_columns() {
    let key = TranslationPK {
        lang: "en".to_owned(),
        key: "greeting".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(&key).unwrap(),
        json!({ "lang": "en", "key": "greeting" })
    );

    let key = WidgetPKV {
        id: id(),
        version: 4,
    };

    assert_eq!(
        serde_json::to_value(&key).unwrap(),
        json!({ "id": id(), "version": 4 })
    );

    let key = AppendOnlyPKV {
        id: Uuid::from_u128(1),
    };

    assert_eq!(
        serde_json::to_value(&key).unwrap(),
        json!({ "id": Uuid::from_u128(1) })
    );
}

#[test]
fn an_empty_batch_renders_nothing() {
    let rendered = Widget::insert([]).render().unwrap();

    assert!(rendered.is_empty());
}

#[test]
fn the_builder_takes_required_fields_and_sets_optional_ones() {
    let insert = WidgetInsert::new(Label("abc".to_owned()))
        .set_id(id())
        .set_note("kept")
        .set_version(4);

    assert_eq!(insert.id, Some(id()));
    assert_eq!(insert.name, Label("abc".to_owned()));
    assert_eq!(insert.note.as_deref(), Some("kept"));
    assert_eq!(insert.version, Some(4));
}

#[test]
fn the_builder_starts_optional_fields_absent() {
    let insert = WidgetInsert::new(Label("abc".to_owned()));

    assert_eq!(insert.id, None);
    assert_eq!(insert.note, None);
    assert_eq!(insert.created_at, None);
    assert_eq!(insert.updated_at, None);
    assert_eq!(insert.version, None);
}

#[test]
fn a_missing_required_field_is_a_deserialization_error() {
    assert!(serde_json::from_str::<WidgetInsert>(r#"{"note":"kept"}"#).is_err());
}

#[test]
fn missing_and_null_optional_fields_deserialize_to_none() {
    let missing: WidgetInsert = serde_json::from_str(r#"{"name":"abc"}"#).unwrap();
    assert_eq!(missing.note, None);
    assert_eq!(missing.id, None);

    let null: WidgetInsert = serde_json::from_str(r#"{"name":"abc","note":null}"#).unwrap();
    assert_eq!(null.note, None);
}

#[test]
fn absent_optional_fields_are_omitted_from_json() {
    let insert = WidgetInsert::new(Label("abc".to_owned()));

    assert_eq!(serde_json::to_string(&insert).unwrap(), r#"{"name":"abc"}"#);
}

#[test]
fn validation_dives_into_required_payloads() {
    assert!(
        WidgetInsert::new(Label("abc".to_owned()))
            .validate()
            .is_ok()
    );
    assert!(
        WidgetInsert::new(Label("ab".to_owned()))
            .validate()
            .is_err()
    );
}
