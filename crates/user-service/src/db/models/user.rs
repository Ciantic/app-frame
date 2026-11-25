use crate::db::primitives::email::Email;
use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type,
    garde::Validate,
    DbType,
)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct UserId(#[garde(skip)] pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel, DbPatch)]
#[db(table = "user")]
pub struct User {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: UserId,

    #[db(pgtype = "text")]
    #[db(unique)]
    pub email: Email,

    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified_at: Option<DateTime<Utc>>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub updated_at: DateTime<Utc>,

    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}
