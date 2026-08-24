use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

use crate::application::services::motherduck_connections::MotherDuckConnectionService;
use crate::application::services::query_events::QueryEventService;
use crate::application::services::query_shapes::QueryShapeService;
use crate::application::services::storage_samples::StorageSampleService;
use crate::application::use_cases::auth::AuthContext;
use crate::domain::entities::insights::{self, Insights};
use crate::domain::entities::motherduck_connections::MotherDuckConnection;
use crate::domain::entities::pricing::RegionTier;
use crate::domain::entities::query_events::{
    Attribution, AttributionCell, AttributionKey, AttributionRow, DashboardSummary, EventFilter,
    EventQuery, FilterValues, LatencyBucket, QueryEvent, SortDirection, SortKey, TimeRange,
};
use crate::domain::entities::query_shapes::{ShapeCell, ShapeStatement, ShapeStats};
use crate::domain::entities::storage_samples::StorageSummary;
use crate::domain::error::Result;

/// Slow query and failure lists are bounded server-side no matter what the
/// client asks for.
const MAX_LIST_LIMIT: u32 = 100;

/// Long statements are truncated in list responses; the events table keeps
/// the full text. The bound only guards the payload size, so it is generous
/// enough for the expanded row view in the dashboard.
const QUERY_TEXT_PREVIEW_LEN: usize = 2000;

/// The list options a dashboard caller controls, after validation: which
/// events, then how many and in what order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOptions {
    pub filter: EventFilter,
    pub limit: u32,
    pub sort: SortKey,
    pub direction: SortDirection,
}

