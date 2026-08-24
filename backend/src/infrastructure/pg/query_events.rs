use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::AssertSqlSafe;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::query_events::QueryEventService;
use crate::domain::entities::insights::Antipattern;
use crate::domain::entities::query_events::{
    AttributionCell, AttributionKey, CostBucketCell, DashboardSummary, EventFilter, EventQuery,
    FilterValues, InstanceTypeCount, LatencyBucket, QueryEvent, SortDirection, SortKey, TimeRange,
};
use crate::domain::entities::query_shapes::{ShapeCell, UnfingerprintedQuery};
use crate::domain::error::Result;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct QueryEventRow {
    connection_id: Uuid,
    md_query_id: Uuid,
    query_text: String,
    query_type: Option<String>,
    start_time: DateTime<Utc>,
    end_time: Option<DateTime<Utc>>,
    execution_time_ms: Option<i64>,
    wait_time_ms: Option<i64>,
    total_elapsed_time_ms: Option<i64>,
    error_type: Option<String>,
    error_message: Option<String>,
    user_name: Option<String>,
    instance_type: Option<String>,
    duckling_id: Option<String>,
    session_name: Option<String>,
    bytes_uploaded: Option<i64>,
    bytes_downloaded: Option<i64>,
    bytes_spilled_to_disk: Option<i64>,
    user_agent: Option<String>,
    is_internal: bool,
    fingerprint: Option<String>,
    ingested_at: DateTime<Utc>,
}

impl From<QueryEventRow> for QueryEvent {
    fn from(row: QueryEventRow) -> Self {
        QueryEvent {
            connection_id: row.connection_id,
            md_query_id: row.md_query_id,
            query_text: row.query_text,
            query_type: row.query_type,
            start_time: row.start_time,
            end_time: row.end_time,
            execution_time_ms: row.execution_time_ms,
            wait_time_ms: row.wait_time_ms,
            total_elapsed_time_ms: row.total_elapsed_time_ms,
            error_type: row.error_type,
            error_message: row.error_message,
            user_name: row.user_name,
            instance_type: row.instance_type,
            duckling_id: row.duckling_id,
            session_name: row.session_name,
            bytes_uploaded: row.bytes_uploaded,
            bytes_downloaded: row.bytes_downloaded,
            bytes_spilled_to_disk: row.bytes_spilled_to_disk,
            user_agent: row.user_agent,
            is_internal: row.is_internal,
            fingerprint: row.fingerprint,
            ingested_at: row.ingested_at,
            // The dashboard use case fills this in from the connection's tier.
            estimated_cost_usd: None,
        }
    }
}

