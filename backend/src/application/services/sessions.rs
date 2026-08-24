use async_trait::async_trait;

use crate::domain::entities::sessions::Session;
use crate::domain::error::Result;

/// Storage boundary for sessions, addressed by token hash so the raw token
/// never reaches the repository.
#[async_trait]
pub trait SessionService: Send + Sync {
    async fn insert(&self, session: Session, token_hash: Vec<u8>) -> Result<Session>;
    async fn find_by_token_hash(&self, token_hash: &[u8]) -> Result<Session>;
    async fn delete_by_token_hash(&self, token_hash: &[u8]) -> Result<()>;
}

#[cfg(test)]
mockall::mock! {
    pub SessionService {}
    #[async_trait]
    impl SessionService for SessionService {
        async fn insert(&self, session: Session, token_hash: Vec<u8>) -> Result<Session>;
        async fn find_by_token_hash(&self, token_hash: &[u8]) -> Result<Session>;
        async fn delete_by_token_hash(&self, token_hash: &[u8]) -> Result<()>;
    }
}
