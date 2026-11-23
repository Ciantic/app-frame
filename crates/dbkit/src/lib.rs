//! Database building blocks shared by the service crates.
//!
//! [`Patch`] is the update input that tells "not given" apart from "set to
//! `NULL`". [`Model`] and its [`TableMeta`] describe a table at runtime, and
//! [`schema::check_live_schema_against_models`] compares a list of them with a
//! live database.

mod model;
mod patch;
pub mod schema;

pub use dbkit_macros::DbModel;
pub use model::ColumnMeta;
pub use model::Model;
pub use model::TableMeta;
pub use patch::Patch;
