use crate::Model;
use crate::Patchable;
use crate::RepoError;
use crate::RepoResult;
use crate::find::find;
use chrono::DateTime;
use chrono::Utc;
use sqlx::Encode;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use sqlx::Type;
use sqlx::postgres::PgArguments;
use sqlx::postgres::PgConnection;
use sqlx::query::QueryAs;

/// Builds the `SET` list of a single-row `UPDATE`.
pub struct Update {
    builder: QueryBuilder<Postgres>,
    assignments: usize,
}

impl Update {
    pub fn new<M: Model>() -> Self {
        let mut builder: QueryBuilder<Postgres> = QueryBuilder::new("UPDATE ");
        builder.push('"').push(M::TABLE).push("\" SET ");

        Self {
            builder,
            assignments: 0,
        }
    }

    /// Appends `"column" = $n`, binding `value`.
    pub fn set<T>(&mut self, column: &'static str, value: T) -> &mut Self
    where
        T: Encode<'static, Postgres> + Type<Postgres>,
    {
        self.separate();
        self.builder
            .push('"')
            .push(column)
            .push("\" = ")
            .push_bind(value);
        self
    }

    /// Appends the managed columns, the version guard and `RETURNING *`.
    pub fn finish<M: Model>(
        &mut self,
        now: DateTime<Utc>,
        id: M::Id,
        version: i64,
    ) -> QueryAs<'_, Postgres, M, PgArguments> {
        if let Some(column) = M::UPDATED_AT {
            self.set(column, now);
        }

        self.separate();
        self.builder
            .push('"')
            .push(M::VERSION)
            .push("\" = \"")
            .push(M::VERSION)
            .push("\" + 1");

        self.builder
            .push(" WHERE \"")
            .push(M::ID)
            .push("\" = ")
            .push_bind(id);
        self.builder
            .push(" AND \"")
            .push(M::VERSION)
            .push("\" = ")
            .push_bind(version);
        self.builder.push(" RETURNING *");

        self.builder.build_query_as::<M>()
    }

    fn separate(&mut self) {
        if self.assignments > 0 {
            self.builder.push(", ");
        }

        self.assignments += 1;
    }
}

/// Applies a patch with an optimistic-locking guard on `version`.
///
/// `now` is written to the updated-at column when the model has one. A row
/// updated by someone else in the meantime reports
/// [`RepoError::Conflict`](crate::RepoError::Conflict), a missing row reports
/// [`RepoError::NotFound`](crate::RepoError::NotFound).
pub async fn update<M: Model>(
    conn: &mut PgConnection,
    now: DateTime<Utc>,
    id: M::Id,
    version: i64,
    patch: &M::Patch,
) -> RepoResult<M> {
    if patch.is_empty() {
        return Err(RepoError::InvalidInput("patch has no fields"));
    }

    let mut update = Update::new::<M>();
    patch.bind(&mut update)?;

    match update
        .finish::<M>(now, id, version)
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(model)) => return Ok(model),
        Ok(None) => {}
        Err(error) => return Err(RepoError::from(error)),
    }

    // The guarded update matched nothing: tell a stale version from a missing row.
    match find::<M>(conn, id).await? {
        Some(_) => Err(RepoError::Conflict),
        None => Err(RepoError::NotFound),
    }
}
