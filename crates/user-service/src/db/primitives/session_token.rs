use serde::Deserialize;
use serde::Serialize;

/// A session or magic link token, stored only as its hash.
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
pub struct SessionToken(#[garde(length(min = 1))] pub String);

#[cfg(test)]
mod tests {
    use super::*;
    use garde::Validate;

    #[test]
    fn rejects_empty() {
        assert!(SessionToken(String::new()).validate().is_err());
    }

    #[test]
    fn accepts_non_empty() {
        assert!(SessionToken("hash".to_owned()).validate().is_ok());
    }
}
