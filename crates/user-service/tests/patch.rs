use chrono::Utc;
use dbkit::Patch;
use serde_json::json;
use user_service::db::models::user::UserPatch;
use user_service::db::models::user_session::UserSessionPatch;
use user_service::db::primitives::email::Email;
use user_service::db::primitives::session_token::SessionToken;
use uuid::Uuid;

fn user_id() -> Uuid {
    Uuid::from_u128(1)
}

#[test]
fn user_patch_reads_all_three_states() {
    let patch: UserPatch = serde_json::from_value(json!({
        "id": user_id(),
        "version": 4,
        "email": "sample@example.com",
        "email_verified_at": null,
    }))
    .unwrap();

    assert_eq!(
        patch.email,
        Patch::Set(Email("sample@example.com".to_owned()))
    );
    assert_eq!(patch.email_verified_at, Patch::Null);
    assert_eq!(patch.deleted_at, Patch::Absent);
}

#[test]
fn user_patch_requires_id_and_version() {
    assert!(serde_json::from_value::<UserPatch>(json!({ "version": 4 })).is_err());
    assert!(serde_json::from_value::<UserPatch>(json!({ "id": user_id() })).is_err());
}

#[test]
fn user_patch_validates_set_values() {
    let valid = json!({ "id": user_id(), "version": 4, "email": "sample@example.com" });
    assert!(UserPatch::validate_and_parse(&valid).is_ok());

    let invalid_email = json!({ "id": user_id(), "version": 4, "email": "nope" });
    assert!(UserPatch::validate_and_parse(&invalid_email).is_err());
}

#[test]
fn user_session_patch_validates_token() {
    let valid = json!({
        "id": Uuid::from_u128(2),
        "version": 1,
        "token_hash": "hash",
        "expires_at": Utc::now(),
    });
    let patch: UserSessionPatch = UserSessionPatch::validate_and_parse(&valid).unwrap();

    assert_eq!(
        patch.token_hash,
        Patch::Set(SessionToken("hash".to_owned()))
    );

    let empty_token = json!({
        "id": Uuid::from_u128(2),
        "version": 1,
        "token_hash": "",
    });
    assert!(UserSessionPatch::validate_and_parse(&empty_token).is_err());
}
