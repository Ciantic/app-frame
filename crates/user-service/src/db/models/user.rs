use crate::db::primitives::TimestampSchema;
use crate::db::primitives::email::Email;
use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbPatch;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;
use zod_rs::prelude::Schema;
use zod_rs::prelude::StringSchema;
use zod_rs::prelude::ZodSchema;
use zod_rs::prelude::string;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn schema() -> StringSchema {
        string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel, DbPatch, ZodSchema)]
#[db(table = "user")]
pub struct User {
    #[db(primary_key)]
    #[db(pgtype = "uuid")]
    #[db(pgdefault = "uuidv7()")]
    pub id: UserId,

    #[db(pgtype = "text")]
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
