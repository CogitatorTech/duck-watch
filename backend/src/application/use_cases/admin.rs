use async_trait::async_trait;

use crate::application::services::admin::AdminService;
use crate::application::use_cases::auth::AuthContext;
use crate::domain::entities::organizations::OrganizationOverview;
use crate::domain::error::{Error, Result};

#[async_trait]
pub trait AdminUseCaseTrait: Send + Sync {
    async fn list_organizations(&self, context: AuthContext) -> Result<Vec<OrganizationOverview>>;
}

pub struct AdminUseCase {
    admin_service: Box<dyn AdminService>,
}

impl AdminUseCase {
    pub fn new(admin_service: Box<dyn AdminService>) -> Self {
        Self { admin_service }
    }
}

#[async_trait]
impl AdminUseCaseTrait for AdminUseCase {
    async fn list_organizations(&self, context: AuthContext) -> Result<Vec<OrganizationOverview>> {
        if !context.is_superadmin {
            return Err(Error::forbidden());
        }
        self.admin_service.list_organization_overviews().await
    }
}

#[cfg(test)]
mockall::mock! {
    pub AdminUseCase {}
    #[async_trait]
    impl AdminUseCaseTrait for AdminUseCase {
        async fn list_organizations(&self, context: AuthContext) -> Result<Vec<OrganizationOverview>>;
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::application::services::admin::MockAdminService;

    fn context(is_superadmin: bool) -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            is_superadmin,
        }
    }

    #[tokio::test]
    async fn a_regular_user_is_forbidden() {
        let mut admin_service = MockAdminService::new();
        admin_service.expect_list_organization_overviews().never();

        let use_case = AdminUseCase::new(Box::new(admin_service));

        assert!(matches!(
            use_case
                .list_organizations(context(false))
                .await
                .unwrap_err(),
            Error::Forbidden
        ));
    }

    #[tokio::test]
    async fn a_superadmin_gets_the_overview() {
        let mut admin_service = MockAdminService::new();
        admin_service
            .expect_list_organization_overviews()
            .return_once(|| Ok(vec![]));

        let use_case = AdminUseCase::new(Box::new(admin_service));

        assert_eq!(
            use_case.list_organizations(context(true)).await.unwrap(),
            vec![]
        );
    }
}
