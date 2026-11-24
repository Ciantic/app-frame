//! Database building blocks shared by the service crates.
//!
//! [`Patch`] is the update input that tells "not given" apart from "set to
//! `NULL`"; `#[derive(DbPatch)]` generates the patch of a model and validates
//! it with `garde`. [`Model`] and its [`TableMeta`] describe a table at
//! runtime, [`SubQuery`] describes a model whose row source is a query, and
//! [`schema::check_live_schema_against_models`] compares a list of tables with
//! a live database. [`DbType`] records the column type and default of
//! a value type, so a model field of that type needs no `pgtype` of its own.
//! [`Column`] and [`Columns`] name the selectable columns of a model;
//! `#[derive(DbModel)]` generates the enum and [`ErasedColumn`] is the erased
//! view a [`TableMeta`] holds. [`Query`] is the filter of one table, built from
//! the typed [`ColumnRef`] handles or parsed from JSON, and [`Query::build`]
//! renders it to the statement `sqlx` executes, decoding into the model, while
//! [`Query::build_selected`] decodes into the model's select type so a sparse
//! projection can report its absent columns.

mod columns;
mod model;
mod patch;
pub mod query;
pub mod schema;
mod select;

pub use columns::Column;
pub use columns::Columns;
pub use dbkit_macros::DbModel;
pub use dbkit_macros::DbPatch;
pub use dbkit_macros::DbSelect;
pub use dbkit_macros::DbType;
pub use dbkit_macros::DbValue;
pub use model::DbType;
pub use model::ErasedColumn;
pub use model::FilterOperator;
pub use model::Meta;
pub use model::Model;
pub use model::Selectable;
pub use model::SubQuery;
pub use model::TableFragment;
pub use model::TableMeta;
pub use patch::Patch;
pub use query::Bind;
pub use query::ColumnRef;
pub use query::Combinator;
pub use query::Filter;
pub use query::Group;
pub use query::IntoColumns;
pub use query::Leaf;
pub use query::Operand;
pub use query::Query;
pub use query::Rendered;
pub use query::SelectedQuery;
pub use query::capability::*;
pub use select::SelectRow;
pub use select::Selected;
pub use select::selected;
