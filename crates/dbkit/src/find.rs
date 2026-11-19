use crate::Model;
use crate::RepoError;
use crate::RepoResult;
use sqlx::AssertSqlSafe;
use sqlx::Postgres;
use sqlx::postgres::PgConnection;

/// Loads one row by primary key.
pub async fn find<M: Model>(conn: &mut PgConnection, id: M::Id) -> RepoResult<Option<M>> {
    let sql = format!("SELECT * FROM \"{}\" WHERE \"{}\" = $1", M::TABLE, M::ID);

    sqlx::query_as::<Postgres, M>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(RepoError::from)
}
