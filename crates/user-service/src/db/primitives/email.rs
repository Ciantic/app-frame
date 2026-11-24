use serde::Deserialize;
use serde::Serialize;

/// An email address as stored; sanitization and validation are separate steps, and `garde` validates the field.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate,
)]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Email(#[garde(email)] pub String);
