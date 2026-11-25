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
#[db(index(columns = [invoice_id]))]
#[db(pg_trigger(
    name = "invoice_row_totals",
    timing = after,
    events(insert, update, delete),
    body = r#"begin
    if tg_op = 'DELETE' then
        update "invoice" set "updated_at" = now(), "version" = "version" + 1
        where "id" = old."invoice_id" and "tenant_id" = old."tenant_id";
        return old;
    end if;

    update "invoice" set "updated_at" = now(), "version" = "version" + 1
    where "id" = new."invoice_id" and "tenant_id" = new."tenant_id";

    if tg_op = 'UPDATE'
        and (old."invoice_id", old."tenant_id") is distinct from (new."invoice_id", new."tenant_id")
    then
        update "invoice" set "updated_at" = now(), "version" = "version" + 1
        where "id" = old."invoice_id" and "tenant_id" = old."tenant_id";
    end if;

    return new;
end;"#
))]
pub struct InvoiceRow {
    #[db(primary_key)]
    #[db(pgdefault = inherit)]
    #[db(query_ops(eq, any))]
    pub id: InvoiceRowId,

    #[db(tenant)]
    #[db(query_ops(eq, any))]
    pub tenant_id: TenantId,

    #[db(query_ops(eq, any))]
    #[db(foreign_key(
        table = "invoice",
        column = "id",
        scope = "tenant_id",
        on_delete = cascade
    ))]
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
    #[db(pg_trigger(
        name = "invoice_row_amounts",
        timing = before,
        events(insert, update),
        body = r#"begin
    new."net_amount" := round(coalesce(new."quantity", 0) * coalesce(new."unit_price", 0), 2);
    new."tax_amount" := round(new."net_amount" * coalesce(new."tax_rate", 0), 2);
    new."total_amount" := new."net_amount" + new."tax_amount";
    return new;
end;"#
    ))]
    pub net_amount: Money,

    #[db(pgdefault = inherit)]
    #[db(pg_trigger = "invoice_row_amounts")]
    pub tax_amount: Money,

    #[db(pgdefault = inherit)]
    #[db(pg_trigger = "invoice_row_amounts")]
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
