use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::Patch;
use serde_json::json;
use uuid::Uuid;
use zod_rs::prelude::StringSchema;
use zod_rs::prelude::string;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct WidgetId(pub Uuid);

impl WidgetId {
    pub fn schema() -> StringSchema {
        string()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, DbModel, DbPatch)]
#[db(table = "widget")]
pub struct Widget {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    pub id: WidgetId,

    #[db(pgtype = "text")]
    pub name: String,

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

fn id() -> Uuid {
    Uuid::from_u128(1)
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

    let lang: String = patch.lang;
    let key: String = patch.key;
    assert_eq!((lang.as_str(), key.as_str()), ("en", "greeting"));
    assert_eq!(patch.value, Patch::Absent);
}

#[test]
fn composite_key_patch_validates_both_key_columns() {
    let valid = json!({ "lang": "en", "key": "greeting", "version": 1, "value": "Hello" });
    assert!(TranslationPatch::validate_and_parse(&valid).is_ok());

    let missing_lang = json!({ "key": "greeting", "version": 1 });
    assert!(TranslationPatch::validate_and_parse(&missing_lang).is_err());

    let wrong_key_type = json!({ "lang": "en", "key": 7, "version": 1 });
    assert!(TranslationPatch::validate_and_parse(&wrong_key_type).is_err());
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

#[test]
fn model_without_version_has_no_version_field() {
    assert!(serde_json::from_value::<AppendOnlyPatch>(json!({})).is_err());

    let patch: AppendOnlyPatch = serde_json::from_value(json!({ "id": id() })).unwrap();

    assert_eq!(patch.note, Patch::Absent);
    assert_eq!(serde_json::to_value(&patch).unwrap(), json!({ "id": id() }));
}

#[test]
fn model_without_version_validates_without_one() {
    let valid = json!({ "id": id(), "note": "Hello" });
    assert!(AppendOnlyPatch::validate_and_parse(&valid).is_ok());

    let missing_id = json!({ "note": "Hello" });
    assert!(AppendOnlyPatch::validate_and_parse(&missing_id).is_err());
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
    assert_eq!(patch.note, Patch::Absent);
}

#[test]
fn patch_round_trips_all_states() {
    let patch = WidgetPatch {
        id: WidgetId(id()),
        name: Patch::Set("Thingie".to_owned()),
        note: Patch::Null,
        version: 3,
    };

    let json = json!({ "id": id(), "name": "Thingie", "note": null, "version": 3 });
    assert_eq!(serde_json::to_value(&patch).unwrap(), json);

    let parsed: WidgetPatch = serde_json::from_value(json).unwrap();
    assert_eq!(parsed.name, patch.name);
    assert_eq!(parsed.note, patch.note);
}

#[test]
fn absent_fields_are_omitted_from_responses() {
    let patch = WidgetPatch {
        id: WidgetId(id()),
        name: Patch::Absent,
        note: Patch::Absent,
        version: 3,
    };

    assert_eq!(
        serde_json::to_value(&patch).unwrap(),
        json!({ "id": id(), "version": 3 })
    );
}

#[test]
fn patch_validates_set_values() {
    let valid = json!({ "id": id(), "version": 3, "name": "Thingie", "note": null });
    assert!(WidgetPatch::validate_and_parse(&valid).is_ok());

    let invalid_type = json!({ "id": id(), "version": 3, "name": 7 });
    assert!(WidgetPatch::validate_and_parse(&invalid_type).is_err());

    let missing_version = json!({ "id": id(), "name": "Thingie" });
    assert!(WidgetPatch::validate_and_parse(&missing_version).is_err());
}
