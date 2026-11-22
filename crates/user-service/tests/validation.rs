use chrono::Utc;
use serde_json::json;
use user_service::db::models::user::User;
use user_service::db::models::user::UserId;
use user_service::db::models::user_session::UserSession;
use user_service::db::models::user_session::UserSessionId;
use user_service::db::primitives::email::Email;
use user_service::db::primitives::session_token::SessionToken;
use uuid::Uuid;

fn user() -> User {
    User {
        id: UserId(Uuid::from_u128(1)),
        email: Email("sample@example.com".to_owned()),
        email_verified_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted_at: None,
        version: 0,
    }
}

fn session() -> UserSession {
    UserSession {
        id: UserSessionId(Uuid::from_u128(2)),
        user_id: UserId(Uuid::from_u128(1)),
        token_hash: SessionToken("sample".to_owned()),
        expires_at: Utc::now(),
        revoked_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        version: 0,
    }
}

#[test]
fn valid_rows_pass_validation() {
    assert!(User::validate_and_parse(&serde_json::to_value(user()).unwrap()).is_ok());
    assert!(UserSession::validate_and_parse(&serde_json::to_value(session()).unwrap()).is_ok());
}

#[test]
fn empty_token_hash_fails_validation() {
    let mut json = serde_json::to_value(session()).unwrap();
    json["token_hash"] = json!("");

    assert!(UserSession::validate_and_parse(&json).is_err());
}

#[test]
fn invalid_email_fails_validation() {
    let mut json = serde_json::to_value(user()).unwrap();
    json["email"] = json!("nope");

    assert!(User::validate_and_parse(&json).is_err());
}

#[test]
fn deserialization_does_not_validate() {
    let mut json = serde_json::to_value(user()).unwrap();
    json["email"] = json!("nope");

    let user: User = serde_json::from_value(json).unwrap();

    assert_eq!(user.email.0, "nope");
}
