use chrono::Utc;
use garde::Validate;
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
        token_hash: "sample".to_owned(),
        expires_at: Utc::now(),
        revoked_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        version: 0,
    }
}

#[test]
fn valid_rows_pass_validation() {
    assert!(user().validate().is_ok());
    assert!(session().validate().is_ok());
}

#[test]
fn empty_token_hash_fails_validation() {
    let mut session = session();
    session.token_hash = String::new();

    assert!(session.validate().is_err());
}

#[test]
fn invalid_email_fails_validation() {
    let mut user = user();
    user.email = Email("nope".to_owned());

    assert!(user.validate().is_err());
}

#[test]
fn deserialization_does_not_validate() {
    let mut json = serde_json::to_value(user()).unwrap();
    json["email"] = serde_json::json!("nope");

    let user: User = serde_json::from_value(json).unwrap();

    assert_eq!(user.email.0, "nope");
    assert!(user.validate().is_err());
}
