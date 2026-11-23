use serde::Deserialize;
use serde::Serialize;

/// An email address as the database stores it.
///
/// The type carries no construction logic: sanitization and validation are
/// separate steps run before or after a value is built. `garde` validates the
/// field when the struct that owns it is validated.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, garde::Validate,
)]
#[serde(transparent)]
#[sqlx(transparent)]
#[garde(transparent)]
pub struct Email(#[garde(email)] pub String);
