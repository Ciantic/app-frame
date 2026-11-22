use super::user::UserId;
use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use facet::Facet;
use garde::Validate;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct UserSessionId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, Facet, DbModel, Validate)]
#[db(table = "user_session")]
pub struct UserSession {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(pgdefault = "uuidv7()")]
    #[garde(skip)]
    pub id: UserSessionId,

    #[db(pgtype = "uuid")]
    #[garde(skip)]
    pub user_id: UserId,

    #[db(pgtype = "text")]
    #[garde(length(min = 1))]
    pub token_hash: String,

    #[db(pgtype = "timestamptz")]
    #[garde(skip)]
    pub expires_at: DateTime<Utc>,

    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub revoked_at: Option<DateTime<Utc>>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    #[garde(skip)]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    #[garde(skip)]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    #[garde(skip)]
    pub version: i64,
}
