use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// An operator that routes a customer's e-invoices.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate, DbType,
)]
#[db(pgtype = "text")]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct EInvoiceOperator(#[garde(length(min = 1))] pub String);
