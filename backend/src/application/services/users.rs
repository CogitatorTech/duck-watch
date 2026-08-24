use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::users::{Email, PasswordHash, User};
use crate::domain::error::Result;

/// Storage boundary for users. The password hash only surfaces through
/// `find_by_email`, which the login flow needs for verification.
#[async_trait]
pub trait UserService: Send + Sync {
    async fn find_by_email(&self, email: &Email) -> Result<(User, PasswordHash)>;
    async fn find_by_id(&self, id: Uuid) -> Result<User>;
}

#[cfg(test)]
mockall::mock! {
    pub UserService {}
    #[async_trait]
    impl UserService for UserService {
        async fn find_by_email(&self, email: &Email) -> Result<(User, PasswordHash)>;
        async fn find_by_id(&self, id: Uuid) -> Result<User>;
    }
}
