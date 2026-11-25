use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;

/// A language tag the invoice can be rendered in, such as `en` or `fi`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate, DbType,
)]
#[db(pgtype = "text")]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Language(#[garde(length(min = 1))] pub String);
