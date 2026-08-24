use crate::domain::error::{Error, RepositoryErrorType};

/// Translates driver failures into domain errors so callers never match on
/// `sqlx::Error` outside this layer.
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::Database(e) => match e.code().unwrap_or_default().as_ref() {
                // unique_violation
                "23505" => Error::Repository(RepositoryErrorType::Conflict),
                // foreign_key_violation
                "23503" => Error::Repository(RepositoryErrorType::Conflict),
                _ => Error::External(e.into()),
            },
            sqlx::Error::RowNotFound => Error::Repository(RepositoryErrorType::NotFound),
            _ => Error::External(err.into()),
        }
    }
}
