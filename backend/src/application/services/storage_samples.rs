use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::storage_samples::StorageSample;
use crate::domain::error::Result;

/// Storage boundary for MotherDuck storage measurements.
#[async_trait]
pub trait StorageSampleService: Send + Sync {
    /// Writes a batch, ignoring measurements already recorded, so re-reading
    /// the same computation is harmless.
    async fn upsert_batch(&self, samples: Vec<StorageSample>) -> Result<u64>;
    /// The newest measurement per database for one connection.
    async fn latest_by_connection(&self, connection_id: Uuid) -> Result<Vec<StorageSample>>;
}

#[cfg(test)]
mockall::mock! {
    pub StorageSampleService {}
    #[async_trait]
    impl StorageSampleService for StorageSampleService {
        async fn upsert_batch(&self, samples: Vec<StorageSample>) -> Result<u64>;
        async fn latest_by_connection(&self, connection_id: Uuid) -> Result<Vec<StorageSample>>;
    }
}
