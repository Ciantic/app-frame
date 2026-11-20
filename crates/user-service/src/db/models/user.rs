use chrono::DateTime;
use chrono::Utc;
use facet::Facet;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Facet)]
pub struct User {
    pub id: UserId,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub version: i64,
}
