use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::Patch;
use garde::Validate;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

/// A newtype that carries a rule, so its patch field dives into it.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type, garde::Validate,
)]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Label(#[garde(length(min = 3))] pub String);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    pub name: String,

    #[db(pgtype = "text")]
    pub label: Label,

    #[db(pgtype = "text")]
    pub note: Option<String>,

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "translation")]
pub struct Translation {
    #[db(primary_key)]
    #[db(pgtype = "text")]
    pub lang: String,

    #[db(primary_key)]
    #[db(pgtype = "text")]
    pub key: String,

    #[db(pgtype = "text")]
    pub value: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "append_only")]
pub struct AppendOnly {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    pub note: String,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    pub created_at: DateTime<Utc>,
}

fn id() -> Uuid {
    Uuid::from_u128(1)
}

#[test]
fn patch_requires_primary_key_and_version() {
    assert!(serde_json::from_value::<WidgetPatch>(json!({ "id": id() })).is_err());
    assert!(serde_json::from_value::<WidgetPatch>(json!({ "version": 3 })).is_err());
    assert!(serde_json::from_value::<WidgetPatch>(json!({ "id": id(), "version": 3 })).is_ok());
}

#[test]
fn patch_fields_cover_business_columns_only() {
    let patch: WidgetPatch = serde_json::from_value(json!({ "id": id(), "version": 3 })).unwrap();

    assert_eq!(patch.name, Patch::Absent);
    assert_eq!(patch.label, Patch::Absent);
    assert_eq!(patch.note, Patch::Absent);
}

#[test]
fn patch_round_trips_all_states() {
    let patch = WidgetPatch {
        id: WidgetId(id()),
        name: Patch::Set("Thingie".to_owned()),
        label: Patch::Set(Label("label".to_owned())),
        note: Patch::Null,
        version: 3,
    };

    let json = json!({
        "id": id(),
        "name": "Thingie",
        "label": "label",
        "note": null,
        "version": 3,
    });
    assert_eq!(serde_json::to_value(&patch).unwrap(), json);

    let parsed: WidgetPatch = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.name, patch.name);
    assert_eq!(parsed.label, patch.label);
    assert_eq!(parsed.note, patch.note);
}

#[test]
fn absent_fields_are_omitted_from_responses() {
    let patch = WidgetPatch {
        id: WidgetId(id()),
        name: Patch::Absent,
        label: Patch::Absent,
        note: Patch::Absent,
        version: 3,
    };

    assert_eq!(
        serde_json::to_value(&patch).unwrap(),
        json!({ "id": id(), "version": 3 })
    );
}

#[test]
fn patch_validates_only_set_values() {
    let absent: WidgetPatch = serde_json::from_value(json!({ "id": id(), "version": 3 })).unwrap();
    assert!(absent.validate().is_ok());

    let cleared: WidgetPatch = serde_json::from_value(json!({
        "id": id(),
        "version": 3,
        "label": null,
        "note": null,
    }))
    .unwrap();
    assert!(cleared.validate().is_ok());
}

#[test]
fn patch_dives_into_a_set_payload() {
    let valid: WidgetPatch = serde_json::from_value(json!({
        "id": id(),
        "version": 3,
        "label": "label",
    }))
    .unwrap();
    assert!(valid.validate().is_ok());

    // Deserialization accepts the value; the payload's rule rejects it.
    let invalid: WidgetPatch = serde_json::from_value(json!({
        "id": id(),
        "version": 3,
        "label": "no",
    }))
    .unwrap();
    let report = invalid.validate().unwrap_err();
    assert!(report.to_string().starts_with("label"), "{report}");
}

#[test]
fn missing_required_fields_are_deserialization_errors() {
    assert!(
        serde_json::from_value::<WidgetPatch>(json!({ "id": id(), "name": "Thingie" })).is_err()
    );
    // A wrong JSON type is a shape error, not a validation error.
    assert!(
        serde_json::from_value::<WidgetPatch>(json!({ "id": id(), "version": 3, "name": 7 }))
            .is_err()
    );
}

#[test]
fn composite_key_patch_requires_every_key_column() {
    assert!(
        serde_json::from_value::<TranslationPatch>(json!({ "key": "greeting", "version": 1 }))
            .is_err()
    );
    assert!(
        serde_json::from_value::<TranslationPatch>(json!({ "lang": "en", "version": 1 })).is_err()
    );
    assert!(
        serde_json::from_value::<TranslationPatch>(
            json!({ "lang": "en", "key": "greeting", "version": 1 })
        )
        .is_ok()
    );
}

#[test]
fn composite_key_columns_are_plain_and_the_rest_is_patched() {
    let patch: TranslationPatch =
        serde_json::from_value(json!({ "lang": "en", "key": "greeting", "version": 1 })).unwrap();

    assert_eq!(
        (patch.lang.as_str(), patch.key.as_str()),
        ("en", "greeting")
    );
    assert_eq!(patch.value, Patch::Absent);
    assert!(patch.validate().is_ok());
}

#[test]
fn model_without_version_has_no_version_field() {
    assert!(serde_json::from_value::<AppendOnlyPatch>(json!({})).is_err());

    let patch: AppendOnlyPatch = serde_json::from_value(json!({ "id": id() })).unwrap();

    assert_eq!(patch.note, Patch::Absent);
    assert_eq!(serde_json::to_value(&patch).unwrap(), json!({ "id": id() }));
    assert!(patch.validate().is_ok());
}

/// A column group embedded in `Frame`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(fragment)]
pub struct Size {
    #[db(pgtype = "int4")]
    pub width: i32,

    #[db(pgtype = "text")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "frame")]
pub struct Frame {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,

    #[db(flatten)]
    pub size: Size,

    #[db(pgtype = "text")]
    pub label: String,

    #[db(version)]
    #[db(pgtype = "bigint")]
    pub version: i64,
}

#[test]
fn flattened_patch_fields_are_part_of_the_parent_object() {
    let patch: FramePatch = serde_json::from_value(json!({
        "id": id(),
        "version": 1,
        "width": 640,
        "note": null,
        "label": "photo",
    }))
    .unwrap();

    assert_eq!(patch.size.width, Patch::Set(640));
    assert_eq!(patch.size.note, Patch::Null);
    assert_eq!(patch.label, Patch::Set("photo".to_owned()));
    assert!(patch.validate().is_ok());
}

#[test]
fn absent_flattened_fields_are_omitted_from_responses() {
    let patch: FramePatch = serde_json::from_value(json!({ "id": id(), "version": 1 })).unwrap();
    assert_eq!(patch.size.width, Patch::Absent);
    assert_eq!(patch.size.note, Patch::Absent);
    assert_eq!(patch.label, Patch::Absent);

    assert_eq!(
        serde_json::to_value(&patch).unwrap(),
        json!({ "id": id(), "version": 1 })
    );
}