#[async_trait]
pub trait DashboardUseCaseTrait: Send + Sync {
    async fn summary(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<DashboardSummary>;
    async fn latency_buckets(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<LatencyBucket>>;
    async fn top_slow(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        options: ListOptions,
    ) -> Result<Vec<QueryEvent>>;
    async fn recent_failures(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        options: ListOptions,
    ) -> Result<Vec<QueryEvent>>;
    /// One event with its untruncated query text, for the expanded row view.
    async fn get_event(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        md_query_id: Uuid,
    ) -> Result<QueryEvent>;
    /// Cost attribution by user and by Duckling size, each row carrying its
    /// share of the period and what the same group cost in the period before.
    async fn attribution(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Attribution>;
    /// Distinct query shapes over the period, most expensive first. A shape
    /// groups queries that differ only in their values.
    async fn shapes(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
        limit: u32,
    ) -> Result<Vec<ShapeStats>>;
    /// One shape's full statement. The shape lists cut long statements to
    /// keep the payload small, so copying one reads it from here instead.
    async fn shape_statement(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        fingerprint: String,
    ) -> Result<ShapeStatement>;
    /// Query patterns worth reviewing over the period, most expensive first.
    /// These are signals to check rather than conclusions, so each one carries
    /// what it actually cost.
    async fn insights(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
        limit: u32,
    ) -> Result<Insights>;
    /// What the account currently stores, and the monthly run rate that
    /// implies. Storage is a level rather than an event stream, so it does
    /// not take the dashboard's time range.
    async fn storage(&self, context: AuthContext, connection_id: Uuid) -> Result<StorageSummary>;
    /// The filter menu choices for one connection and window.
    async fn filter_values(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        range: TimeRange,
    ) -> Result<FilterValues>;
}

pub struct DashboardUseCase {
    connection_service: Box<dyn MotherDuckConnectionService>,
    query_event_service: Box<dyn QueryEventService>,
    storage_sample_service: Box<dyn StorageSampleService>,
    query_shape_service: Box<dyn QueryShapeService>,
}

impl DashboardUseCase {
    pub fn new(
        connection_service: Box<dyn MotherDuckConnectionService>,
        query_event_service: Box<dyn QueryEventService>,
        storage_sample_service: Box<dyn StorageSampleService>,
        query_shape_service: Box<dyn QueryShapeService>,
    ) -> Self {
        Self {
            connection_service,
            query_event_service,
            storage_sample_service,
            query_shape_service,
        }
    }

    /// Every read first proves the connection belongs to the caller's
    /// organization; a foreign id reports as not found. The connection comes
    /// back because its region tier prices the results.
    async fn authorize(
        &self,
        context: AuthContext,
        connection_id: Uuid,
    ) -> Result<MotherDuckConnection> {
        self.connection_service
            .find_by_id_and_org(connection_id, context.org_id)
            .await
    }

    async fn list(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        options: ListOptions,
        failures_only: bool,
    ) -> Result<Vec<QueryEvent>> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let events = self
            .query_event_service
            .list_events(
                connection_id,
                EventQuery {
                    filter: options.filter,
                    limit: options.limit.min(MAX_LIST_LIMIT),
                    sort: options.sort,
                    direction: options.direction,
                    failures_only,
                },
            )
            .await?;
        Ok(truncate_query_text(price_events(events, tier)))
    }
}

/// Attaches the cost estimate each event's Duckling size and run time imply.
fn price_events(mut events: Vec<QueryEvent>, tier: RegionTier) -> Vec<QueryEvent> {
    for event in &mut events {
        event.estimated_cost_usd =
            tier.estimate_cost_usd(event.instance_type.as_deref(), event.total_elapsed_time_ms);
    }
    events
}

/// Folds the per size cells into one priced row per group, largest cost
/// first. `previous` supplies what each group cost in the period before.
fn fold_attribution(
    cells: Vec<AttributionCell>,
    previous: &HashMap<String, f64>,
    tier: RegionTier,
) -> Vec<AttributionRow> {
    let mut rows: HashMap<String, AttributionRow> = HashMap::new();

    for cell in cells {
        let cost =
            tier.estimate_group_cost_usd(&cell.instance_type, cell.total_ms, cell.query_count);
        let row = rows.entry(cell.key.clone()).or_insert(AttributionRow {
            key: cell.key,
            query_count: 0,
            failure_count: 0,
            total_ms: 0,
            estimated_cost_usd: 0.0,
            cost_share: 0.0,
            previous_cost_usd: 0.0,
        });
        row.query_count += cell.query_count;
        row.failure_count += cell.failure_count;
        row.total_ms += cell.total_ms;
        row.estimated_cost_usd += cost;
    }

    let total: f64 = rows.values().map(|row| row.estimated_cost_usd).sum();
    let mut rows: Vec<AttributionRow> = rows.into_values().collect();
    for row in &mut rows {
        row.cost_share = match total > 0.0 {
            true => row.estimated_cost_usd / total,
            false => 0.0,
        };
        row.previous_cost_usd = previous.get(&row.key).copied().unwrap_or(0.0);
    }

    // Biggest spender first, with the name breaking ties so the order is
    // stable between refreshes.
    rows.sort_by(|a, b| {
        b.estimated_cost_usd
            .partial_cmp(&a.estimated_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
    rows
}

/// Folds the per size cells into one priced row per shape, most expensive
/// first.
fn fold_shapes(cells: Vec<ShapeCell>, tier: RegionTier) -> Vec<ShapeStats> {
    let mut shapes: HashMap<String, ShapeStats> = HashMap::new();

    for cell in cells {
        let cost = tier.estimate_group_cost_usd(&cell.instance_type, cell.total_ms, cell.runs);
        let entry = shapes
            .entry(cell.fingerprint.clone())
            .or_insert(ShapeStats {
                fingerprint: cell.fingerprint,
                example_sql: String::new(),
                runs: 0,
                failure_count: 0,
                total_ms: 0,
                max_ms: 0,
                bytes_spilled: 0,
                // Every cell of one shape carries the same flags, so the
                // first cell to arrive settles them.
                antipatterns: cell.antipatterns,
                estimated_cost_usd: 0.0,
                cost_share: 0.0,
                last_seen: cell.last_seen,
            });
        entry.runs += cell.runs;
        entry.failure_count += cell.failure_count;
        entry.total_ms += cell.total_ms;
        entry.max_ms = entry.max_ms.max(cell.max_ms);
        entry.bytes_spilled = entry.bytes_spilled.saturating_add(cell.bytes_spilled);
        entry.estimated_cost_usd += cost;
        entry.last_seen = entry.last_seen.max(cell.last_seen);
        if entry.example_sql.is_empty() {
            entry.example_sql = cell.example_sql;
        }
    }

    let total: f64 = shapes.values().map(|shape| shape.estimated_cost_usd).sum();
    let mut shapes: Vec<ShapeStats> = shapes.into_values().collect();
    for shape in &mut shapes {
        shape.cost_share = match total > 0.0 {
            true => shape.estimated_cost_usd / total,
            false => 0.0,
        };
        if shape.example_sql.chars().count() > QUERY_TEXT_PREVIEW_LEN {
            shape.example_sql = shape
                .example_sql
                .chars()
                .take(QUERY_TEXT_PREVIEW_LEN)
                .collect::<String>()
                + "...";
        }
    }

    shapes.sort_by(|a, b| {
        b.estimated_cost_usd
            .partial_cmp(&a.estimated_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.runs.cmp(&a.runs))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });
    shapes
}

/// The cost each group ran up, for comparing one period against another.
fn cost_by_key(cells: Vec<AttributionCell>, tier: RegionTier) -> HashMap<String, f64> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for cell in cells {
        let cost =
            tier.estimate_group_cost_usd(&cell.instance_type, cell.total_ms, cell.query_count);
        *totals.entry(cell.key).or_insert(0.0) += cost;
    }
    totals
}

fn truncate_query_text(mut events: Vec<QueryEvent>) -> Vec<QueryEvent> {
    for event in &mut events {
        if event.query_text.chars().count() > QUERY_TEXT_PREVIEW_LEN {
            event.query_text = event
                .query_text
                .chars()
                .take(QUERY_TEXT_PREVIEW_LEN)
                .collect::<String>()
                + "...";
        }
    }
    events
}

#[async_trait]
impl DashboardUseCaseTrait for DashboardUseCase {
    async fn summary(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<DashboardSummary> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let mut summary = self
            .query_event_service
            .summary(connection_id, filter)
            .await?;

        // The store counts and sums; the tier turns that into money.
        for entry in &mut summary.instance_types {
            entry.estimated_cost_usd = tier.estimate_group_cost_usd(
                &entry.instance_type,
                entry.total_ms,
                entry.query_count,
            );
        }
        summary.estimated_cost_usd = summary
            .instance_types
            .iter()
            .map(|entry| entry.estimated_cost_usd)
            .sum();

        Ok(summary)
    }

    async fn latency_buckets(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<LatencyBucket>> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let mut buckets = self
            .query_event_service
            .latency_buckets(connection_id, filter.clone())
            .await?;

        // Price each bucket from its per size cells, so the chart can plot
        // money as well as latency.
        let cells = self
            .query_event_service
            .cost_cells(connection_id, filter)
            .await?;
        let mut cost_by_bucket: HashMap<_, f64> = HashMap::new();
        for cell in cells {
            let cost =
                tier.estimate_group_cost_usd(&cell.instance_type, cell.total_ms, cell.query_count);
            *cost_by_bucket.entry(cell.bucket_start).or_insert(0.0) += cost;
        }
        for bucket in &mut buckets {
            bucket.estimated_cost_usd = cost_by_bucket
                .get(&bucket.bucket_start)
                .copied()
                .unwrap_or(0.0);
        }

        Ok(buckets)
    }

    async fn attribution(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Attribution> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let previous_filter = EventFilter {
            range: filter.range.previous(),
            ..filter.clone()
        };

        let mut by_user = Vec::new();
        let mut by_instance_type = Vec::new();
        let mut previous_cost_usd = 0.0;
        for key in [AttributionKey::User, AttributionKey::InstanceType] {
            let previous = cost_by_key(
                self.query_event_service
                    .attribution_cells(connection_id, previous_filter.clone(), key)
                    .await?,
                tier,
            );
            let rows = fold_attribution(
                self.query_event_service
                    .attribution_cells(connection_id, filter.clone(), key)
                    .await?,
                &previous,
                tier,
            );
            match key {
                AttributionKey::User => {
                    // The total for the period before has to come from that
                    // period's own groups. Summing the rows below would only
                    // count spenders who are still active, so a user who has
                    // since stopped would silently vanish from the comparison.
                    previous_cost_usd = previous.values().sum();
                    by_user = rows;
                }
                AttributionKey::InstanceType => by_instance_type = rows,
            }
        }

        let estimated_cost_usd = by_user.iter().map(|row| row.estimated_cost_usd).sum();

        Ok(Attribution {
            by_user,
            by_instance_type,
            estimated_cost_usd,
            previous_cost_usd,
        })
    }

    async fn top_slow(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        options: ListOptions,
    ) -> Result<Vec<QueryEvent>> {
        self.list(context, connection_id, options, false).await
    }

    async fn recent_failures(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        options: ListOptions,
    ) -> Result<Vec<QueryEvent>> {
        self.list(context, connection_id, options, true).await
    }

    async fn get_event(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        md_query_id: Uuid,
    ) -> Result<QueryEvent> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let event = self
            .query_event_service
            .find_event(connection_id, md_query_id)
            .await?;
        Ok(price_events(vec![event], tier).remove(0))
    }

    async fn shapes(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
        limit: u32,
    ) -> Result<Vec<ShapeStats>> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let cells = self
            .query_event_service
            .shape_cells(connection_id, filter)
            .await?;

        let mut shapes = fold_shapes(cells, tier);
        shapes.truncate(limit.min(MAX_LIST_LIMIT) as usize);
        Ok(shapes)
    }

    async fn shape_statement(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        fingerprint: String,
    ) -> Result<ShapeStatement> {
        self.authorize(context, connection_id).await?;
        self.query_shape_service
            .find_statement(connection_id, &fingerprint)
            .await
    }

    async fn insights(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        filter: EventFilter,
        limit: u32,
    ) -> Result<Insights> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let cells = self
            .query_event_service
            .shape_cells(connection_id, filter)
            .await?;

        // Detection runs on priced shapes, because what a finding cost is
        // what decides whether it is worth reporting.
        let mut found = insights::detect(&fold_shapes(cells, tier));
        // The count and the per kind totals are taken before the cap, so a
        // capped list still says how much it is leaving out.
        found.findings.truncate(limit.min(MAX_LIST_LIMIT) as usize);
        Ok(found)
    }

    async fn storage(&self, context: AuthContext, connection_id: Uuid) -> Result<StorageSummary> {
        let tier = self.authorize(context, connection_id).await?.region_tier;
        let samples = self
            .storage_sample_service
            .latest_by_connection(connection_id)
            .await?;
        Ok(StorageSummary::from_samples(samples, tier))
    }

    async fn filter_values(
        &self,
        context: AuthContext,
        connection_id: Uuid,
        range: TimeRange,
    ) -> Result<FilterValues> {
        self.authorize(context, connection_id).await?;
        self.query_event_service
            .filter_values(connection_id, range)
            .await
    }
}

#[cfg(test)]
mockall::mock! {
    pub DashboardUseCase {}
    #[async_trait]
    impl DashboardUseCaseTrait for DashboardUseCase {
        async fn summary(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<DashboardSummary>;
        async fn latency_buckets(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<Vec<LatencyBucket>>;
        async fn top_slow(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            options: ListOptions,
        ) -> Result<Vec<QueryEvent>>;
        async fn recent_failures(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            options: ListOptions,
        ) -> Result<Vec<QueryEvent>>;
        async fn get_event(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            md_query_id: Uuid,
        ) -> Result<QueryEvent>;
        async fn attribution(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            filter: EventFilter,
        ) -> Result<Attribution>;
        async fn shapes(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            filter: EventFilter,
            limit: u32,
        ) -> Result<Vec<ShapeStats>>;
        async fn shape_statement(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            fingerprint: String,
        ) -> Result<ShapeStatement>;
        async fn insights(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            filter: EventFilter,
            limit: u32,
        ) -> Result<Insights>;
        async fn storage(
            &self,
            context: AuthContext,
            connection_id: Uuid,
        ) -> Result<StorageSummary>;
        async fn filter_values(
            &self,
            context: AuthContext,
            connection_id: Uuid,
            range: TimeRange,
        ) -> Result<FilterValues>;
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::application::services::motherduck_connections::MockMotherDuckConnectionService;
    use crate::application::services::query_events::MockQueryEventService;
    use crate::application::services::query_shapes::MockQueryShapeService;
    use crate::application::services::storage_samples::MockStorageSampleService;
    use crate::domain::entities::insights::Antipattern;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::query_events::{CostBucketCell, InstanceTypeCount};
    use crate::domain::entities::query_events::{QueryEventDraft, TimeWindow};
    use crate::domain::entities::query_shapes::{ShapeCell, ShapeStatement};
    use crate::domain::error::Error;

    fn context() -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            is_superadmin: false,
        }
    }

    fn day() -> TimeRange {
        TimeRange::from_window(TimeWindow::Day, Utc::now())
    }

    fn options() -> ListOptions {
        ListOptions {
            filter: EventFilter::all(day(), false),
            limit: 5000,
            sort: SortKey::Duration,
            direction: SortDirection::Descending,
        }
    }

    fn allowing_connection_service(org_id: Uuid) -> MockMotherDuckConnectionService {
        let mut service = MockMotherDuckConnectionService::new();
        service.expect_find_by_id_and_org().returning(move |id, _| {
            let (mut connection, _) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
                .unwrap()
                .into_new_connection(org_id, Utc::now());
            connection.id = id;
            Ok(connection)
        });
        service
    }

    fn long_event(connection_id: Uuid) -> QueryEvent {
        QueryEventDraft {
            md_query_id: Uuid::new_v4(),
            query_text: "x".repeat(3000),
            query_type: None,
            start_time: Utc::now(),
            end_time: None,
            execution_time_ms: None,
            wait_time_ms: None,
            total_elapsed_time_ms: Some(10),
            error_type: None,
            error_message: None,
            user_name: None,
            instance_type: None,
            duckling_id: None,
            session_name: None,
            bytes_uploaded: None,
            bytes_downloaded: None,
            bytes_spilled_to_disk: None,
            user_agent: None,
        }
        .into_event(connection_id, Utc::now())
    }

    #[tokio::test]
    async fn reads_reject_a_foreign_connection() {
        let mut connection_service = MockMotherDuckConnectionService::new();
        connection_service
            .expect_find_by_id_and_org()
            .return_once(|_, _| Err(Error::not_found()));
        let mut query_events = MockQueryEventService::new();
        query_events.expect_summary().never();

        let use_case = DashboardUseCase::new(
            Box::new(connection_service),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        assert!(
            use_case
                .summary(context(), Uuid::new_v4(), EventFilter::all(day(), false))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn top_slow_truncates_long_query_text_and_caps_the_limit() {
        let context = context();
        let connection_id = Uuid::new_v4();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_list_events()
            .withf(move |_, query| {
                query.limit == MAX_LIST_LIMIT
                    && !query.failures_only
                    && query.sort == SortKey::Duration
            })
            .return_once(move |id, _| Ok(vec![long_event(id)]));

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let events = use_case
            .top_slow(context, connection_id, options())
            .await
            .unwrap();

        assert_eq!(
            events[0].query_text.chars().count(),
            QUERY_TEXT_PREVIEW_LEN + 3
        );
        assert!(events[0].query_text.ends_with("..."));
    }

    #[tokio::test]
    async fn listed_events_carry_a_cost_estimate() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events.expect_list_events().return_once(move |id, _| {
            let mut event = long_event(id);
            event.instance_type = Some("standard".into());
            event.total_elapsed_time_ms = Some(3_600_000);
            Ok(vec![event])
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let events = use_case
            .top_slow(context, Uuid::new_v4(), options())
            .await
            .unwrap();

        // An hour on a tier 1 Standard Duckling is the hourly rate.
        let cost = events[0].estimated_cost_usd.unwrap();
        assert!((cost - 2.40).abs() < 1e-9, "cost was {cost}");
    }

    #[tokio::test]
    async fn the_summary_prices_each_instance_type() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events.expect_summary().return_once(|_, _| {
            Ok(DashboardSummary {
                query_count: 2,
                failure_count: 0,
                p50_ms: None,
                p95_ms: None,
                instance_types: vec![
                    InstanceTypeCount {
                        instance_type: "standard".into(),
                        query_count: 1,
                        total_ms: 3_600_000,
                        estimated_cost_usd: 0.0,
                    },
                    InstanceTypeCount {
                        instance_type: "jumbo".into(),
                        query_count: 1,
                        total_ms: 3_600_000,
                        estimated_cost_usd: 0.0,
                    },
                ],
                estimated_cost_usd: 0.0,
            })
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let summary = use_case
            .summary(context, Uuid::new_v4(), EventFilter::all(day(), false))
            .await
            .unwrap();

        assert!((summary.instance_types[0].estimated_cost_usd - 2.40).abs() < 1e-9);
        assert!((summary.instance_types[1].estimated_cost_usd - 4.80).abs() < 1e-9);
        assert!((summary.estimated_cost_usd - 7.20).abs() < 1e-9);
    }

    fn cell(key: &str, instance: &str, count: i64, total_ms: i64) -> AttributionCell {
        AttributionCell {
            key: key.into(),
            instance_type: instance.into(),
            query_count: count,
            failure_count: 0,
            total_ms,
        }
    }

    fn shape_cell(fingerprint: &str, instance: &str, runs: i64, total_ms: i64) -> ShapeCell {
        ShapeCell {
            fingerprint: fingerprint.into(),
            instance_type: instance.into(),
            example_sql: format!("select from {fingerprint}"),
            runs,
            failure_count: 0,
            total_ms,
            max_ms: total_ms,
            bytes_spilled: 0,
            antipatterns: Vec::new(),
            last_seen: Utc::now(),
        }
    }

    #[tokio::test]
    async fn shapes_rank_by_cost_and_fold_across_sizes() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events.expect_shape_cells().return_once(|_, _| {
            Ok(vec![
                // One shape ran on two sizes, so its cost is the sum.
                shape_cell("aaaa", "standard", 1, 3_600_000),
                shape_cell("aaaa", "jumbo", 1, 3_600_000),
                shape_cell("bbbb", "standard", 10, 3_600_000),
            ])
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let shapes = use_case
            .shapes(context, Uuid::new_v4(), EventFilter::all(day(), false), 20)
            .await
            .unwrap();

        assert_eq!(shapes.len(), 2);
        // Standard plus jumbo for one hour each is $7.20, ahead of $2.40.
        assert_eq!(shapes[0].fingerprint, "aaaa");
        assert!((shapes[0].estimated_cost_usd - 7.20).abs() < 1e-9);
        assert_eq!(shapes[0].runs, 2);
        assert!((shapes[0].cost_share - 0.75).abs() < 1e-9);
        assert_eq!(shapes[1].runs, 10);
        assert!(!shapes[0].example_sql.is_empty());
    }

    #[tokio::test]
    async fn insights_are_raised_from_priced_shapes() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events.expect_shape_cells().return_once(|_, _| {
            // A dear shape carrying a flag, and a trivial one carrying the
            // same flag that must not be raised.
            let mut dear = shape_cell("aaaa", "standard", 1, 3_600_000);
            dear.antipatterns = vec![Antipattern::SelectStar];
            let mut trivial = shape_cell("bbbb", "standard", 1, 100);
            trivial.antipatterns = vec![Antipattern::SelectStar];
            Ok(vec![dear, trivial])
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let found = use_case
            .insights(context, Uuid::new_v4(), EventFilter::all(day(), false), 20)
            .await
            .unwrap();

        assert_eq!(found.findings.len(), 1);
        assert_eq!(found.total, 1);
        assert_eq!(found.findings[0].fingerprint, "aaaa");
        assert_eq!(found.findings[0].antipattern, Antipattern::SelectStar);
        // An hour on a tier 1 Standard Duckling.
        assert!((found.findings[0].estimated_cost_usd - 2.40).abs() < 1e-9);
        // One kind found, summed over the one shape that raised it.
        assert_eq!(found.totals.len(), 1);
        assert_eq!(found.totals[0].shapes, 1);
    }

    #[tokio::test]
    async fn a_capped_list_still_reports_everything_that_was_found() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events.expect_shape_cells().return_once(|_, _| {
            Ok((0..5)
                .map(|index| {
                    let mut cell = shape_cell(&format!("shape{index}"), "standard", 1, 3_600_000);
                    cell.bytes_spilled = 2_000_000_000;
                    cell
                })
                .collect())
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let found = use_case
            .insights(context, Uuid::new_v4(), EventFilter::all(day(), false), 2)
            .await
            .unwrap();

        // The list is capped, but nothing about the period is hidden.
        assert_eq!(found.findings.len(), 2);
        assert_eq!(found.total, 5);
        assert_eq!(found.totals[0].shapes, 5);
    }

    #[tokio::test]
    async fn the_full_statement_comes_back_untruncated() {
        // The lists cut long statements, so this read is the only way to get
        // a whole one, and it must not apply the same cut.
        let context = context();
        let long = "select ".to_string() + &"x".repeat(5000);
        let stored = long.clone();

        let mut shapes = MockQueryShapeService::new();
        shapes
            .expect_find_statement()
            .withf(|_, fingerprint| fingerprint == "aaaa")
            .return_once(move |_, fingerprint| {
                Ok(ShapeStatement {
                    fingerprint: fingerprint.to_string(),
                    example_sql: stored,
                    parsed: true,
                    first_seen: Utc::now(),
                })
            });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(MockQueryEventService::new()),
            Box::new(MockStorageSampleService::new()),
            Box::new(shapes),
        );

        let statement = use_case
            .shape_statement(context, Uuid::new_v4(), "aaaa".into())
            .await
            .unwrap();

        assert_eq!(statement.example_sql, long);
        assert!(statement.example_sql.chars().count() > QUERY_TEXT_PREVIEW_LEN);
    }

    #[tokio::test]
    async fn the_full_statement_rejects_a_foreign_connection() {
        let mut connection_service = MockMotherDuckConnectionService::new();
        connection_service
            .expect_find_by_id_and_org()
            .return_once(|_, _| Err(Error::not_found()));
        let mut shapes = MockQueryShapeService::new();
        shapes.expect_find_statement().never();

        let use_case = DashboardUseCase::new(
            Box::new(connection_service),
            Box::new(MockQueryEventService::new()),
            Box::new(MockStorageSampleService::new()),
            Box::new(shapes),
        );

        assert!(
            use_case
                .shape_statement(context(), Uuid::new_v4(), "aaaa".into())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn insights_reject_a_foreign_connection() {
        let mut connection_service = MockMotherDuckConnectionService::new();
        connection_service
            .expect_find_by_id_and_org()
            .return_once(|_, _| Err(Error::not_found()));
        let mut query_events = MockQueryEventService::new();
        query_events.expect_shape_cells().never();

        let use_case = DashboardUseCase::new(
            Box::new(connection_service),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        assert!(
            use_case
                .insights(
                    context(),
                    Uuid::new_v4(),
                    EventFilter::all(day(), false),
                    20
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn storage_is_priced_at_the_connection_tier() {
        use crate::domain::entities::storage_samples::StorageSampleDraft;

        let context = context();
        let connection_id = Uuid::new_v4();

        let mut storage = MockStorageSampleService::new();
        storage
            .expect_latest_by_connection()
            .return_once(move |id| {
                Ok(vec![
                    StorageSampleDraft {
                        database_name: "analytics".into(),
                        active_bytes: 2_000_000_000,
                        historical_bytes: 0,
                        retained_for_clone_bytes: 0,
                        failsafe_bytes: 0,
                        computed_at: Utc::now(),
                    }
                    .into_sample(id, Utc::now()),
                ])
            });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(MockQueryEventService::new()),
            Box::new(storage),
            Box::new(MockQueryShapeService::new()),
        );

        let summary = use_case.storage(context, connection_id).await.unwrap();

        assert_eq!(summary.total_bytes, 2_000_000_000);
        // Two gigabytes at the tier 1 rate of four cents each.
        assert!((summary.estimated_monthly_cost_usd - 0.08).abs() < 1e-9);
    }

    #[tokio::test]
    async fn attribution_ranks_spenders_and_compares_periods() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        // Two reads per grouping: the period, then the one before it.
        query_events
            .expect_attribution_cells()
            .times(4)
            .returning(|_, filter, key| {
                let now = Utc::now();
                let current = filter.range.end > now - chrono::Duration::minutes(1);
                Ok(match (key, current) {
                    // Alice ran an hour of Standard, Bob half an hour.
                    (AttributionKey::User, true) => vec![
                        cell("alice", "standard", 1, 3_600_000),
                        cell("bob", "standard", 1, 1_800_000),
                    ],
                    // Alice cost half as much in the period before.
                    (AttributionKey::User, false) => {
                        vec![cell("alice", "standard", 1, 1_800_000)]
                    }
                    (AttributionKey::InstanceType, true) => {
                        vec![cell("standard", "standard", 2, 5_400_000)]
                    }
                    (AttributionKey::InstanceType, false) => vec![],
                })
            });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let attribution = use_case
            .attribution(context, Uuid::new_v4(), EventFilter::all(day(), false))
            .await
            .unwrap();

        // Biggest spender first.
        let alice = &attribution.by_user[0];
        assert_eq!(alice.key, "alice");
        assert!((alice.estimated_cost_usd - 2.40).abs() < 1e-9);
        assert!((alice.cost_share - 2.0 / 3.0).abs() < 1e-9);
        assert!((alice.previous_cost_usd - 1.20).abs() < 1e-9);

        let bob = &attribution.by_user[1];
        assert!((bob.estimated_cost_usd - 1.20).abs() < 1e-9);
        // Bob is new this period, so there is nothing to compare against.
        assert_eq!(bob.previous_cost_usd, 0.0);

        assert!((attribution.estimated_cost_usd - 3.60).abs() < 1e-9);
        assert!((attribution.previous_cost_usd - 1.20).abs() < 1e-9);
        assert_eq!(attribution.by_instance_type.len(), 1);
        assert!((attribution.by_instance_type[0].cost_share - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn the_previous_total_counts_spenders_who_have_since_stopped() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_attribution_cells()
            .times(4)
            .returning(|_, filter, key| {
                let now = Utc::now();
                let current = filter.range.end > now - chrono::Duration::minutes(1);
                Ok(match (key, current) {
                    // Bob is the only spender this period.
                    (AttributionKey::User, true) => vec![cell("bob", "standard", 1, 1_800_000)],
                    // Alice spent last period and has run nothing since, so
                    // she is not a row this period.
                    (AttributionKey::User, false) => {
                        vec![cell("alice", "standard", 1, 3_600_000)]
                    }
                    (AttributionKey::InstanceType, true) => {
                        vec![cell("standard", "standard", 1, 1_800_000)]
                    }
                    (AttributionKey::InstanceType, false) => {
                        vec![cell("standard", "standard", 1, 3_600_000)]
                    }
                })
            });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let attribution = use_case
            .attribution(context, Uuid::new_v4(), EventFilter::all(day(), false))
            .await
            .unwrap();

        assert_eq!(attribution.by_user.len(), 1);
        assert!((attribution.estimated_cost_usd - 1.20).abs() < 1e-9);
        // Alice's hour on a Standard Duckling is what the period before cost,
        // whether or not she is still running queries.
        assert!(
            (attribution.previous_cost_usd - 2.40).abs() < 1e-9,
            "previous was {}",
            attribution.previous_cost_usd
        );
    }

    #[tokio::test]
    async fn a_period_without_cost_has_no_shares() {
        let context = context();
        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_attribution_cells()
            .times(4)
            .returning(|_, _, _| Ok(vec![]));

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let attribution = use_case
            .attribution(context, Uuid::new_v4(), EventFilter::all(day(), false))
            .await
            .unwrap();

        assert!(attribution.by_user.is_empty());
        assert_eq!(attribution.estimated_cost_usd, 0.0);
    }

    #[tokio::test]
    async fn the_chart_prices_each_bucket() {
        let context = context();
        let bucket_start = Utc::now();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_latency_buckets()
            .return_once(move |_, _| {
                Ok(vec![LatencyBucket {
                    bucket_start,
                    query_count: 1,
                    failure_count: 0,
                    p50_ms: None,
                    p95_ms: None,
                    estimated_cost_usd: 0.0,
                }])
            });
        query_events.expect_cost_cells().return_once(move |_, _| {
            Ok(vec![CostBucketCell {
                bucket_start,
                instance_type: "standard".into(),
                query_count: 1,
                total_ms: 3_600_000,
            }])
        });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let buckets = use_case
            .latency_buckets(context, Uuid::new_v4(), EventFilter::all(day(), false))
            .await
            .unwrap();

        assert!((buckets[0].estimated_cost_usd - 2.40).abs() < 1e-9);
    }

    #[tokio::test]
    async fn the_summary_honors_the_filters() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_summary()
            .withf(|_, filter| {
                filter.user_name.as_deref() == Some("bob") && filter.min_duration_ms == Some(500)
            })
            .return_once(|_, _| {
                Ok(DashboardSummary {
                    query_count: 1,
                    failure_count: 0,
                    p50_ms: None,
                    p95_ms: None,
                    instance_types: vec![],
                    estimated_cost_usd: 0.0,
                })
            });

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let summary = use_case
            .summary(
                context,
                Uuid::new_v4(),
                EventFilter {
                    user_name: Some("bob".into()),
                    min_duration_ms: Some(500),
                    ..EventFilter::all(day(), false)
                },
            )
            .await
            .unwrap();

        assert_eq!(summary.query_count, 1);
    }

    #[tokio::test]
    async fn top_slow_forwards_the_filters() {
        let context = context();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_list_events()
            .withf(|_, query| {
                query.filter.search.as_deref() == Some("sales")
                    && query.filter.user_name.as_deref() == Some("bob")
                    && query.filter.query_type.as_deref() == Some("DDL")
                    && query.filter.min_duration_ms == Some(500)
            })
            .return_once(|_, _| Ok(vec![]));

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let events = use_case
            .top_slow(
                context,
                Uuid::new_v4(),
                ListOptions {
                    filter: EventFilter {
                        search: Some("sales".into()),
                        user_name: Some("bob".into()),
                        query_type: Some("DDL".into()),
                        min_duration_ms: Some(500),
                        ..EventFilter::all(day(), false)
                    },
                    ..options()
                },
            )
            .await
            .unwrap();
        assert_eq!(events, vec![]);
    }

    #[tokio::test]
    async fn recent_failures_forwards_the_sort_and_internal_options() {
        let context = context();
        let connection_id = Uuid::new_v4();

        let mut query_events = MockQueryEventService::new();
        query_events
            .expect_list_events()
            .withf(|_, query| {
                query.failures_only
                    && query.filter.include_internal
                    && query.sort == SortKey::StartTime
                    && query.direction == SortDirection::Ascending
            })
            .return_once(|_, _| Ok(vec![]));

        let use_case = DashboardUseCase::new(
            Box::new(allowing_connection_service(context.org_id)),
            Box::new(query_events),
            Box::new(MockStorageSampleService::new()),
            Box::new(MockQueryShapeService::new()),
        );

        let events = use_case
            .recent_failures(
                context,
                connection_id,
                ListOptions {
                    sort: SortKey::StartTime,
                    direction: SortDirection::Ascending,
                    filter: EventFilter::all(day(), true),
                    ..options()
                },
            )
            .await
            .unwrap();

        assert_eq!(events, vec![]);
    }
}
