use crate::db::models::tenant::TenantId;
use crate::db::models::user_session::UserSession;
use crate::db::primitives::session_token::SessionToken;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use chrono::Utc;
use httpkit::DbState;
use sha2::Digest;
use sha2::Sha256;

/// The cookie that carries the raw session token; a bearer token is the fallback.
pub const SESSION_COOKIE: &str = "session";

pub struct CurrentSession(pub UserSession);

impl FromRequestParts<AppState> for CurrentSession {
    type Rejection = httpkit::ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = session_token(parts).ok_or(httpkit::ApiError::Unauthorized)?;
        let hash = hex::encode(Sha256::digest(token.as_bytes()));

        let session = UserSession::query()
            .filters([UserSession::columns().token_hash.eq(SessionToken(hash))])
            .limit(1)
            .build()?
            .fetch_optional(state.pool())
            .await
            .map_err(httpkit::ApiError::from)?;

        match session {
            Some(session) if session.revoked_at.is_none() && session.expires_at > Utc::now() => {
                Ok(Self(session))
            }
            _ => Err(httpkit::ApiError::Unauthorized),
        }
    }
}

impl httpkit::Session for CurrentSession {
    type TenantId = TenantId;

    fn has_permission(&self, permission: &httpkit::Permission<TenantId>) -> bool {
        self.0.has_permission(permission)
    }
}

fn session_token(parts: &Parts) -> Option<&str> {
    if let Some(cookie) = parts
        .headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        for part in cookie.split(';') {
            let mut pair = part.trim().splitn(2, '=');
            let Some(name) = pair.next() else {
                continue;
            };
            let Some(value) = pair.next() else {
                continue;
            };

            if name == SESSION_COOKIE && !value.is_empty() {
                return Some(value);
            }
        }
    }

    let value = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?;

    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
