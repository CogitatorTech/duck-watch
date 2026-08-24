use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::users::{Email, PasswordHash, User};
use crate::domain::error::Result;

/// Storage boundary for the account. DuckWatch is a single account tool, so
/// there is at most one row here. The password hash only surfaces through
/// `find_by_email`, which the login flow needs for verification.
#[async_trait]
pub trait UserService: Send + Sync {
    async fn find_by_email(&self, email: &Email) -> Result<(User, PasswordHash)>;
    async fn find_by_id(&self, id: Uuid) -> Result<User>;
    /// Whether an account already exists, which is what closes the first run
    /// setup once it has been used.
    async fn any_exists(&self) -> Result<bool>;
    /// Creates the one account. It reports a conflict if one already exists,
    /// so two racing requests cannot both claim the instance.
    async fn create(&self, user: User, password_hash: PasswordHash) -> Result<User>;
}

#[cfg(test)]
mockall::mock! {
    pub UserService {}
    #[async_trait]
    impl UserService for UserService {
        async fn find_by_email(&self, email: &Email) -> Result<(User, PasswordHash)>;
        async fn find_by_id(&self, id: Uuid) -> Result<User>;
        async fn any_exists(&self) -> Result<bool>;
        async fn create(&self, user: User, password_hash: PasswordHash) -> Result<User>;
    }
}
