use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// An email address as stored; `garde` validates the field where it is used.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate, DbType,
)]
#[db(pgtype = "text")]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct EmailAddress(#[garde(email)] pub String);
