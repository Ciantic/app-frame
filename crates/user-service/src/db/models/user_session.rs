use super::user::UserId;
use chrono::DateTime;
use chrono::Utc;
use facet::Facet;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct UserSessionId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Facet)]
pub struct UserSession {
    pub id: UserSessionId,
    pub user_id: UserId,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}
