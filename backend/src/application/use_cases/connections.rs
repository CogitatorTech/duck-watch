use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::application::services::motherduck::MotherDuckClient;
use crate::application::services::motherduck_connections::MotherDuckConnectionService;
use crate::application::use_cases::auth::AuthContext;
use crate::domain::entities::motherduck_connections::{
    ConnectionDraft, ConnectionStatus, MotherDuckConnection,
};
use crate::domain::error::Result;

#[async_trait]
pub trait ConnectionsUseCaseTrait: Send + Sync {
    /// Every connection the caller's organization owns, each carrying how its
    /// ingestion is going, since that applies to every dashboard number.
    async fn list_connections(&self, context: AuthContext) -> Result<Vec<ConnectionStatus>>;
    async fn create_connection(
        &self,
        context: AuthContext,
        draft: ConnectionDraft,
    ) -> Result<MotherDuckConnection>;
    async fn delete_connection(&self, context: AuthContext, id: Uuid) -> Result<()>;
}

pub struct ConnectionsUseCase {
    connection_service: Box<dyn MotherDuckConnectionService>,
    motherduck_client: Box<dyn MotherDuckClient>,
    /// How long a connection may go without a successful sync before it is
    /// reported as stale, derived from how often the poller runs.
    stale_after: chrono::Duration,
}

impl ConnectionsUseCase {
    pub fn new(
        connection_service: Box<dyn MotherDuckConnectionService>,
        motherduck_client: Box<dyn MotherDuckClient>,
        stale_after: chrono::Duration,
    ) -> Self {
        Self {
            connection_service,
            motherduck_client,
            stale_after,
        }
    }
}

#[async_trait]
impl ConnectionsUseCaseTrait for ConnectionsUseCase {
    async fn list_connections(&self, _context: AuthContext) -> Result<Vec<ConnectionStatus>> {
        let now = Utc::now();
        Ok(self
            .connection_service
            .find_all()
            .await?
            .iter()
            .map(|connection| connection.status(now, self.stale_after))
            .collect())
    }

    async fn create_connection(
        &self,
        _context: AuthContext,
        draft: ConnectionDraft,
    ) -> Result<MotherDuckConnection> {
        // Validate against MotherDuck first, so a bad token or a plan without
        // query history access never gets stored.
        self.motherduck_client
            .test_connection(draft.token())
            .await?;

        let (connection, token) = draft.into_new_connection(Utc::now());
        self.connection_service.insert(connection, token).await
    }

    async fn delete_connection(&self, _context: AuthContext, id: Uuid) -> Result<()> {
        self.connection_service.delete(id).await
    }
}

#[cfg(test)]
mockall::mock! {
    pub ConnectionsUseCase {}
    #[async_trait]
    impl ConnectionsUseCaseTrait for ConnectionsUseCase {
        async fn list_connections(&self, context: AuthContext) -> Result<Vec<ConnectionStatus>>;
        async fn create_connection(
            &self,
            context: AuthContext,
            draft: ConnectionDraft,
        ) -> Result<MotherDuckConnection>;
        async fn delete_connection(&self, context: AuthContext, id: Uuid) -> Result<()>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::motherduck::MockMotherDuckClient;
    use crate::application::services::motherduck_connections::MockMotherDuckConnectionService;
    use crate::domain::entities::motherduck_connections::IngestionHealth;
    use crate::domain::entities::pricing::RegionTier;
    use crate::domain::error::Error;

    fn stale_after() -> chrono::Duration {
        chrono::Duration::minutes(5)
    }

    fn context() -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn listing_reports_ingestion_health_per_connection() {
        let context = context();
        let now = Utc::now();

        let mut service = MockMotherDuckConnectionService::new();
        service.expect_find_all().return_once(move || {
            let fresh = |name: &str| {
                ConnectionDraft::new(name, "tok", RegionTier::Tier1)
                    .unwrap()
                    .into_new_connection(now)
                    .0
            };

            let mut healthy = fresh("healthy");
            healthy.last_synced_at = Some(now);
            healthy.last_success_at = Some(now);

            let mut broken = fresh("broken");
            broken.last_synced_at = Some(now);
            broken.last_success_at = Some(now - chrono::Duration::hours(9));
            broken.last_sync_error = Some("permission denied".into());

            Ok(vec![healthy, broken])
        });

        let use_case = ConnectionsUseCase::new(
            Box::new(service),
            Box::new(MockMotherDuckClient::new()),
            stale_after(),
        );

        let statuses = use_case.list_connections(context).await.unwrap();

        assert_eq!(statuses[0].health, IngestionHealth::Healthy);
        assert_eq!(statuses[1].health, IngestionHealth::Failing);
        // The failing connection reports how long its data has been stale,
        // which the attempt time alone would not show.
        assert!(statuses[1].seconds_since_success.unwrap() >= 9 * 3600);
    }

    #[tokio::test]
    async fn create_connection_validates_the_token_first() {
        let context = context();

        let mut client = MockMotherDuckClient::new();
        client.expect_test_connection().return_once(|_| Ok(()));
        let mut service = MockMotherDuckConnectionService::new();
        service
            .expect_insert()
            .withf(|connection, token| token.reveal() == "tok" && connection.enabled)
            .return_once(|connection, _| Ok(connection));

        let use_case = ConnectionsUseCase::new(Box::new(service), Box::new(client), stale_after());
        let draft = ConnectionDraft::new("prod", "tok", RegionTier::Tier1).unwrap();

        let created = use_case.create_connection(context, draft).await.unwrap();
        assert_eq!(created.name, "prod");
    }

    #[tokio::test]
    async fn create_connection_rejects_a_bad_token_without_storing() {
        let mut client = MockMotherDuckClient::new();
        client
            .expect_test_connection()
            .return_once(|_| Err(Error::validation("no access")));
        let mut service = MockMotherDuckConnectionService::new();
        service.expect_insert().never();

        let use_case = ConnectionsUseCase::new(Box::new(service), Box::new(client), stale_after());
        let draft = ConnectionDraft::new("prod", "tok", RegionTier::Tier1).unwrap();

        assert!(matches!(
            use_case
                .create_connection(context(), draft)
                .await
                .unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[tokio::test]
    async fn delete_connection_passes_the_id_through() {
        let context = context();
        let id = Uuid::new_v4();

        let mut service = MockMotherDuckConnectionService::new();
        service
            .expect_delete()
            .withf(move |got_id| *got_id == id)
            .return_once(|_| Ok(()));

        let use_case = ConnectionsUseCase::new(
            Box::new(service),
            Box::new(MockMotherDuckClient::new()),
            stale_after(),
        );

        use_case.delete_connection(context, id).await.unwrap();
    }
}
