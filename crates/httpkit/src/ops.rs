//! The generic query, insert, patch and delete operations behind the generated handlers.

use crate::ApiError;
use crate::ApiResult;
use crate::DbState;
use axum::Json;
use axum::http::StatusCode;
use dbkit::Column;
use dbkit::DeleteKey;
use dbkit::InsertRow;
use dbkit::PatchRow;
use dbkit::Query;
use garde::Validate;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::FromRow;
use sqlx::postgres::PgRow;

/// The largest page a query endpoint returns when the request names no `limit`.
pub const DEFAULT_LIMIT: u32 = 1000;

/// The one URL parameter a query endpoint accepts.
#[derive(Debug, serde::Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
}

/// Parses the query payload, rejects projections and applies the default page size.
pub fn parse_query<C: Column>(q: Option<&str>) -> ApiResult<Query<C>> {
    let mut query = Query::<C>::parse(q.unwrap_or("{}"))?;

    if query.select.is_some() {
        return Err(ApiError::bad_request(
            "`select` is not supported by this endpoint",
        ));
    }

    if query.limit.is_none() {
        query.limit = Some(DEFAULT_LIMIT);
    }

    Ok(query)
}

pub async fn query<S, C>(state: &S, query: Query<C>) -> ApiResult<Json<Vec<C::Model>>>
where
    S: DbState,
    C: Column,
    C::Model: Serialize + Send + Unpin + for<'r> FromRow<'r, PgRow>,
{
    let rows = query.build()?.fetch_all(state.pool()).await?;

    Ok(Json(rows))
}

pub async fn insert<S, P>(
    state: &S,
    entries: Vec<P>,
) -> ApiResult<(StatusCode, Json<Vec<<P as InsertRow>::Key>>)>
where
    S: DbState,
    P: InsertRow + DeserializeOwned + Validate,
    P::Key: Serialize,
    <P as Validate>::Context: Default,
{
    if entries.is_empty() {
        return Err(ApiError::bad_request(
            "an insert batch needs at least one entry",
        ));
    }

    for entry in &entries {
        entry.validate()?;
    }

    let keys = dbkit::InsertQuery::new(entries)
        .execute(state.pool())
        .await?;

    Ok((StatusCode::CREATED, Json(keys)))
}

pub async fn patch<S, P>(state: &S, entries: Vec<P>) -> ApiResult<StatusCode>
where
    S: DbState,
    P: PatchRow + DeserializeOwned + Validate,
    <P as Validate>::Context: Default,
{
    if entries.is_empty() {
        return Err(ApiError::bad_request(
            "a patch batch needs at least one entry",
        ));
    }

    for entry in &entries {
        entry.validate()?;
    }

    dbkit::PatchQuery::new(entries)
        .execute(state.pool())
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete<S, K>(state: &S, keys: Vec<K>) -> ApiResult<StatusCode>
where
    S: DbState,
    K: DeleteKey + DeserializeOwned + Serialize,
{
    if keys.is_empty() {
        return Err(ApiError::bad_request(
            "a delete batch needs at least one key",
        ));
    }

    dbkit::DeleteQuery::new(keys).execute(state.pool()).await?;

    Ok(StatusCode::NO_CONTENT)
}
