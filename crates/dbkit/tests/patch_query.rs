use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::Patch;
use dbkit::PatchError;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

fn id() -> WidgetId {
    WidgetId(Uuid::from_u128(1))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: WidgetId,

    #[db(pgtype = "text")]
    name: String,

    #[db(pgtype = "text")]
    note: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    created_at: chrono::DateTime<chrono::Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    updated_at: chrono::DateTime<chrono::Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "translation")]
pub struct Translation {
    #[db(primary_key)]
    #[db(pgtype = "text")]
    lang: String,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    key: String,

    #[db(pgtype = "text")]
    value: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "append_only")]
pub struct AppendOnly {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: WidgetId,

    #[db(pgtype = "text")]
    note: String,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(fragment)]
pub struct Size {
    #[db(pgtype = "int4")]
    width: i32,

    #[db(pgtype = "text")]
    note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "frame")]
pub struct Frame {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    id: WidgetId,

    #[db(flatten)]
    size: Size,

    #[db(pgtype = "text")]
    label: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    version: i64,
}

#[test]
fn a_set_and_a_cleared_field_render_one_statement() {
    let patch = WidgetPatch {
        id: id(),
        name: Patch::Set("Thingie".to_owned()),
        note: Patch::Null,
        version: 3,
    };

    let rendered = Widget::patch([patch]).render().unwrap();

    assert_eq!(rendered.len(), 1);
    assert_eq!(
        rendered[0].sql,
        "UPDATE \"widget\" SET \"name\" = $1, \"note\" = NULL, \"version\" = \"version\" + 1, \
         \"updated_at\" = now() WHERE \"id\" = $2 AND \"version\" = $3"
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("Thingie"), json!(id()), json!(3)]
    );
}

#[test]
fn absent_fields_are_omitted() {
    let patch = WidgetPatch {
        id: id(),
        name: Patch::Set("Thingie".to_owned()),
        note: Patch::Absent,
        version: 3,
    };

    let rendered = Widget::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"widget\" SET \"name\" = $1, \"version\" = \"version\" + 1, \"updated_at\" = \
         now() WHERE \"id\" = $2 AND \"version\" = $3"
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("Thingie"), json!(id()), json!(3)]
    );
}

#[test]
fn every_entry_is_its_own_statement() {
    let first = WidgetPatch {
        id: id(),
        name: Patch::Set("one".to_owned()),
        note: Patch::Absent,
        version: 1,
    };
    let second = WidgetPatch {
        id: id(),
        name: Patch::Absent,
        note: Patch::Null,
        version: 2,
    };

    let rendered = Widget::patch([first, second]).render().unwrap();

    assert_eq!(rendered.len(), 2);
    assert_eq!(
        rendered[1].sql,
        "UPDATE \"widget\" SET \"note\" = NULL, \"version\" = \"version\" + 1, \"updated_at\" = \
         now() WHERE \"id\" = $1 AND \"version\" = $2"
    );
    assert_eq!(rendered[1].values, vec![json!(id()), json!(2)]);
}

#[test]
fn a_composite_key_keeps_declaration_order() {
    let patch = TranslationPatch::new("en".to_owned(), "greeting".to_owned(), 2).set_value("hi");

    assert_eq!(patch.lang, "en");
    assert_eq!(patch.key, "greeting");

    let rendered = Translation::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"translation\" SET \"value\" = $1, \"version\" = \"version\" + 1 WHERE \"lang\" \
         = $2 AND \"key\" = $3 AND \"version\" = $4"
    );
    assert_eq!(
        rendered[0].values,
        vec![json!("hi"), json!("en"), json!("greeting"), json!(2)]
    );
}

#[test]
fn a_model_without_version_has_no_predicate_or_bump() {
    let patch = AppendOnlyPatch {
        id: id(),
        note: Patch::Set("kept".to_owned()),
    };

    let rendered = AppendOnly::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"append_only\" SET \"note\" = $1 WHERE \"id\" = $2"
    );
    assert_eq!(rendered[0].values, vec![json!("kept"), json!(id())]);
}

#[test]
fn a_flattened_patch_contributes_its_columns() {
    let patch = FramePatch {
        id: id(),
        size: SizePatch {
            width: Patch::Set(640),
            note: Patch::Null,
        },
        label: Patch::Absent,
        version: 1,
    };

    let rendered = Frame::patch([patch]).render().unwrap();

    assert_eq!(
        rendered[0].sql,
        "UPDATE \"frame\" SET \"width\" = $1, \"note\" = NULL, \"version\" = \"version\" + 1 \
         WHERE \"id\" = $2 AND \"version\" = $3"
    );
    assert_eq!(rendered[0].values, vec![json!(640), json!(id()), json!(1)]);
}

#[test]
fn the_builder_takes_required_fields_and_sets_optional_ones() {
    let patch = WidgetPatch::new(id(), 3).set_name("Thingie").clear_note();

    assert_eq!(patch.id, id());
    assert_eq!(patch.version, 3);
    assert_eq!(patch.name, Patch::Set("Thingie".to_owned()));
    assert_eq!(patch.note, Patch::Null);
}

#[test]
fn the_builder_starts_optional_fields_absent() {
    let patch = TranslationPatch::new("en".to_owned(), "greeting".to_owned(), 2);

    assert_eq!(patch.value, Patch::Absent);
}

#[test]
fn a_model_without_version_takes_only_the_key() {
    let patch = AppendOnlyPatch::new(id()).set_note("kept");

    assert_eq!(patch.id, id());
    assert_eq!(patch.note, Patch::Set("kept".to_owned()));
}

#[test]
fn the_builder_recurses_into_a_flattened_patch() {
    let patch = FramePatch::new(id(), 1)
        .set_size(SizePatch::new().set_width(640).clear_note())
        .set_label("photo");

    assert_eq!(patch.size.width, Patch::Set(640));
    assert_eq!(patch.size.note, Patch::Null);
    assert_eq!(patch.label, Patch::Set("photo".to_owned()));
}

#[test]
fn an_entry_without_changes_is_rejected() {
    let patch = WidgetPatch {
        id: id(),
        name: Patch::Absent,
        note: Patch::Absent,
        version: 3,
    };

    let error = Widget::patch([patch]).render().unwrap_err();

    assert!(matches!(error, PatchError::Empty { index: 0 }), "{error}");
    assert_eq!(error.to_string(), "patch entry 0 changes no column");
}

#[test]
fn an_empty_batch_renders_nothing() {
    let rendered = Widget::patch([]).render().unwrap();

    assert!(rendered.is_empty());
}
