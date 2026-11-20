use email_service::db::models::email::Email;
use email_service::db::models::email::EmailId;
use serde_json::Value;
use uuid::Uuid;

fn sample() -> Email {
    Email {
        id: EmailId(Uuid::from_u128(1)),
        from: "sender@example.com".to_owned(),
        to: "recipient@example.com".to_owned(),
        subject: "Sample".to_owned(),
        body: "Hello".to_owned(),
        sent_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        version: 0,
    }
}

#[test]
fn null_optionals_are_omitted() {
    let json = serde_json::to_value(sample()).unwrap();

    assert!(json.get("sent_at").is_none(), "{json}");
}

#[test]
fn set_optionals_are_present() {
    let mut email = sample();
    email.sent_at = Some(chrono::Utc::now());

    let json = serde_json::to_value(email).unwrap();

    assert!(
        matches!(json.get("sent_at"), Some(Value::String(_))),
        "{json}"
    );
}

#[test]
fn omitted_optionals_deserialize() {
    let json = serde_json::json!({
        "id": Uuid::from_u128(1),
        "from": "sender@example.com",
        "to": "recipient@example.com",
        "subject": "Sample",
        "body": "Hello",
        "created_at": chrono::Utc::now(),
        "updated_at": chrono::Utc::now(),
        "version": 0,
    });

    let email: Email = serde_json::from_value(json).unwrap();

    assert!(email.sent_at.is_none());
}
