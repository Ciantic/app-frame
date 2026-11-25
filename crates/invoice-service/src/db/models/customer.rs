use crate::db::primitives::e_invoice_address::EInvoiceAddress;
use crate::db::primitives::e_invoice_operator::EInvoiceOperator;
use crate::db::primitives::email_address::EmailAddress;
use crate::db::primitives::language::Language;
use chrono::DateTime;
use chrono::Utc;
use dbkit::DbDelete;
use dbkit::DbInsert;
use dbkit::DbModel;
use dbkit::DbPatch;
use dbkit::DbSelect;
use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;
use user_service::db::models::tenant::TenantId;
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
pub struct CustomerId(#[garde(skip)] pub Uuid);

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    sqlx::FromRow,
    DbModel,
    DbPatch,
    DbInsert,
    DbDelete,
    DbSelect,
)]
#[db(table = "customer")]
#[db(unique(columns = [tenant_id, id]))]
pub struct Customer {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: CustomerId,

    #[db(tenant)]
    #[db(query_ops(eq, any))]
    #[db(foreign_key(table = "tenant", column = "id"))]
    pub tenant_id: TenantId,

    #[db(pgtype = "text")]
    pub name: String,

    pub email: EmailAddress,

    #[db(pgtype = "text")]
    pub address: String,

    #[db(pgtype = "text")]
    pub business_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub e_invoice_address: Option<EInvoiceAddress>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub e_invoice_operator: Option<EInvoiceOperator>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,

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
