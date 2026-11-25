use chrono::DateTime;
use chrono::Utc;
use dbkit::DbModel;
use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, DbType)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct EmailId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel)]
#[db(table = "email")]
pub struct Email {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: EmailId,

    #[db(pgtype = "text")]
    pub from: String,

    #[db(pgtype = "text")]
    pub to: String,

    #[db(pgtype = "text")]
    pub subject: String,

    #[db(pgtype = "text")]
    pub body: String,

    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<DateTime<Utc>>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    pub updated_at: DateTime<Utc>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}
