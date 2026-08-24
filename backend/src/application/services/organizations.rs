use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::organizations::Organization;
use crate::domain::entities::users::{PasswordHash, User};
use crate::domain::error::Result;

/// Storage boundary for organizations. `create_with_owner` persists the
/// organization together with its first user in one transaction, so a
/// duplicate email cannot leave an empty organization behind.
#[async_trait]
pub trait OrganizationService: Send + Sync {
    async fn create_with_owner(
        &self,
        organization: Organization,
        user: User,
        password_hash: PasswordHash,
    ) -> Result<(Organization, User)>;
    async fn find_by_id(&self, id: Uuid) -> Result<Organization>;
}

#[cfg(test)]
mockall::mock! {
    pub OrganizationService {}
    #[async_trait]
    impl OrganizationService for OrganizationService {
        async fn create_with_owner(
            &self,
            organization: Organization,
            user: User,
            password_hash: PasswordHash,
        ) -> Result<(Organization, User)>;
        async fn find_by_id(&self, id: Uuid) -> Result<Organization>;
    }
}
