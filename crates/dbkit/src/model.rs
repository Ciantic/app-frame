use crate::RepoResult;
use crate::Update;
use sqlx::Encode;
use sqlx::FromRow;
use sqlx::Postgres;
use sqlx::Type;
use sqlx::postgres::PgRow;

/// A table row that [`find`](crate::find) and [`update`](crate::update) can
/// operate on.
///
/// `#[derive(DbModel)]` will generate this implementation once code generation
/// lands; for now it is written by hand.
pub trait Model: Sized + Send + Unpin + for<'r> FromRow<'r, PgRow> {
    type Id: Encode<'static, Postgres> + Type<Postgres> + Copy + Send + 'static;
    type Patch: Patchable<Self>;

    const TABLE: &'static str;
    const ID: &'static str;
    const COLUMNS: &'static [&'static str];

    const CREATED_AT: Option<&'static str>;
    const UPDATED_AT: Option<&'static str>;
    const VERSION: &'static str;

    /// The optimistic-locking token of this row.
    fn version(&self) -> i64;
}

/// The update input of a model: one [`Patch`](crate::Patch) per patchable
/// field.
pub trait Patchable<M> {
    /// Whether the patch changes nothing and must be rejected.
    fn is_empty(&self) -> bool;

    /// Appends the present fields to the `SET` list.
    ///
    /// Returns [`RepoError::InvalidInput`](crate::RepoError::InvalidInput) when
    /// a `NOT NULL` column is patched to `NULL`.
    fn bind(&self, update: &mut Update) -> RepoResult<()>;
}
