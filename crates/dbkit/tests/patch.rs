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
