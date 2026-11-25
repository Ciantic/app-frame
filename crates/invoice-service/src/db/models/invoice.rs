use super::customer::Customer;
use super::invoice_row::InvoiceRow;
use crate::db::primitives::customer_id::CustomerId;
use crate::db::primitives::date::Date;
use crate::db::primitives::language::Language;
use crate::db::primitives::money::Money;
use crate::db::primitives::seller_id::SellerId;
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
pub struct InvoiceId(#[garde(skip)] pub Uuid);

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
#[db(table = "invoice")]
#[db(pg_trigger = "invoice_totals")]
#[db(unique(columns = [tenant_id, id]))]
pub struct Invoice {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: InvoiceId,

    #[db(tenant)]
    #[db(query_ops(eq, any))]
    pub tenant_id: TenantId,

    #[db(pgtype = "text")]
    #[db(unique)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,

    #[db(query_ops(eq, any))]
    #[db(foreign_key(table = "customer", column = "id", scope = "tenant_id"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<CustomerId>,

    #[db(relation(
        one,
        table = "customer",
        local_key = [tenant_id, customer_id],
        target_key = [tenant_id, id]
    ))]
    #[sqlx(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer: Option<Customer>,

    #[db(query_ops(eq, any))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_id: Option<SellerId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,

    #[db(query_ops(gte, lte))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_date: Option<Date>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<Date>,

    #[db(pgdefault = inherit)]
    pub net_amount: Money,

    #[db(pgdefault = inherit)]
    pub tax_amount: Money,

    #[db(pg_virtual = "\"net_amount\" + \"tax_amount\"")]
    pub total_amount: Money,

    #[db(relation(
        many,
        table = "invoice_row",
        local_key = [tenant_id, id],
        target_key = [tenant_id, invoice_id]
    ))]
    #[sqlx(skip)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<InvoiceRow>,

    #[db(pgtype = "text")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    #[db(created_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    #[db(orderable)]
    pub created_at: DateTime<Utc>,

    #[db(updated_at)]
    #[db(pgtype = "timestamptz")]
    #[db(pgdefault = "now()")]
    #[db(orderable)]
    pub updated_at: DateTime<Utc>,

    #[db(pgtype = "timestamptz")]
    #[db(query_ops(eq, neq))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,

    #[db(version)]
    #[db(pgtype = "bigint")]
    #[db(pgdefault = "0")]
    pub version: i64,
}
