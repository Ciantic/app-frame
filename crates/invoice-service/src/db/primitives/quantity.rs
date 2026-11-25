use dbkit::DbType;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

/// A number of billed units, stored as `numeric(12,3)`.
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
#[db(pgtype = "numeric(12,3)")]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct Quantity(#[garde(skip)] pub Decimal);
