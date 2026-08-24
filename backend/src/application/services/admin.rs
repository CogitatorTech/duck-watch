use async_trait::async_trait;

use crate::domain::entities::organizations::OrganizationOverview;
use crate::domain::error::Result;

/// Cross-tenant reads for the platform operator. Nothing here is reachable
/// without a superadmin caller; the use case enforces that.
#[async_trait]
pub trait AdminService: Send + Sync {
    /// Every organization with its user count and connections, newest first.
    async fn list_organization_overviews(&self) -> Result<Vec<OrganizationOverview>>;
}

#[cfg(test)]
mockall::mock! {
    pub AdminService {}
    #[async_trait]
    impl AdminService for AdminService {
        async fn list_organization_overviews(&self) -> Result<Vec<OrganizationOverview>>;
    }
}
