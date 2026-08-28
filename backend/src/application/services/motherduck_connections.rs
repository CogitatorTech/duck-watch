use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::entities::motherduck_connections::{MotherDuckConnection, MotherDuckToken};
use crate::domain::error::Result;

/// The ingestion poller's per-connection bookkeeping after a sync attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub watermark_start_time: Option<DateTime<Utc>>,
    pub last_synced_at: DateTime<Utc>,
    /// Set when the attempt succeeded. `None` leaves the stored success time
    /// alone, so a failure does not erase when ingestion last worked.
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_sync_error: Option<String>,
    /// Set when the pass worked but lost rows for good. `None` leaves the
    /// stored warning alone, because the rows a past pass skipped stay
    /// missing however well later passes go.
    pub ingest_warning: Option<String>,
}

/// Storage boundary for MotherDuck connections. Org-scoped methods serve the
/// web layer; the rest serve the ingestion poller. The token only surfaces
/// through `get_token`, decrypted on the way out.
#[async_trait]
pub trait MotherDuckConnectionService: Send + Sync {
    async fn find_all(&self) -> Result<Vec<MotherDuckConnection>>;
    async fn find_by_id(&self, id: Uuid) -> Result<MotherDuckConnection>;
    async fn insert(
        &self,
        connection: MotherDuckConnection,
        token: MotherDuckToken,
    ) -> Result<MotherDuckConnection>;
    async fn delete(&self, id: Uuid) -> Result<()>;
    async fn find_enabled(&self) -> Result<Vec<MotherDuckConnection>>;
    async fn get_token(&self, id: Uuid) -> Result<MotherDuckToken>;
    async fn update_sync_state(&self, id: Uuid, state: SyncState) -> Result<()>;
}

#[cfg(test)]
mockall::mock! {
    pub MotherDuckConnectionService {}
    #[async_trait]
    impl MotherDuckConnectionService for MotherDuckConnectionService {
        async fn find_all(&self) -> Result<Vec<MotherDuckConnection>>;
        async fn find_by_id(&self, id: Uuid) -> Result<MotherDuckConnection>;
        async fn insert(
            &self,
            connection: MotherDuckConnection,
            token: MotherDuckToken,
        ) -> Result<MotherDuckConnection>;
        async fn delete(&self, id: Uuid) -> Result<()>;
        async fn find_enabled(&self) -> Result<Vec<MotherDuckConnection>>;
        async fn get_token(&self, id: Uuid) -> Result<MotherDuckToken>;
        async fn update_sync_state(&self, id: Uuid, state: SyncState) -> Result<()>;
    }
}
