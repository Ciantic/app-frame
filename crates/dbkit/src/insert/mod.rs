//! Insert inputs: the builder that inserts rows and returns their primary keys.

mod query;

pub use query::InsertError;
pub use query::InsertQuery;
pub use query::InsertRow;
pub use query::RenderedInsert;
