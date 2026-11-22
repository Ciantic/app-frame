use serde_json::Value;
use user_service::db::models::user::User;
use user_service::db::models::user::UserId;
use user_service::db::models::user_session::UserSession;
use user_service::db::models::user_session::UserSessionId;
use user_service::db::primitives::email::Email;
use uuid::Uuid;

fn user() -> User {
    User {
        id: UserId(Uuid::from_u128(1)),
        email: Email("sample@example.com".to_owned()),
        email_verified_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        deleted_at: None,
        version: 0,
    }
}

fn session() -> UserSession {
    UserSession {
        id: UserSessionId(Uuid::from_u128(2)),
        user_id: UserId(Uuid::from_u128(1)),
        token_hash: "sample".to_owned(),
        expires_at: chrono::Utc::now(),
        revoked_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        version: 0,
    }
}

#[test]
fn null_optionals_are_omitted() {
    let user = serde_json::to_value(user()).unwrap();
    let session = serde_json::to_value(session()).unwrap();

    assert!(user.get("email_verified_at").is_none(), "{user}");
    assert!(user.get("deleted_at").is_none(), "{user}");
    assert!(session.get("revoked_at").is_none(), "{session}");
}

#[test]
fn set_optionals_are_present() {
    let mut user = user();
    user.email_verified_at = Some(chrono::Utc::now());
    user.deleted_at = Some(chrono::Utc::now());

    let user = serde_json::to_value(user).unwrap();

    assert!(
        matches!(user.get("email_verified_at"), Some(Value::String(_))),
        "{user}"
    );
    assert!(
        matches!(user.get("deleted_at"), Some(Value::String(_))),
        "{user}"
    );
}

#[test]
fn omitted_optionals_deserialize() {
    let json = serde_json::json!({
        "id": Uuid::from_u128(1),
        "user_id": Uuid::from_u128(1),
        "token_hash": "sample",
        "expires_at": chrono::Utc::now(),
        "created_at": chrono::Utc::now(),
        "updated_at": chrono::Utc::now(),
        "version": 0,
    });

    let session: UserSession = serde_json::from_value(json).unwrap();

    assert!(session.revoked_at.is_none());
}