#[derive(sqlx::FromRow)]
struct BucketRow {
    bucket_start: DateTime<Utc>,
    query_count: i64,
    failure_count: i64,
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    query_count: i64,
    failure_count: i64,
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct AttributionRow {
    key: String,
    instance_type: String,
    query_count: i64,
    failure_count: i64,
    total_ms: i64,
}

#[derive(sqlx::FromRow)]
struct ShapeCellRow {
    fingerprint: String,
    instance_type: String,
    example_sql: Option<String>,
    runs: i64,
    failure_count: i64,
    total_ms: i64,
    max_ms: i64,
    bytes_spilled: i64,
    antipatterns: Option<Vec<String>>,
    last_seen: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct UnfingerprintedRow {
    md_query_id: Uuid,
    query_text: String,
    start_time: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct CostBucketRow {
    bucket_start: DateTime<Utc>,
    instance_type: String,
    query_count: i64,
    total_ms: i64,
}

#[derive(sqlx::FromRow)]
struct InstanceTypeRow {
    instance_type: String,
    query_count: i64,
    total_ms: i64,
}

/// The filter predicates every read shares, as bind placeholders $2 through
/// $7. `bind_filter` below supplies them in this order.
const FILTER_PREDICATES: &str = "e.start_time >= $2 and e.start_time < $3
               and ($4 or not e.is_internal)
               and ($5::text is null or e.query_text ilike $5)
               and ($6::text is null or e.user_name = $6)
               and ($7::text is null or e.query_type = $7)
               and ($8::bigint is null or e.total_elapsed_time_ms >= $8)
               and ($9::text is null or e.fingerprint = $9)";

/// Escapes the ilike wildcard characters, so a search for a literal `%` or
/// `_` matches those characters instead of everything.
fn like_pattern(search: &str) -> String {
    let escaped = search
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// Supplies the values `FILTER_PREDICATES` expects, as $2 through $9. Any
/// further bind a caller adds therefore starts at $10.
fn bind_filter<'q, T>(
    query: sqlx::query::QueryAs<'q, sqlx::Postgres, T, sqlx::postgres::PgArguments>,
    filter: &EventFilter,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, T, sqlx::postgres::PgArguments> {
    query
        .bind(filter.range.start)
        .bind(filter.range.end)
        .bind(filter.include_internal)
        .bind(filter.search.as_deref().map(like_pattern))
        .bind(filter.user_name.clone())
        .bind(filter.query_type.clone())
        .bind(filter.min_duration_ms)
        .bind(filter.fingerprint.clone())
}

pub struct PgQueryEventService {
    db: PgPool,
}

impl PgQueryEventService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl QueryEventService for PgQueryEventService {
    async fn upsert_batch(&self, events: Vec<QueryEvent>) -> Result<u64> {
        if events.is_empty() {
            return Ok(0);
        }

        // One multi-row statement via unnest, so a poll cycle costs a single
        // round trip regardless of batch size.
        let mut connection_ids = Vec::with_capacity(events.len());
        let mut md_query_ids = Vec::with_capacity(events.len());
        let mut query_texts = Vec::with_capacity(events.len());
        let mut query_types = Vec::with_capacity(events.len());
        let mut start_times = Vec::with_capacity(events.len());
        let mut end_times = Vec::with_capacity(events.len());
        let mut execution_times = Vec::with_capacity(events.len());
        let mut wait_times = Vec::with_capacity(events.len());
        let mut total_elapsed_times = Vec::with_capacity(events.len());
        let mut error_types = Vec::with_capacity(events.len());
        let mut error_messages = Vec::with_capacity(events.len());
        let mut user_names = Vec::with_capacity(events.len());
        let mut instance_types = Vec::with_capacity(events.len());
        let mut duckling_ids = Vec::with_capacity(events.len());
        let mut session_names = Vec::with_capacity(events.len());
        let mut bytes_uploaded = Vec::with_capacity(events.len());
        let mut bytes_downloaded = Vec::with_capacity(events.len());
        let mut bytes_spilled = Vec::with_capacity(events.len());
        let mut user_agents = Vec::with_capacity(events.len());
        let mut is_internals = Vec::with_capacity(events.len());
        let mut fingerprints = Vec::with_capacity(events.len());
        let mut ingested_ats = Vec::with_capacity(events.len());

        for event in events {
            connection_ids.push(event.connection_id);
            md_query_ids.push(event.md_query_id);
            query_texts.push(event.query_text);
            query_types.push(event.query_type);
            start_times.push(event.start_time);
            end_times.push(event.end_time);
            execution_times.push(event.execution_time_ms);
            wait_times.push(event.wait_time_ms);
            total_elapsed_times.push(event.total_elapsed_time_ms);
            error_types.push(event.error_type);
            error_messages.push(event.error_message);
            user_names.push(event.user_name);
            instance_types.push(event.instance_type);
            duckling_ids.push(event.duckling_id);
            session_names.push(event.session_name);
            bytes_uploaded.push(event.bytes_uploaded);
            bytes_downloaded.push(event.bytes_downloaded);
            bytes_spilled.push(event.bytes_spilled_to_disk);
            user_agents.push(event.user_agent);
            is_internals.push(event.is_internal);
            fingerprints.push(event.fingerprint);
            ingested_ats.push(event.ingested_at);
        }

        let result = sqlx::query(
            "insert into query_events (connection_id, md_query_id, query_text, query_type,
                 start_time, end_time, execution_time_ms, wait_time_ms, total_elapsed_time_ms,
                 error_type, error_message, user_name, instance_type, duckling_id, session_name,
                 bytes_uploaded, bytes_downloaded, bytes_spilled_to_disk, user_agent,
                 is_internal, fingerprint, ingested_at)
             select * from unnest(
                 $1::uuid[], $2::uuid[], $3::text[], $4::varchar[],
                 $5::timestamptz[], $6::timestamptz[], $7::bigint[], $8::bigint[], $9::bigint[],
                 $10::varchar[], $11::text[], $12::varchar[], $13::varchar[], $14::varchar[],
                 $15::varchar[], $16::bigint[], $17::bigint[], $18::bigint[], $19::varchar[],
                 $20::bool[], $21::varchar[], $22::timestamptz[])
             on conflict (connection_id, md_query_id) do update set
                 end_time = excluded.end_time,
                 execution_time_ms = excluded.execution_time_ms,
                 wait_time_ms = excluded.wait_time_ms,
                 total_elapsed_time_ms = excluded.total_elapsed_time_ms,
                 user_agent = excluded.user_agent,
                 error_type = excluded.error_type,
                 error_message = excluded.error_message,
                 is_internal = excluded.is_internal,
                 -- a re-read keeps the fingerprint already assigned
                 fingerprint = coalesce(excluded.fingerprint, query_events.fingerprint),
                 ingested_at = excluded.ingested_at",
        )
        .bind(&connection_ids)
        .bind(&md_query_ids)
        .bind(&query_texts)
        .bind(&query_types)
        .bind(&start_times)
        .bind(&end_times)
        .bind(&execution_times)
        .bind(&wait_times)
        .bind(&total_elapsed_times)
        .bind(&error_types)
        .bind(&error_messages)
        .bind(&user_names)
        .bind(&instance_types)
        .bind(&duckling_ids)
        .bind(&session_names)
        .bind(&bytes_uploaded)
        .bind(&bytes_downloaded)
        .bind(&bytes_spilled)
        .bind(&user_agents)
        .bind(&is_internals)
        .bind(&fingerprints)
        .bind(&ingested_ats)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    async fn summary(&self, connection_id: Uuid, filter: EventFilter) -> Result<DashboardSummary> {
        let totals = sqlx::query_as::<_, SummaryRow>(AssertSqlSafe(format!(
            "select count(*) as query_count,
                    count(*) filter (where e.error_type is not null) as failure_count,
                    percentile_cont(0.5) within group (order by e.total_elapsed_time_ms) as p50_ms,
                    percentile_cont(0.95) within group (order by e.total_elapsed_time_ms) as p95_ms
             from query_events e
             where e.connection_id = $1 and {FILTER_PREDICATES}"
        )));
        let totals = bind_filter(totals.bind(connection_id), &filter)
            .fetch_one(&self.db)
            .await?;

        let instance_types = sqlx::query_as::<_, InstanceTypeRow>(AssertSqlSafe(format!(
            "select coalesce(e.instance_type, 'unknown') as instance_type,
                    count(*) as query_count,
                    coalesce(sum(e.total_elapsed_time_ms), 0)::bigint as total_ms
             from query_events e
             where e.connection_id = $1 and {FILTER_PREDICATES}
             group by 1
             order by query_count desc"
        )));
        let instance_types = bind_filter(instance_types.bind(connection_id), &filter)
            .fetch_all(&self.db)
            .await?;

        Ok(DashboardSummary {
            query_count: totals.query_count,
            failure_count: totals.failure_count,
            p50_ms: totals.p50_ms,
            p95_ms: totals.p95_ms,
            instance_types: instance_types
                .into_iter()
                .map(|row| InstanceTypeCount {
                    instance_type: row.instance_type,
                    query_count: row.query_count,
                    total_ms: row.total_ms,
                    // Priced by the use case, which knows the region tier.
                    estimated_cost_usd: 0.0,
                })
                .collect(),
            estimated_cost_usd: 0.0,
        })
    }

    async fn latency_buckets(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<LatencyBucket>> {
        let bucket_seconds = filter.range.bucket_size().num_seconds();

        // Every bucket in the range is generated and left joined, so a quiet
        // period shows as an empty bucket instead of vanishing and letting
        // the remaining bars imply an even spacing they do not have. The
        // bucket width follows the filter binds as $9.
        let rows = sqlx::query_as::<_, BucketRow>(AssertSqlSafe(
            "select b.bucket_start,
                    count(e.md_query_id) as query_count,
                    count(e.md_query_id) filter (where e.error_type is not null)
                        as failure_count,
                    percentile_cont(0.5) within group (order by e.total_elapsed_time_ms)
                        as p50_ms,
                    percentile_cont(0.95) within group (order by e.total_elapsed_time_ms)
                        as p95_ms
             from generate_series($2::timestamptz, $3::timestamptz, make_interval(secs => $10))
                      as b(bucket_start)
             left join query_events e
                    on e.connection_id = $1
                   and e.start_time >= b.bucket_start
                   and e.start_time < b.bucket_start + make_interval(secs => $10)
                   -- The last bucket runs past the end of the range whenever
                   -- the range is not a whole number of buckets, so the range
                   -- end has to bound the join as well. The start needs no
                   -- bound, since the series begins at it.
                   and e.start_time < $3
                   and ($4 or not e.is_internal)
                   and ($5::text is null or e.query_text ilike $5)
                   and ($6::text is null or e.user_name = $6)
                   and ($7::text is null or e.query_type = $7)
                   and ($8::bigint is null or e.total_elapsed_time_ms >= $8)
                   and ($9::text is null or e.fingerprint = $9)
             where b.bucket_start < $3
             group by 1
             order by 1",
        ));
        let rows = bind_filter(rows.bind(connection_id), &filter)
            .bind(bucket_seconds as f64)
            .fetch_all(&self.db)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| LatencyBucket {
                bucket_start: row.bucket_start,
                query_count: row.query_count,
                failure_count: row.failure_count,
                p50_ms: row.p50_ms,
                p95_ms: row.p95_ms,
                // Priced by the use case, which knows the region tier.
                estimated_cost_usd: 0.0,
            })
            .collect())
    }

    async fn cost_cells(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<CostBucketCell>> {
        let bucket_seconds = filter.range.bucket_size().num_seconds();

        let rows = sqlx::query_as::<_, CostBucketRow>(AssertSqlSafe(format!(
            "select date_bin(make_interval(secs => $10), e.start_time, $2) as bucket_start,
                    coalesce(e.instance_type, 'unknown') as instance_type,
                    count(*) as query_count,
                    coalesce(sum(e.total_elapsed_time_ms), 0)::bigint as total_ms
             from query_events e
             where e.connection_id = $1 and {FILTER_PREDICATES}
             group by 1, 2"
        )));
        let rows = bind_filter(rows.bind(connection_id), &filter)
            .bind(bucket_seconds as f64)
            .fetch_all(&self.db)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| CostBucketCell {
                bucket_start: row.bucket_start,
                instance_type: row.instance_type,
                query_count: row.query_count,
                total_ms: row.total_ms,
            })
            .collect())
    }

    async fn attribution_cells(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
        key: AttributionKey,
    ) -> Result<Vec<AttributionCell>> {
        // The grouping column comes from the fixed match below, never from a
        // caller string.
        let key_column = match key {
            AttributionKey::User => "user_name",
            AttributionKey::InstanceType => "instance_type",
        };

        let rows = sqlx::query_as::<_, AttributionRow>(AssertSqlSafe(format!(
            "select coalesce(e.{key_column}, 'unknown') as key,
                    coalesce(e.instance_type, 'unknown') as instance_type,
                    count(*) as query_count,
                    count(*) filter (where e.error_type is not null) as failure_count,
                    coalesce(sum(e.total_elapsed_time_ms), 0)::bigint as total_ms
             from query_events e
             where e.connection_id = $1 and {FILTER_PREDICATES}
             group by 1, 2"
        )));
        let rows = bind_filter(rows.bind(connection_id), &filter)
            .fetch_all(&self.db)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| AttributionCell {
                key: row.key,
                instance_type: row.instance_type,
                query_count: row.query_count,
                failure_count: row.failure_count,
                total_ms: row.total_ms,
            })
            .collect())
    }

