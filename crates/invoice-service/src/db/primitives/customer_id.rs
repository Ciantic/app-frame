use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// The unique identifier of a customer.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    sqlx::Type,
    garde::Validate,
    DbType,
)]
#[db(pgtype = "uuid")]
#[db(pgdefault = "uuidv7()")]
#[sqlx(transparent)]
pub struct CustomerId(#[garde(skip)] pub Uuid);
