use facet::Facet;
use garde::Validate;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// An email address as the database stores it.
///
/// The type carries no construction logic: sanitization and validation are
/// separate steps run before or after a value is built. `garde` validates the
/// field when the struct that owns it is validated.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet, Validate
)]
#[garde(transparent)]
#[serde(transparent)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct Email(#[garde(email)] pub String);
