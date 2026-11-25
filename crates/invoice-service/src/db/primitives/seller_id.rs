use dbkit::DbType;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

/// The identifier of a seller owned by another service.
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
#[sqlx(transparent)]
pub struct SellerId(#[garde(skip)] pub Uuid);
