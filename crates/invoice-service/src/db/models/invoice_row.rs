use super::invoice::InvoiceId;
use crate::db::primitives::money::Money;
use crate::db::primitives::quantity::Quantity;
use crate::db::primitives::tax_rate::TaxRate;
use crate::db::primitives::unit::Unit;
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
pub struct InvoiceRowId(#[garde(skip)] pub Uuid);

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
#[db(table = "invoice_row")]
#[db(pg_trigger = "invoice_row_amounts")]
#[db(pg_trigger = "invoice_row_totals")]
pub struct InvoiceRow {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: InvoiceRowId,

    #[db(tenant)]
    #[db(query_ops(eq, any))]
    pub tenant_id: TenantId,

    #[db(query_ops(eq, any))]
    #[db(foreign_key(table = "invoice", column = "id", scope = "tenant_id"))]
    pub invoice_id: InvoiceId,

    #[db(pgtype = "text")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<Quantity>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_price: Option<Money>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_rate: Option<TaxRate>,

    #[db(pgdefault = inherit)]
    pub net_amount: Money,

    #[db(pgdefault = inherit)]
    pub tax_amount: Money,

    #[db(pgdefault = inherit)]
    pub total_amount: Money,

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