    async fn list_events(&self, connection_id: Uuid, query: EventQuery) -> Result<Vec<QueryEvent>> {
        // Every dynamic clause comes from the fixed matches below, never from
        // caller strings, so the assembled statement is as safe as a literal.
        let order_column = match query.sort {
            SortKey::StartTime => "start_time",
            SortKey::Duration => "total_elapsed_time_ms",
        };
        let order_direction = match query.direction {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        };
        let list_filter = match query.failures_only {
            true => "and e.error_type is not null",
            // The slow list drops rows that have no duration yet.
            false => "and e.total_elapsed_time_ms is not null",
        };

        // The row limit follows the filter binds as $10.
        let sql = format!(
            "select connection_id, md_query_id, query_text, query_type, start_time, end_time,
                    execution_time_ms, wait_time_ms, total_elapsed_time_ms, error_type,
                    error_message, user_name, instance_type, duckling_id, session_name,
                    bytes_uploaded, bytes_downloaded, bytes_spilled_to_disk, user_agent,
                    is_internal,
                    fingerprint, ingested_at
             from query_events e
             where e.connection_id = $1 and {FILTER_PREDICATES}
               {list_filter}
             order by {order_column} {order_direction} nulls last
             limit $10"
        );

        let rows = sqlx::query_as::<_, QueryEventRow>(AssertSqlSafe(sql));
        let rows = bind_filter(rows.bind(connection_id), &query.filter)
            .bind(i64::from(query.limit))
            .fetch_all(&self.db)
            .await?;

        Ok(rows.into_iter().map(QueryEvent::from).collect())
    }

