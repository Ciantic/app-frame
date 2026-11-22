use facet::Facet;
use serde::Deserialize;
use serde::Serialize;
use zod_rs::prelude::StringSchema;
use zod_rs::prelude::string;

/// A session or magic link token, stored only as its hash.
///
/// The type carries no construction logic: sanitization and validation are
/// separate steps run before or after a value is built. `zod-rs` validates the
/// field when the struct that owns it is validated.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type, Facet)]
#[serde(transparent)]
#[facet(transparent)]
#[sqlx(transparent)]
pub struct SessionToken(pub String);

impl SessionToken {
    pub fn schema() -> StringSchema {
        string().min(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zod_rs::prelude::Schema;

    #[test]
    fn schema_rejects_empty() {
        assert!(SessionToken::schema().validate(&json!("")).is_err());
    }

    #[test]
    fn schema_accepts_non_empty() {
        assert!(SessionToken::schema().validate(&json!("hash")).is_ok());
    }
}
