//! Delete inputs: the builder that deletes rows by key and checks each version.

mod query;

pub use query::DeleteError;
pub use query::DeleteKey;
pub use query::DeleteQuery;
pub use query::RenderedDelete;
