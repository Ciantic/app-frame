//! JSON and query extractors whose rejections use the problem+json error shape.

use crate::ApiError;
use axum::extract::FromRequest;
use axum::extract::FromRequestParts;
use axum::extract::Query;
use axum::extract::Request;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(request, state)
            .await
            .map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;

        Ok(Self(value))
    }
}

pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|rejection| ApiError::bad_request(rejection.body_text()))?;

        Ok(Self(value))
    }
}
