use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::motherduck_connections::MotherDuckToken;
use crate::domain::entities::query_events::QueryEventDraft;
use crate::domain::entities::storage_samples::StorageSampleDraft;
use crate::domain::error::Result;

/// External-system boundary for MotherDuck itself. The implementation in
/// `infrastructure/motherduck/` speaks to it through a DuckDB connection.
#[async_trait]
pub trait MotherDuckClient: Send + Sync {
    /// Opens a connection and touches the query history view, so a bad token
    /// or a plan without history access fails before anything is stored.
    async fn test_connection(&self, token: &MotherDuckToken) -> Result<()>;

    /// Fetches query history rows that started after `since`, oldest first,
    /// at most `limit` of them.
    async fn fetch_query_history(
        &self,
        token: &MotherDuckToken,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<QueryEventDraft>>;

    /// Reads per database storage. This needs a wider permission than the
    /// query history does, so a caller must tolerate it failing on its own.
    async fn fetch_storage(&self, token: &MotherDuckToken) -> Result<Vec<StorageSampleDraft>>;
}

#[cfg(test)]
mockall::mock! {
    pub MotherDuckClient {}
    #[async_trait]
    impl MotherDuckClient for MotherDuckClient {
        async fn test_connection(&self, token: &MotherDuckToken) -> Result<()>;
        async fn fetch_query_history(
            &self,
            token: &MotherDuckToken,
            since: Option<DateTime<Utc>>,
            limit: u32,
        ) -> Result<Vec<QueryEventDraft>>;
        async fn fetch_storage(&self, token: &MotherDuckToken) -> Result<Vec<StorageSampleDraft>>;
    }
}
