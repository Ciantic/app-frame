use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// A unit of measure such as `h` or `pcs`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate, DbType,
)]
#[db(pgtype = "text")]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Unit(#[garde(length(min = 1))] pub String);
