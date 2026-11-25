//! The HTTP error type: one response shape and the dbkit/sqlx mappings.

use axum::Json;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use serde::Serialize;
use sqlx::Error as SqlxError;
use std::error::Error as StdError;
use std::fmt;

pub type ApiResult<T> = Result<T, ApiError>;

/// Everything a handler can reject, rendered as `application/problem+json`.
#[derive(Debug)]
pub enum ApiError {
    BadRequest {
        detail: String,
    },
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict {
        detail: String,
    },
    Unprocessable {
        detail: String,
    },
    Internal {
        source: Box<dyn StdError + Send + Sync>,
    },
}

#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl ApiError {
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::BadRequest {
            detail: detail.into(),
        }
    }

    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::Conflict {
            detail: detail.into(),
        }
    }

    pub fn unprocessable(detail: impl Into<String>) -> Self {
        Self::Unprocessable {
            detail: detail.into(),
        }
    }

    pub fn internal(source: impl StdError + Send + Sync + 'static) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }

    fn problem(&self) -> (StatusCode, Problem) {
        match self {
            Self::BadRequest { detail } => (
                StatusCode::BAD_REQUEST,
                Problem {
                    kind: "about:blank",
                    title: "Bad Request",
                    status: 400,
                    detail: Some(detail.clone()),
                },
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Problem {
                    kind: "about:blank",
                    title: "Unauthorized",
                    status: 401,
                    detail: None,
                },
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                Problem {
                    kind: "about:blank",
                    title: "Forbidden",
                    status: 403,
                    detail: None,
                },
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Problem {
                    kind: "about:blank",
                    title: "Not Found",
                    status: 404,
                    detail: None,
                },
            ),
            Self::Conflict { detail } => (
                StatusCode::CONFLICT,
                Problem {
                    kind: "about:blank",
                    title: "Conflict",
                    status: 409,
                    detail: Some(detail.clone()),
                },
            ),
            Self::Unprocessable { detail } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Problem {
                    kind: "about:blank",
                    title: "Unprocessable Entity",
                    status: 422,
                    detail: Some(detail.clone()),
                },
            ),
            Self::Internal { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Problem {
                    kind: "about:blank",
                    title: "Internal Server Error",
                    status: 500,
                    detail: None,
                },
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Internal { source } = &self {
            tracing::error!(error = %source, "httpkit internal error");
        }

        let (status, problem) = self.problem();

        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(problem),
        )
            .into_response()
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest { detail }
            | Self::Conflict { detail }
            | Self::Unprocessable { detail } => formatter.write_str(detail),
            Self::Unauthorized => formatter.write_str("unauthorized"),
            Self::Forbidden => formatter.write_str("forbidden"),
            Self::NotFound => formatter.write_str("not found"),
            Self::Internal { source } => source.fmt(formatter),
        }
    }
}

impl StdError for ApiError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Internal { source } => Some(&**source),
            _ => None,
        }
    }
}

impl From<dbkit::query::Error> for ApiError {
    fn from(error: dbkit::query::Error) -> Self {
        Self::bad_request(error.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::bad_request(error.to_string())
    }
}

impl From<garde::Report> for ApiError {
    fn from(report: garde::Report) -> Self {
        Self::unprocessable(report.to_string())
    }
}

impl From<dbkit::InsertError> for ApiError {
    fn from(error: dbkit::InsertError) -> Self {
        match error {
            dbkit::InsertError::NotATable => Self::internal(error),
            dbkit::InsertError::NotInserted { .. } => {
                Self::conflict("an insert returned no primary key")
            }
            dbkit::InsertError::Sqlx(error) => Self::from(error),
        }
    }
}

impl From<dbkit::PatchError> for ApiError {
    fn from(error: dbkit::PatchError) -> Self {
        match error {
            dbkit::PatchError::Empty { .. } => Self::bad_request("a patch changes no column"),
            dbkit::PatchError::NotUpdated { .. } => Self::conflict(
                "a patch did not update exactly one row; the row is missing or its version changed",
            ),
            dbkit::PatchError::KeyColumns { .. }
            | dbkit::PatchError::VersionColumns
            | dbkit::PatchError::TenantColumns
            | dbkit::PatchError::NotATable => Self::internal(error),
            dbkit::PatchError::Sqlx(error) => Self::from(error),
        }
    }
}

impl From<dbkit::DeleteError> for ApiError {
    fn from(error: dbkit::DeleteError) -> Self {
        match error {
            dbkit::DeleteError::NotDeleted { .. } => Self::conflict(
                "a delete did not remove exactly one row; the row is missing or its version \
                 changed",
            ),
            dbkit::DeleteError::KeyColumns { .. }
            | dbkit::DeleteError::VersionColumns
            | dbkit::DeleteError::TenantColumns
            | dbkit::DeleteError::NotATable => Self::internal(error),
            dbkit::DeleteError::Sqlx(error) => Self::from(error),
        }
    }
}

impl From<SqlxError> for ApiError {
    fn from(error: SqlxError) -> Self {
        match &error {
            SqlxError::Database(database) => match database.code().as_deref() {
                Some("23505") => Self::conflict("a unique constraint rejected the write"),
                Some("23503") => Self::conflict("a foreign key constraint rejected the write"),
                Some("23502") => Self::unprocessable("a not-null constraint rejected the write"),
                Some("23514") => Self::unprocessable("a check constraint rejected the write"),
                _ => Self::internal(error),
            },
            _ => Self::internal(error),
        }
    }
}
