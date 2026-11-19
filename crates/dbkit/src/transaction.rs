use crate::RepoError;
use crate::RepoResult;
use sqlx::PgPool;
use sqlx::postgres::PgConnection;
use std::time::Duration;

const MAX_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(50);

/// Runs `body` in a transaction, retrying it on a serialization failure or a
/// deadlock.
///
/// The retry wraps the whole transaction, so `body` may run more than once and
/// must not have side effects outside the database.
pub async fn in_transaction<T, F>(pool: &PgPool, mut body: F) -> RepoResult<T>
where
    F: AsyncFnMut(&mut PgConnection) -> RepoResult<T>,
{
    let mut attempt = 0;

    loop {
        attempt += 1;

        let mut transaction = pool.begin().await?;

        match body(&mut transaction).await {
            Ok(value) => {
                transaction.commit().await?;
                return Ok(value);
            }
            Err(error) => {
                drop(transaction);

                if !retryable(&error) || attempt >= MAX_ATTEMPTS {
                    return Err(error);
                }

                tokio::time::sleep(RETRY_DELAY * attempt).await;
            }
        }
    }
}

fn retryable(error: &RepoError) -> bool {
    let RepoError::Db(sqlx::Error::Database(database)) = error else {
        return false;
    };

    matches!(database.code().as_deref(), Some("40001" | "40P01"))
}
