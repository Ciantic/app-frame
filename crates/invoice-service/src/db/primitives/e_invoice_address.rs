use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// A Finnish e-invoice address (verkkolaskuosoite).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate, DbType,
)]
#[db(pgtype = "text")]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct EInvoiceAddress(#[garde(length(min = 1))] pub String);
