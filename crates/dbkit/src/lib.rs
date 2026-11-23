//! Database building blocks shared by the service crates.
//!
//! [`Patch`] is the update input that tells "not given" apart from "set to
//! `NULL`"; `#[derive(DbPatch)]` generates the patch of a model and validates
//! it with `garde`. [`Model`] and its [`TableMeta`] describe a table at
//! runtime, and [`schema::check_live_schema_against_models`] compares a list of
//! them with a live database. [`DbType`] records the column type and default of
//! a value type, so a model field of that type needs no `pgtype` of its own.

mod model;
mod patch;
pub mod schema;

pub use dbkit_macros::DbModel;
pub use dbkit_macros::DbPatch;
pub use dbkit_macros::DbType;
pub use model::ColumnMeta;
pub use model::DbType;
pub use model::FilterOperator;
pub use model::Meta;
pub use model::Model;
pub use model::TableFragment;
pub use model::TableMeta;
pub use patch::Patch;
