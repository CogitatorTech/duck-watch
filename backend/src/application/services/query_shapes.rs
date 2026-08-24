use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::insights::Antipattern;
use crate::domain::entities::query_shapes::{QueryShape, ShapeStatement, UnflaggedShape};
use crate::domain::error::Result;

/// Storage boundary for query shapes, which hold one normalized statement and
/// one example per fingerprint.
#[async_trait]
pub trait QueryShapeService: Send + Sync {
    /// Records shapes that are new, leaving the first example and first seen
    /// time of a shape already known.
    async fn upsert_batch(&self, shapes: Vec<QueryShape>) -> Result<u64>;
    /// One shape's full statement, which list responses cut short.
    async fn find_statement(
        &self,
        connection_id: Uuid,
        fingerprint: &str,
    ) -> Result<ShapeStatement>;
    /// Shapes stored before anti-pattern analysis existed, oldest first.
    async fn find_unflagged(&self, connection_id: Uuid, limit: u32) -> Result<Vec<UnflaggedShape>>;
    /// Records what analysis found, including finding nothing, so an examined
    /// shape is not examined again.
    async fn set_antipatterns(
        &self,
        connection_id: Uuid,
        assignments: Vec<(String, Vec<Antipattern>)>,
    ) -> Result<u64>;
}

#[cfg(test)]
mockall::mock! {
    pub QueryShapeService {}
    #[async_trait]
    impl QueryShapeService for QueryShapeService {
        async fn upsert_batch(&self, shapes: Vec<QueryShape>) -> Result<u64>;
        async fn find_statement(
            &self,
            connection_id: Uuid,
            fingerprint: &str,
        ) -> Result<ShapeStatement>;
        async fn find_unflagged(
            &self,
            connection_id: Uuid,
            limit: u32,
        ) -> Result<Vec<UnflaggedShape>>;
        async fn set_antipatterns(
            &self,
            connection_id: Uuid,
            assignments: Vec<(String, Vec<Antipattern>)>,
        ) -> Result<u64>;
    }
}
