//! The narrow state contract the generated handlers need.

/// An application state that owns the database pool.
pub trait DbState: Clone + Send + Sync + 'static {
    fn pool(&self) -> &sqlx::PgPool;
}
