//! Database building blocks shared by the service crates.
//!
//! Repositories take `&mut PgConnection` as their first argument, so every
//! write can join a caller's transaction. This crate supplies the types those
//! signatures use: [`Patch`], [`Model`], [`Patchable`], [`RepoError`] and the
//! generic [`find`] and [`update`] operations.

mod error;
mod find;
mod model;
mod patch;
mod transaction;
mod update;

pub use dbkit_macros::DbModel;
pub use error::RepoError;
pub use error::RepoResult;
pub use find::find;
pub use model::Model;
pub use model::Patchable;
pub use patch::Patch;
pub use transaction::in_transaction;
pub use update::Update;
pub use update::update;
