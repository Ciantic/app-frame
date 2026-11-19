use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;

/// Failure shared by every repository operation.
#[derive(Debug)]
pub enum RepoError {
    /// The requested row does not exist.
    NotFound,
    /// The row exists but the write contradicted a constraint or a version
    /// guard.
    Conflict,
    /// The caller's input cannot be applied to the row.
    InvalidInput(&'static str),
    Db(sqlx::Error),
}

pub type RepoResult<T> = Result<T, RepoError>;

impl From<sqlx::Error> for RepoError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                RepoError::Conflict
            }
            _ => RepoError::Db(error),
        }
    }
}

impl Display for RepoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoError::NotFound => formatter.write_str("not found"),
            RepoError::Conflict => formatter.write_str("conflict"),
            RepoError::InvalidInput(message) => write!(formatter, "invalid input: {message}"),
            RepoError::Db(error) => write!(formatter, "database error: {error}"),
        }
    }
}

impl Error for RepoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RepoError::Db(error) => Some(error),
            _ => None,
        }
    }
}
