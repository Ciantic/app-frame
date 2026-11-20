use chrono::DateTime;
use chrono::Utc;
use facet::Facet;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct EmailId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Facet)]
pub struct Email {
    pub id: EmailId,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}
