use serde::Deserialize;
use serde::Serialize;

/// A session or magic-link token, stored only as its hash; `garde` validates the field where it is used.
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
