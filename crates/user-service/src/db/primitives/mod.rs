pub mod email;
pub mod session_token;

use chrono::DateTime;
use chrono::Utc;
use zod_rs::prelude::StringSchema;
use zod_rs::prelude::string;

/// Gives zod-rs a schema for `chrono` timestamps, which serde serializes as
/// strings.
pub trait TimestampSchema {
    fn schema() -> StringSchema;
}

impl TimestampSchema for DateTime<Utc> {
    fn schema() -> StringSchema {
        string()
    }
}
