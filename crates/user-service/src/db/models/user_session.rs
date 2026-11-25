use super::tenant::TenantId;
use super::user::UserId;
use crate::db::primitives::session_token::SessionToken;
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
pub struct UserSessionId(#[garde(skip)] pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel, DbPatch)]
#[db(table = "user_session")]
pub struct UserSession {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: UserSessionId,

    pub user_id: UserId,

    #[db(pgtype = "text")]
    pub token_hash: SessionToken,

    #[db(pgtype = "timestamptz")]
    pub expires_at: DateTime<Utc>,

    #[db(pgtype = "timestamptz")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,

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

impl UserSession {
    pub fn has_global_permissions(&self, scope: &str, action: &str) -> bool {
        // Out of scope for now, but can be used to check global permissions
        true
    }

    pub fn has_tenant_permissions(&self, tenant_id: TenantId, scope: &str, action: &str) -> bool {
        // Out of scope for now, but can be used to check tenant-specific permissions
        true
    }
}
