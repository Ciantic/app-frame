use dbkit::DbType;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

/// An amount of money in the invoice currency, stored as `numeric(14,2)`.
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
#[db(pgtype = "numeric(14,2)")]
#[db(pgdefault = "0")]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct Money(#[garde(skip)] pub Decimal);
