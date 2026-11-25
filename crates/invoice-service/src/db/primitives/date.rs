use chrono::NaiveDate;
use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// A calendar date without a time or zone, stored as `date`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type,
    garde::Validate,
    DbType,
)]
#[db(pgtype = "date")]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct Date(#[garde(skip)] pub NaiveDate);
