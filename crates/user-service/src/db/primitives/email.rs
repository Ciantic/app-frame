use serde::Deserialize;
use serde::Serialize;
use zod_rs::prelude::StringSchema;
use zod_rs::prelude::string;

/// An email address as the database stores it.
///
/// The type carries no construction logic: sanitization and validation are
/// separate steps run before or after a value is built. `zod-rs` validates the
/// field when the struct that owns it is validated.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct Email(pub String);

impl Email {
    pub fn schema() -> StringSchema {
        string().email()
    }
}
