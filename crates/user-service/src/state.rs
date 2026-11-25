use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

impl AppState {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl httpkit::DbState for AppState {
    fn pool(&self) -> &PgPool {
        &self.db
    }
}
