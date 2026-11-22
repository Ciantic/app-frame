use facet::Facet;
use garde::Validate;
use serde::Deserialize;
use serde::Serialize;

/// A session or magic link token, stored only as its hash.
///
/// The type carries no construction logic: sanitization and validation are
/// separate steps run before or after a value is built. `garde` validates the
/// field when the struct that owns it is validated.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet, Validate,
)]
#[garde(transparent)]
#[serde(transparent)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct SessionToken(#[garde(length(min = 1))] pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garde_rejects_empty() {
        assert!(SessionToken(String::new()).validate().is_err());
    }

    #[test]
    fn garde_accepts_non_empty() {
        assert!(SessionToken("hash".to_owned()).validate().is_ok());
    }
}
