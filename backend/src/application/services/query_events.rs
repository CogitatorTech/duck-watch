use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::entities::query_events::{
    AttributionCell, AttributionKey, CostBucketCell, DashboardSummary, EventFilter, EventQuery,
    FilterValues, LatencyBucket, QueryEvent, TimeRange,
};
use crate::domain::entities::query_shapes::{ShapeCell, UnfingerprintedQuery};
use crate::domain::error::Result;

/// Storage boundary for ingested query events: an idempotent batch write for
/// the poller and the aggregate reads behind the dashboard. Internal rows
/// (DuckWatch's own polling traffic) only count when a read asks for them.
#[async_trait]
pub trait QueryEventService: Send + Sync {
    /// Inserts the batch, updating rows already present so re-reading an
    /// overlap window and late-completing queries are both safe.
    async fn upsert_batch(&self, events: Vec<QueryEvent>) -> Result<u64>;
    async fn summary(&self, connection_id: Uuid, filter: EventFilter) -> Result<DashboardSummary>;
    async fn latency_buckets(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<LatencyBucket>>;
    /// The dashboard lists: slow queries and failures share this, differing
    /// only in the filter and ordering carried by `query`.
    async fn list_events(&self, connection_id: Uuid, query: EventQuery) -> Result<Vec<QueryEvent>>;
    /// One event with its full query text, for the expanded row view.
    async fn find_event(&self, connection_id: Uuid, md_query_id: Uuid) -> Result<QueryEvent>;
    /// Per shape and Duckling size counts, which the application prices.
    async fn shape_cells(&self, connection_id: Uuid, filter: EventFilter)
    -> Result<Vec<ShapeCell>>;
    /// Statements still waiting for a fingerprint, oldest first.
    async fn find_unfingerprinted(
        &self,
        connection_id: Uuid,
        limit: u32,
    ) -> Result<Vec<UnfingerprintedQuery>>;
    /// Attaches fingerprints to events that were ingested before analysis ran.
    async fn set_fingerprints(
        &self,
        connection_id: Uuid,
        assignments: Vec<(Uuid, String)>,
    ) -> Result<u64>;
    /// Per chart bucket and Duckling size counts, which the application prices
    /// into the cost view of the chart.
    async fn cost_cells(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<CostBucketCell>>;
    /// Per group and Duckling size counts, which the application prices.
    async fn attribution_cells(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
        key: AttributionKey,
    ) -> Result<Vec<AttributionCell>>;
    /// Distinct users and query categories in the window, for filter menus.
    async fn filter_values(&self, connection_id: Uuid, range: TimeRange) -> Result<FilterValues>;
}

#[cfg(test)]
mockall::mock! {
    pub QueryEventService {}
    #[async_trait]
    impl QueryEventService for QueryEventService {
        async fn upsert_batch(&self, events: Vec<QueryEvent>) -> Result<u64>;
        async fn summary(
            &self,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<DashboardSummary>;
        async fn latency_buckets(
            &self,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<Vec<LatencyBucket>>;
        async fn list_events(
            &self,
            connection_id: Uuid,
            query: EventQuery,
        ) -> Result<Vec<QueryEvent>>;
        async fn cost_cells(
            &self,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<Vec<CostBucketCell>>;
        async fn attribution_cells(
            &self,
            connection_id: Uuid,
            filter: EventFilter,
            key: AttributionKey,
        ) -> Result<Vec<AttributionCell>>;
        async fn find_event(&self, connection_id: Uuid, md_query_id: Uuid) -> Result<QueryEvent>;
        async fn shape_cells(
            &self,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<Vec<ShapeCell>>;
        async fn find_unfingerprinted(
            &self,
            connection_id: Uuid,
            limit: u32,
        ) -> Result<Vec<UnfingerprintedQuery>>;
        async fn set_fingerprints(
            &self,
            connection_id: Uuid,
            assignments: Vec<(Uuid, String)>,
        ) -> Result<u64>;
        async fn filter_values(
            &self,
            connection_id: Uuid,
            range: TimeRange,
        ) -> Result<FilterValues>;
    }
}
