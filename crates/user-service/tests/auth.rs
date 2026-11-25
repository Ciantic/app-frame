use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use chrono::Duration;
use chrono::Utc;
use pgtest_server::PgtestServer;
use sha2::Digest;
use sha2::Sha256;
use user_service::auth::CurrentSession;
use user_service::db::schema::SCHEMA;
use user_service::state::AppState;
use uuid::Uuid;

const TOKEN: &str = "secret-session-token";

fn parts(header: header::HeaderName, value: &str) -> Parts {
    axum::http::Request::builder()
        .header(header, value)
        .body(())
        .unwrap()
        .into_parts()
        .0
}

async fn state() -> (AppState, Uuid) {
    let db = PgtestServer::start().await.database().await;
    SCHEMA.apply(&db).await.unwrap();

    let user_id = Uuid::from_u128(1);
    let session_id = Uuid::from_u128(2);
    let hash = hex::encode(Sha256::digest(TOKEN.as_bytes()));

    sqlx::query(r#"INSERT INTO "user" (id, email) VALUES ($1, $2)"#)
        .bind(user_id)
        .bind("sample@example.com")
        .execute(&db)
        .await
        .unwrap();

    sqlx::query(
        r#"INSERT INTO "user_session" (id, user_id, token_hash, expires_at)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(session_id)
    .bind(user_id)
    .bind(hash)
    .bind(Utc::now() + Duration::hours(1))
    .execute(&db)
    .await
    .unwrap();

    (AppState::new(db), session_id)
}

#[tokio::test]
async fn reads_the_session_from_a_cookie() {
    let (state, session_id) = state().await;
    let mut parts = parts(header::COOKIE, &format!("other=1; session={TOKEN}"));

    let session = CurrentSession::from_request_parts(&mut parts, &state)
        .await
        .unwrap();

    assert_eq!(session.0.id.0, session_id);
}

#[tokio::test]
async fn falls_back_to_a_bearer_token() {
    let (state, session_id) = state().await;
    let mut parts = parts(header::AUTHORIZATION, &format!("Bearer {TOKEN}"));

    let session = CurrentSession::from_request_parts(&mut parts, &state)
        .await
        .unwrap();

    assert_eq!(session.0.id.0, session_id);
}

#[tokio::test]
async fn rejects_an_unknown_token() {
    let (state, _) = state().await;
    let mut parts = parts(header::COOKIE, "session=wrong");

    assert!(
        CurrentSession::from_request_parts(&mut parts, &state)
            .await
            .is_err()
    );
}
