use dbkit::DbModel;
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
pub struct TenantId(#[garde(skip)] pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, DbModel)]
#[db(table = "tenant")]
pub struct Tenant {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    pub id: TenantId,

    #[db(pgtype = "text")]
    pub name: String,
}
