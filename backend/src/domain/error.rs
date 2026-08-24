use tracing::{debug, error};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("repository error: {0}")]
    Repository(RepositoryErrorType),
    #[error("validation error: {0}")]
    Validation(anyhow::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error(transparent)]
    External(#[from] anyhow::Error),
}

impl Error {
    pub fn validation(message: impl Into<String>) -> Self {
        Error::Validation(anyhow::anyhow!(message.into()))
    }

    /// Constructor for the layers above. The PostgreSQL implementation reaches
    /// this variant through the `sqlx::Error` conversion instead.
    #[allow(dead_code)]
    pub fn not_found() -> Self {
        Error::Repository(RepositoryErrorType::NotFound)
    }

    /// A missing, invalid, or expired credential. The message stays generic so
    /// a caller cannot tell which of those it was.
    pub fn unauthorized() -> Self {
        Error::Unauthorized
    }

    /// An unexpected failure is worth a stack trace; an expected one is not.
    pub fn log(&self) {
        match self {
            Error::External(_) => error!("{:?}", self),
            _ => debug!("{}", self),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RepositoryErrorType {
    #[error("entity not found")]
    NotFound,
    #[error("entity conflict")]
    Conflict,
}
