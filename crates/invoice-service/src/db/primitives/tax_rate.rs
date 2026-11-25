use dbkit::DbType;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

/// A tax rate as a fraction, stored as `numeric(5,4)`; `0.255` is 25.5%.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    sqlx::Type,
    garde::Validate,
    DbType,
)]
#[db(pgtype = "numeric(5,4)")]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct TaxRate(#[garde(skip)] pub Decimal);
