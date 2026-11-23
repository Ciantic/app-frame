use garde::Validate;
use user_service::db::primitives::email::Email;
use user_service::db::primitives::session_token::SessionToken;

#[test]
fn email_rule_rejects_missing_at_sign() {
    assert!(Email("sample@example.com".to_owned()).validate().is_ok());

    let report = Email("nope".to_owned()).validate().unwrap_err();
    assert!(report.to_string().contains("email"), "{report}");
}

#[test]
fn token_rule_rejects_empty() {
    assert!(SessionToken("hash".to_owned()).validate().is_ok());
    assert!(SessionToken(String::new()).validate().is_err());
}

#[test]
fn deserialization_does_not_validate() {
    let email: Email = serde_json::from_value(serde_json::json!("nope")).unwrap();

    assert_eq!(email.0, "nope");
}
