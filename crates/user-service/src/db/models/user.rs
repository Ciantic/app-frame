use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use facet::Facet;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Facet, DbModel)]
#[db(table = "user")]
pub struct User {
    #[db(primary_key, pgtype = "uuid")]
    pub id: UserId,
    #[db(pgtype = "text")]
    pub email: String,
    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified_at: Option<DateTime<Utc>>,
    #[db(created_at, pgtype = "timestamptz")]
    pub created_at: DateTime<Utc>,
    #[db(updated_at, pgtype = "timestamptz")]
    pub updated_at: DateTime<Utc>,
    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[db(version, pgtype = "bigint")]
    pub version: i64,
}