    async fn filter_values(&self, connection_id: Uuid, range: TimeRange) -> Result<FilterValues> {
        let user_names: Vec<(String,)> = sqlx::query_as(
            "select distinct user_name from query_events
             where connection_id = $1 and start_time >= $2 and start_time < $3
               and user_name is not null
             order by 1",
        )
        .bind(connection_id)
        .bind(range.start)
        .bind(range.end)
        .fetch_all(&self.db)
        .await?;

        let query_types: Vec<(String,)> = sqlx::query_as(
            "select distinct query_type from query_events
             where connection_id = $1 and start_time >= $2 and start_time < $3
               and query_type is not null
             order by 1",
        )
        .bind(connection_id)
        .bind(range.start)
        .bind(range.end)
        .fetch_all(&self.db)
        .await?;

        Ok(FilterValues {
            user_names: user_names.into_iter().map(|row| row.0).collect(),
            query_types: query_types.into_iter().map(|row| row.0).collect(),
        })
    }

    async fn shape_cells(
        &self,
        connection_id: Uuid,
        filter: EventFilter,
    ) -> Result<Vec<ShapeCell>> {
        // Grouped by shape and size, because pricing depends on the size.
        // The example comes from the shapes table, so the long statement text
        // is not part of the grouping key.
        let rows = sqlx::query_as::<_, ShapeCellRow>(AssertSqlSafe(format!(
            "select e.fingerprint as fingerprint,
                    coalesce(e.instance_type, 'unknown') as instance_type,
                    min(s.example_sql) as example_sql,
                    count(*) as runs,
                    count(*) filter (where e.error_type is not null) as failure_count,
                    coalesce(sum(e.total_elapsed_time_ms), 0)::bigint as total_ms,
                    coalesce(max(e.total_elapsed_time_ms), 0)::bigint as max_ms,
                    coalesce(sum(e.bytes_spilled_to_disk), 0)::bigint as bytes_spilled,
                    min(s.antipatterns) as antipatterns,
                    max(e.start_time) as last_seen
             from query_events e
             left join query_shapes s
                    on s.connection_id = e.connection_id and s.fingerprint = e.fingerprint
             where e.connection_id = $1 and e.fingerprint is not null
               and {FILTER_PREDICATES}
             group by 1, 2"
        )));
        let rows = bind_filter(rows.bind(connection_id), &filter)
            .fetch_all(&self.db)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| ShapeCell {
                fingerprint: row.fingerprint,
                instance_type: row.instance_type,
                example_sql: row.example_sql.unwrap_or_default(),
                runs: row.runs,
                failure_count: row.failure_count,
                total_ms: row.total_ms,
                max_ms: row.max_ms,
                bytes_spilled: row.bytes_spilled,
                // A name this release does not know, written by a later one,
                // is dropped rather than failing the read.
                antipatterns: row
                    .antipatterns
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|name| Antipattern::parse(name))
                    .collect(),
                last_seen: row.last_seen,
            })
            .collect())
    }

    async fn find_unfingerprinted(
        &self,
        connection_id: Uuid,
        limit: u32,
    ) -> Result<Vec<UnfingerprintedQuery>> {
        let rows = sqlx::query_as::<_, UnfingerprintedRow>(
            "select md_query_id, query_text, start_time
             from query_events
             where connection_id = $1 and fingerprint is null
             order by start_time desc
             limit $2",
        )
        .bind(connection_id)
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| UnfingerprintedQuery {
                md_query_id: row.md_query_id,
                query_text: row.query_text,
                start_time: row.start_time,
            })
            .collect())
    }

    async fn set_fingerprints(
        &self,
        connection_id: Uuid,
        assignments: Vec<(Uuid, String)>,
    ) -> Result<u64> {
        if assignments.is_empty() {
            return Ok(0);
        }

        let (ids, fingerprints): (Vec<Uuid>, Vec<String>) = assignments.into_iter().unzip();

        let result = sqlx::query(
            "update query_events e
             set fingerprint = a.fingerprint
             from unnest($2::uuid[], $3::varchar[]) as a(md_query_id, fingerprint)
             where e.connection_id = $1 and e.md_query_id = a.md_query_id",
        )
        .bind(connection_id)
        .bind(&ids)
        .bind(&fingerprints)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    async fn find_event(&self, connection_id: Uuid, md_query_id: Uuid) -> Result<QueryEvent> {
        let row = sqlx::query_as::<_, QueryEventRow>(
            "select connection_id, md_query_id, query_text, query_type, start_time, end_time,
                    execution_time_ms, wait_time_ms, total_elapsed_time_ms, error_type,
                    error_message, user_name, instance_type, duckling_id, session_name,
                    bytes_uploaded, bytes_downloaded, bytes_spilled_to_disk, user_agent,
                    is_internal,
                    fingerprint, ingested_at
             from query_events
             where connection_id = $1 and md_query_id = $2",
        )
        .bind(connection_id)
        .bind(md_query_id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use chrono::Duration;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::motherduck_connections::MotherDuckConnectionService;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::pricing::RegionTier;
    use crate::domain::entities::query_events::{AttributionKey, QueryEventDraft, TimeWindow};
    use crate::infrastructure::crypto::SecretCipher;
    use crate::infrastructure::pg::motherduck_connections::PgMotherDuckConnectionService;

    /// The range a preset window covers, ending now.
    fn range(window: TimeWindow) -> TimeRange {
        TimeRange::from_window(window, Utc::now())
    }

    /// Everything in a preset window, unfiltered.
    fn day_filter() -> EventFilter {
        EventFilter::all(range(TimeWindow::Day), false)
    }

    fn default_query() -> EventQuery {
        EventQuery {
            filter: day_filter(),
            limit: 20,
            sort: SortKey::Duration,
            direction: SortDirection::Descending,
            failures_only: false,
        }
    }

    async fn seed_connection(pool: &Pool<Postgres>) -> Uuid {
        let cipher = Arc::new(SecretCipher::from_base64_key(&STANDARD.encode([9u8; 32])).unwrap());
        let (connection, token) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(Utc::now());
        PgMotherDuckConnectionService::new(pool.clone(), cipher)
            .insert(connection, token)
            .await
            .unwrap()
            .id
    }

    fn event(
        connection_id: Uuid,
        elapsed_ms: i64,
        minutes_ago: i64,
        error: Option<&str>,
    ) -> QueryEvent {
        let start_time = Utc::now() - Duration::minutes(minutes_ago);
        QueryEventDraft {
            md_query_id: Uuid::new_v4(),
            query_text: format!("select {elapsed_ms}"),
            query_type: Some("QUERY".into()),
            start_time,
            end_time: Some(start_time),
            execution_time_ms: Some(elapsed_ms),
            wait_time_ms: Some(0),
            total_elapsed_time_ms: Some(elapsed_ms),
            error_type: error.map(str::to_string),
            error_message: error.map(|e| format!("{e} happened")),
            user_name: Some("alice".into()),
            instance_type: Some("pulse".into()),
            duckling_id: None,
            session_name: None,
            bytes_uploaded: None,
            bytes_downloaded: None,
            bytes_spilled_to_disk: None,
            user_agent: Some("duckdb/1.5.2".into()),
        }
        .into_event(connection_id, Utc::now())
    }

    fn internal_event(connection_id: Uuid, minutes_ago: i64) -> QueryEvent {
        let start_time = Utc::now() - Duration::minutes(minutes_ago);
        QueryEventDraft {
            md_query_id: Uuid::new_v4(),
            query_text: "select 1".into(),
            query_type: Some("QUERY".into()),
            start_time,
            end_time: Some(start_time),
            execution_time_ms: Some(5),
            wait_time_ms: Some(0),
            total_elapsed_time_ms: Some(5),
            error_type: None,
            error_message: None,
            user_name: Some("duckwatch-service".into()),
            instance_type: Some("pulse".into()),
            duckling_id: None,
            session_name: None,
            bytes_uploaded: None,
            bytes_downloaded: None,
            bytes_spilled_to_disk: None,
            user_agent: Some("duckdb/1.5.2 duckwatch".into()),
        }
        .into_event(connection_id, Utc::now())
    }

    #[sqlx::test]
    async fn upsert_batch_is_idempotent_and_updates_completions(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);

        let mut first = event(connection_id, 100, 5, None);
        first.end_time = None;
        first.total_elapsed_time_ms = None;
        assert_eq!(service.upsert_batch(vec![first.clone()]).await.unwrap(), 1);

        // The same query fetched again after completion updates in place.
        first.end_time = Some(Utc::now());
        first.total_elapsed_time_ms = Some(250);
        service.upsert_batch(vec![first]).await.unwrap();

        let summary = service.summary(connection_id, day_filter()).await.unwrap();
        assert_eq!(summary.query_count, 1);
        assert_eq!(summary.p50_ms, Some(250.0));
    }

    #[sqlx::test]
    async fn summary_computes_exact_percentiles(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);

        let events = (1..=100)
            .map(|i| event(connection_id, i * 10, 5, None))
            .collect();
        service.upsert_batch(events).await.unwrap();

        let summary = service.summary(connection_id, day_filter()).await.unwrap();
        assert_eq!(summary.query_count, 100);
        assert_eq!(summary.failure_count, 0);
        assert_eq!(summary.p50_ms, Some(505.0));
        // percentile_cont interpolates: 0.95 * 99 + 1 = position 95.05.
        let p95 = summary.p95_ms.unwrap();
        assert!((p95 - 950.5).abs() < 0.001, "p95 was {p95}");
        assert_eq!(summary.instance_types.len(), 1);
        assert_eq!(summary.instance_types[0].query_count, 100);
    }

    #[sqlx::test]
    async fn internal_rows_are_hidden_unless_asked_for(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        service
            .upsert_batch(vec![
                event(connection_id, 50, 5, None),
                internal_event(connection_id, 5),
            ])
            .await
            .unwrap();

        let hidden = service.summary(connection_id, day_filter()).await.unwrap();
        assert_eq!(hidden.query_count, 1);

        let shown = service
            .summary(
                connection_id,
                EventFilter::all(range(TimeWindow::Day), true),
            )
            .await
            .unwrap();
        assert_eq!(shown.query_count, 2);

        let listed = service
            .list_events(connection_id, default_query())
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].is_internal);

        let all = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter {
                        include_internal: true,
                        ..day_filter()
                    },
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    #[sqlx::test]
    async fn list_events_sorts_by_the_requested_key(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        service
            .upsert_batch(vec![
                event(connection_id, 50, 30, None),
                event(connection_id, 500, 10, None),
                event(connection_id, 5, 20, None),
            ])
            .await
            .unwrap();

        let by_duration = service
            .list_events(connection_id, default_query())
            .await
            .unwrap();
        assert_eq!(
            by_duration
                .iter()
                .map(|e| e.total_elapsed_time_ms)
                .collect::<Vec<_>>(),
            vec![Some(500), Some(50), Some(5)]
        );

        let oldest_first = service
            .list_events(
                connection_id,
                EventQuery {
                    sort: SortKey::StartTime,
                    direction: SortDirection::Ascending,
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            oldest_first
                .iter()
                .map(|e| e.total_elapsed_time_ms)
                .collect::<Vec<_>>(),
            vec![Some(50), Some(5), Some(500)]
        );
    }

    #[sqlx::test]
    async fn failures_only_returns_errors_in_the_window(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        service
            .upsert_batch(vec![
                event(connection_id, 10, 5, None),
                event(connection_id, 10, 5, Some("SYNTAX_ERROR")),
                // Outside the one hour window.
                event(connection_id, 10, 90, Some("TIMEOUT")),
            ])
            .await
            .unwrap();

        let failures = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter::all(range(TimeWindow::Hour), false),
                    failures_only: true,
                    sort: SortKey::StartTime,
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].error_type.as_deref(), Some("SYNTAX_ERROR"));
    }

    #[sqlx::test]
    async fn list_events_applies_the_filters(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let mut by_bob = event(connection_id, 900, 5, None);
        by_bob.user_name = Some("bob".into());
        by_bob.query_text = "create table sales_100% as select 1".into();
        by_bob.query_type = Some("DDL".into());
        service
            .upsert_batch(vec![event(connection_id, 40, 5, None), by_bob.clone()])
            .await
            .unwrap();

        let by_user = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter {
                        user_name: Some("bob".into()),
                        ..day_filter()
                    },
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_user.len(), 1);
        assert_eq!(by_user[0].md_query_id, by_bob.md_query_id);

        // The search escapes ilike wildcards, so "100%" matches literally.
        let by_search = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter {
                        search: Some("SALES_100%".into()),
                        ..day_filter()
                    },
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_search.len(), 1);

        let by_type = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter {
                        query_type: Some("DDL".into()),
                        ..day_filter()
                    },
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_type.len(), 1);

        let by_duration = service
            .list_events(
                connection_id,
                EventQuery {
                    filter: EventFilter {
                        min_duration_ms: Some(500),
                        ..day_filter()
                    },
                    ..default_query()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_duration.len(), 1);
        assert_eq!(by_duration[0].total_elapsed_time_ms, Some(900));
    }

    #[sqlx::test]
    async fn attribution_cells_group_by_key_and_size(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let mut bob = event(connection_id, 900, 5, Some("TIMEOUT"));
        bob.user_name = Some("bob".into());
        bob.instance_type = Some("jumbo".into());
        service
            .upsert_batch(vec![
                event(connection_id, 100, 5, None),
                event(connection_id, 200, 5, None),
                bob,
            ])
            .await
            .unwrap();

        let by_user = service
            .attribution_cells(connection_id, day_filter(), AttributionKey::User)
            .await
            .unwrap();
        let alice = by_user.iter().find(|cell| cell.key == "alice").unwrap();
        assert_eq!(alice.query_count, 2);
        assert_eq!(alice.total_ms, 300);
        assert_eq!(alice.instance_type, "pulse");

        let bob_cell = by_user.iter().find(|cell| cell.key == "bob").unwrap();
        assert_eq!(bob_cell.failure_count, 1);
        assert_eq!(bob_cell.instance_type, "jumbo");

        let by_size = service
            .attribution_cells(connection_id, day_filter(), AttributionKey::InstanceType)
            .await
            .unwrap();
        assert_eq!(by_size.len(), 2);
        assert!(by_size.iter().all(|cell| cell.key == cell.instance_type));
    }

    #[sqlx::test]
    async fn cost_cells_split_each_bucket_by_size(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let mut jumbo = event(connection_id, 500, 5, None);
        jumbo.instance_type = Some("jumbo".into());
        service
            .upsert_batch(vec![event(connection_id, 100, 5, None), jumbo])
            .await
            .unwrap();

        let cells = service
            .cost_cells(connection_id, day_filter())
            .await
            .unwrap();

        // Same bucket, one row per Duckling size.
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].bucket_start, cells[1].bucket_start);
        assert_eq!(cells.iter().map(|cell| cell.total_ms).sum::<i64>(), 600);
    }

    #[sqlx::test]
    async fn filter_values_lists_users_and_types(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let mut by_bob = event(connection_id, 10, 5, None);
        by_bob.user_name = Some("bob".into());
        by_bob.query_type = Some("DDL".into());
        service
            .upsert_batch(vec![event(connection_id, 10, 5, None), by_bob])
            .await
            .unwrap();

        let values = service
            .filter_values(connection_id, range(TimeWindow::Day))
            .await
            .unwrap();
        assert_eq!(
            values.user_names,
            vec!["alice".to_string(), "bob".to_string()]
        );
        assert_eq!(
            values.query_types,
            vec!["DDL".to_string(), "QUERY".to_string()]
        );
    }

    #[sqlx::test]
    async fn find_event_returns_the_full_row(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let stored = event(connection_id, 42, 5, Some("TIMEOUT"));
        service.upsert_batch(vec![stored.clone()]).await.unwrap();

        let found = service
            .find_event(connection_id, stored.md_query_id)
            .await
            .unwrap();
        assert_eq!(found.md_query_id, stored.md_query_id);
        assert_eq!(found.query_text, stored.query_text);
        assert_eq!(found.error_type.as_deref(), Some("TIMEOUT"));

        assert!(
            service
                .find_event(connection_id, Uuid::new_v4())
                .await
                .is_err()
        );
    }

    #[sqlx::test]
    async fn the_user_agent_round_trips_so_the_tag_can_be_audited(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        let stored = event(connection_id, 10, 5, None);
        service.upsert_batch(vec![stored.clone()]).await.unwrap();

        let found = service
            .find_event(connection_id, stored.md_query_id)
            .await
            .unwrap();
        assert_eq!(found.user_agent.as_deref(), Some("duckdb/1.5.2"));
    }

    #[sqlx::test]
    async fn a_re_read_records_the_user_agent_that_arrived_late(pool: Pool<Postgres>) {
        // MotherDuck reports the user agent minutes after a query runs, so
        // the first read of a row has none and a later pass fills it in. The
        // upsert has to carry that through, or the tag never lands.
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);

        let mut first = event(connection_id, 10, 5, None);
        first.user_agent = None;
        first.is_internal = false;
        service.upsert_batch(vec![first.clone()]).await.unwrap();

        let mut again = first.clone();
        again.user_agent = Some("duckdb/v1.5.5(linux_amd64) rust duckwatch".into());
        again.is_internal = true;
        service.upsert_batch(vec![again]).await.unwrap();

        let found = service
            .find_event(connection_id, first.md_query_id)
            .await
            .unwrap();
        assert_eq!(
            found.user_agent.as_deref(),
            Some("duckdb/v1.5.5(linux_amd64) rust duckwatch")
        );
        assert!(found.is_internal, "the late tag must reclassify the row");
    }

    #[sqlx::test]
    async fn latency_buckets_stop_at_the_end_of_the_range(pool: Pool<Postgres>) {
        // A custom range whose length is not a whole number of buckets, so
        // the final bucket runs past the range end: 62 minutes at the five
        // minute width this range selects leaves a trailing partial bucket.
        let now = Utc::now();
        let end = now - Duration::minutes(10);
        let filter = EventFilter::all(
            TimeRange::new(end - Duration::minutes(62), end).unwrap(),
            false,
        );

        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        service
            .upsert_batch(vec![
                // Inside the range.
                event(connection_id, 10, 20, None),
                // After the range ends, but inside the last bucket's width.
                event(connection_id, 10, 9, None),
                event(connection_id, 10, 8, Some("TIMEOUT")),
            ])
            .await
            .unwrap();

        let buckets = service
            .latency_buckets(connection_id, filter.clone())
            .await
            .unwrap();
        let summary = service.summary(connection_id, filter).await.unwrap();

        // The chart and the tiles describe the same range, so they must agree.
        let charted: i64 = buckets.iter().map(|bucket| bucket.query_count).sum();
        let charted_failures: i64 = buckets.iter().map(|bucket| bucket.failure_count).sum();
        assert_eq!(charted, summary.query_count, "chart counted {charted}");
        assert_eq!(charted_failures, summary.failure_count);
        assert_eq!(charted, 1);
    }

    #[sqlx::test]
    async fn latency_buckets_group_by_time(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryEventService::new(pool);
        service
            .upsert_batch(vec![
                event(connection_id, 10, 2, None),
                event(connection_id, 20, 2, None),
                event(connection_id, 30, 40, Some("TIMEOUT")),
            ])
            .await
            .unwrap();

        let buckets = service
            .latency_buckets(connection_id, day_filter())
            .await
            .unwrap();

        // A day at half hour buckets is a full grid, quiet periods included,
        // so the bars stay evenly spaced in time.
        assert_eq!(buckets.len(), 48);
        assert!(
            buckets
                .windows(2)
                .all(|pair| pair[0].bucket_start < pair[1].bucket_start),
            "buckets must run in order"
        );

        let populated: Vec<_> = buckets.iter().filter(|b| b.query_count > 0).collect();
        assert_eq!(populated.len(), 2);
        assert_eq!(populated[0].failure_count, 1);
        assert_eq!(populated[1].query_count, 2);
        assert_eq!(
            buckets.iter().map(|b| b.query_count).sum::<i64>(),
            3,
            "every event lands in exactly one bucket"
        );
    }
}
