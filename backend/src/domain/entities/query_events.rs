use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::error::{Error, Result};

/// The user agent DuckWatch sets on its own MotherDuck connections, so its
/// polling traffic can be told apart from customer queries.
pub const DUCKWATCH_USER_AGENT: &str = "duckwatch";

/// Leading comment DuckWatch puts on every statement it sends to MotherDuck,
/// so its own traffic is recognizable from the query text alone. MotherDuck
/// returns it intact in `query_history`.
///
/// This and the user agent below are independent signals, and either is
/// enough. The user agent depends on MotherDuck continuing to report a
/// client's custom agent, which is undocumented; the marker depends only on
/// what DuckWatch itself sends. Both name DuckWatch specifically, so unlike
/// matching the metadata schema neither can catch a person querying their own
/// query history.
pub const DUCKWATCH_SQL_MARKER: &str = "-- duckwatch";

/// One query from MotherDuck's history, as stored per connection.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct QueryEvent {
    pub connection_id: Uuid,
    pub md_query_id: Uuid,
    pub query_text: String,
    pub query_type: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub execution_time_ms: Option<i64>,
    pub wait_time_ms: Option<i64>,
    pub total_elapsed_time_ms: Option<i64>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub user_name: Option<String>,
    pub instance_type: Option<String>,
    pub duckling_id: Option<String>,
    pub session_name: Option<String>,
    pub bytes_uploaded: Option<i64>,
    pub bytes_downloaded: Option<i64>,
    pub bytes_spilled_to_disk: Option<i64>,
    /// The client that ran the query, as MotherDuck reports it. Stored so the
    /// internal classification below can be checked rather than trusted.
    pub user_agent: Option<String>,
    /// True for DuckWatch's own polling traffic, hidden on the dashboard by
    /// default.
    pub is_internal: bool,
    /// Assigned by ingestion once the statement has been analyzed, so rows
    /// read before that step ran carry `None`.
    pub fingerprint: Option<String>,
    pub ingested_at: DateTime<Utc>,
    /// Derived from the connection's region tier when a dashboard reads the
    /// event, so it is never stored and is `None` straight from the store.
    #[serde(default)]
    pub estimated_cost_usd: Option<f64>,
}

/// A query history row as fetched from MotherDuck, before it is attached to a
/// connection. The MotherDuck client produces these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryEventDraft {
    pub md_query_id: Uuid,
    pub query_text: String,
    pub query_type: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub execution_time_ms: Option<i64>,
    pub wait_time_ms: Option<i64>,
    pub total_elapsed_time_ms: Option<i64>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub user_name: Option<String>,
    pub instance_type: Option<String>,
    pub duckling_id: Option<String>,
    pub session_name: Option<String>,
    pub bytes_uploaded: Option<i64>,
    pub bytes_downloaded: Option<i64>,
    pub bytes_spilled_to_disk: Option<i64>,
    /// As reported by MotherDuck, and what DuckWatch's own traffic is
    /// recognized by.
    pub user_agent: Option<String>,
}

impl QueryEventDraft {
    /// DuckWatch traffic is recognized two ways, and either is enough: the
    /// user agent MotherDuck reports, and the marker comment DuckWatch writes
    /// into its own statements. Both identify DuckWatch specifically, so a
    /// person querying the metadata schema themselves stays visible.
    fn is_internal(&self) -> bool {
        let tagged = self
            .user_agent
            .as_deref()
            .is_some_and(|agent| agent.to_lowercase().contains(DUCKWATCH_USER_AGENT));
        tagged || self.query_text.contains(DUCKWATCH_SQL_MARKER)
    }

    pub fn into_event(self, connection_id: Uuid, ingested_at: DateTime<Utc>) -> QueryEvent {
        let is_internal = self.is_internal();
        QueryEvent {
            connection_id,
            md_query_id: self.md_query_id,
            query_text: self.query_text,
            query_type: self.query_type,
            start_time: self.start_time,
            end_time: self.end_time,
            execution_time_ms: self.execution_time_ms,
            wait_time_ms: self.wait_time_ms,
            total_elapsed_time_ms: self.total_elapsed_time_ms,
            error_type: self.error_type,
            error_message: self.error_message,
            user_name: self.user_name,
            instance_type: self.instance_type,
            duckling_id: self.duckling_id,
            session_name: self.session_name,
            bytes_uploaded: self.bytes_uploaded,
            bytes_downloaded: self.bytes_downloaded,
            bytes_spilled_to_disk: self.bytes_spilled_to_disk,
            user_agent: self.user_agent,
            is_internal,
            fingerprint: None,
            ingested_at,
            estimated_cost_usd: None,
        }
    }
}

/// Server-side ordering for the query lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    StartTime,
    Duration,
}

impl SortKey {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "started" => Ok(Self::StartTime),
            "duration" => Ok(Self::Duration),
            _ => Err(Error::validation("sort must be started or duration")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "asc" => Ok(Self::Ascending),
            "desc" => Ok(Self::Descending),
            _ => Err(Error::validation("dir must be asc or desc")),
        }
    }
}

/// Which events a dashboard read covers. The lists, the summary, and the
/// chart all take this, so the numbers on screen always describe the same set
/// of queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFilter {
    pub range: TimeRange,
    /// Whether DuckWatch's own polling traffic counts.
    pub include_internal: bool,
    /// Case-insensitive substring match on the query text.
    pub search: Option<String>,
    /// Exact match on the MotherDuck user that ran the query.
    pub user_name: Option<String>,
    /// Exact match on the query category (QUERY, DDL, DML, and so on).
    pub query_type: Option<String>,
    /// Only events that ran at least this long.
    pub min_duration_ms: Option<i64>,
    /// Only runs of one query shape.
    pub fingerprint: Option<String>,
}

impl EventFilter {
    /// The whole range with nothing filtered out. The web layer always builds
    /// a filter from the request, so today only tests construct this.
    #[cfg(test)]
    pub fn all(range: TimeRange, include_internal: bool) -> Self {
        Self {
            range,
            include_internal,
            search: None,
            user_name: None,
            query_type: None,
            min_duration_ms: None,
            fingerprint: None,
        }
    }
}

/// What one dashboard list request asks the store for: which events, then how
/// many and in what order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventQuery {
    pub filter: EventFilter,
    pub limit: u32,
    pub sort: SortKey,
    pub direction: SortDirection,
    pub failures_only: bool,
}

/// What a cost attribution groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionKey {
    /// The MotherDuck user that ran the query.
    User,
    /// The Duckling size the query ran on.
    InstanceType,
}

/// One group and Duckling size pair as the store counts it. Pricing needs the
/// size, so the store groups by both and the application folds the pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionCell {
    pub key: String,
    pub instance_type: String,
    pub query_count: i64,
    pub failure_count: i64,
    pub total_ms: i64,
}

/// One chart bucket and Duckling size pair, which the application prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostBucketCell {
    pub bucket_start: DateTime<Utc>,
    pub instance_type: String,
    pub query_count: i64,
    pub total_ms: i64,
}

/// What one user or Duckling size accounted for in the period.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct AttributionRow {
    pub key: String,
    pub query_count: i64,
    pub failure_count: i64,
    pub total_ms: i64,
    pub estimated_cost_usd: f64,
    /// Share of the period's estimated cost, between 0 and 1.
    pub cost_share: f64,
    /// The same group's cost over the preceding period of equal length, so a
    /// reader can see whether spend is moving.
    pub previous_cost_usd: f64,
}

/// Cost attribution for one connection and period, both ways of slicing it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Attribution {
    pub by_user: Vec<AttributionRow>,
    pub by_instance_type: Vec<AttributionRow>,
    pub estimated_cost_usd: f64,
    pub previous_cost_usd: f64,
}

/// The values a dashboard can offer as filter choices for one connection and
/// window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct FilterValues {
    pub user_names: Vec<String>,
    pub query_types: Vec<String>,
}

/// The dashboard's lookback window. Parsing lives here so the web layer only
/// forwards strings and every consumer shares one definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWindow {
    Hour,
    Day,
    Week,
    Month,
}

impl TimeWindow {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "1h" => Ok(Self::Hour),
            "24h" => Ok(Self::Day),
            "7d" => Ok(Self::Week),
            "30d" => Ok(Self::Month),
            _ => Err(Error::validation(
                "window must be one of 1h, 24h, 7d, or 30d",
            )),
        }
    }

    pub fn duration(self) -> Duration {
        match self {
            Self::Hour => Duration::hours(1),
            Self::Day => Duration::hours(24),
            Self::Week => Duration::days(7),
            Self::Month => Duration::days(30),
        }
    }

    /// Bucket width that yields about sixty points per window. Reads go
    /// through `TimeRange::bucket_size`; this documents the preset widths and
    /// keeps them pinned by tests.
    #[cfg(test)]
    pub fn bucket_size(self) -> Duration {
        TimeRange {
            start: DateTime::UNIX_EPOCH,
            end: DateTime::UNIX_EPOCH + self.duration(),
        }
        .bucket_size()
    }
}

/// An explicit lookback span. Presets build one, and a caller supplied
/// from/to pair builds one directly, so every read below takes the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// A range wider than this is refused, since the chart and the aggregates
/// would scan far more than a dashboard needs.
pub const MAX_RANGE_DAYS: i64 = 90;

/// Bucket widths the latency chart may use, smallest first.
const BUCKET_LADDER_SECONDS: [i64; 12] = [
    60,      // 1 minute
    300,     // 5 minutes
    900,     // 15 minutes
    1_800,   // 30 minutes
    3_600,   // 1 hour
    10_800,  // 3 hours
    21_600,  // 6 hours
    43_200,  // 12 hours
    86_400,  // 1 day
    172_800, // 2 days
    259_200, // 3 days
    604_800, // 1 week
];

/// About this many points make a readable chart.
const TARGET_BUCKETS: i64 = 60;

impl TimeRange {
    /// Builds the range a preset window covers, ending now.
    pub fn from_window(window: TimeWindow, now: DateTime<Utc>) -> Self {
        Self {
            start: now - window.duration(),
            end: now,
        }
    }

    /// Builds an explicit range, rejecting an empty or oversized one.
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self> {
        if start >= end {
            return Err(Error::validation("from must be before to"));
        }
        if end - start > Duration::days(MAX_RANGE_DAYS) {
            return Err(Error::validation(format!(
                "the range must be at most {MAX_RANGE_DAYS} days"
            )));
        }
        Ok(Self { start, end })
    }

    pub fn duration(&self) -> Duration {
        self.end - self.start
    }

    /// The equally long period ending where this one starts.
    pub fn previous(&self) -> Self {
        let span = self.duration();
        Self {
            start: self.start - span,
            end: self.start,
        }
    }

    /// The smallest ladder width that keeps the chart near `TARGET_BUCKETS`.
    pub fn bucket_size(&self) -> Duration {
        let wanted = (self.duration().num_seconds() / TARGET_BUCKETS).max(1);
        let seconds = BUCKET_LADDER_SECONDS
            .into_iter()
            .find(|candidate| *candidate >= wanted)
            .unwrap_or(BUCKET_LADDER_SECONDS[BUCKET_LADDER_SECONDS.len() - 1]);
        Duration::seconds(seconds)
    }
}

/// One latency chart bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct LatencyBucket {
    pub bucket_start: DateTime<Utc>,
    pub query_count: i64,
    pub failure_count: i64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    /// Priced by the application from the connection's region tier.
    #[serde(default)]
    pub estimated_cost_usd: f64,
}

/// Query counts per Duckling instance type, for compute attribution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct InstanceTypeCount {
    pub instance_type: String,
    pub query_count: i64,
    /// Total run time attributed to this Duckling size in the range.
    pub total_ms: i64,
    pub estimated_cost_usd: f64,
}

/// The dashboard's headline numbers for one connection and window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct DashboardSummary {
    pub query_count: i64,
    pub failure_count: i64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub instance_types: Vec<InstanceTypeCount>,
    /// The sum of the per instance type estimates; see `RegionTier` for what
    /// the estimate does and does not cover.
    pub estimated_cost_usd: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample_draft(start_time: DateTime<Utc>) -> QueryEventDraft {
        QueryEventDraft {
            md_query_id: Uuid::new_v4(),
            query_text: "select 1".to_string(),
            query_type: Some("QUERY".to_string()),
            start_time,
            end_time: Some(start_time),
            execution_time_ms: Some(5),
            wait_time_ms: Some(1),
            total_elapsed_time_ms: Some(6),
            error_type: None,
            error_message: None,
            user_name: Some("alice".to_string()),
            instance_type: Some("pulse".to_string()),
            duckling_id: None,
            session_name: None,
            bytes_uploaded: None,
            bytes_downloaded: None,
            bytes_spilled_to_disk: None,
            user_agent: Some("duckdb/1.5.2".to_string()),
        }
    }

    #[test]
    fn into_event_attaches_the_connection() {
        let now = Utc::now();
        let draft = sample_draft(now);
        let id = draft.md_query_id;
        let connection_id = Uuid::new_v4();

        let event = draft.into_event(connection_id, now);

        assert_eq!(event.connection_id, connection_id);
        assert_eq!(event.md_query_id, id);
        assert_eq!(event.ingested_at, now);
    }

    #[test]
    fn a_customer_query_is_not_internal() {
        let event = sample_draft(Utc::now()).into_event(Uuid::new_v4(), Utc::now());
        assert!(!event.is_internal);
    }

    #[test]
    fn a_duckwatch_tagged_query_is_internal() {
        let mut draft = sample_draft(Utc::now());
        draft.user_agent = Some("duckdb/1.5.2 linux duckwatch".to_string());
        assert!(draft.into_event(Uuid::new_v4(), Utc::now()).is_internal);
    }

    #[test]
    fn a_marked_statement_is_internal_without_a_user_agent() {
        // Either signal alone has to be enough, so that DuckWatch's own
        // polling stays out of the figures even if MotherDuck stops reporting
        // custom user agents.
        let mut draft = sample_draft(Utc::now());
        draft.user_agent = None;
        draft.query_text =
            "-- duckwatch\n select query_id from md_information_schema.query_history".to_string();
        assert!(draft.into_event(Uuid::new_v4(), Utc::now()).is_internal);
    }

    #[test]
    fn a_person_querying_their_own_history_stays_visible() {
        // The rule this replaced hid every query that mentioned the metadata
        // schema, including hand written ones. Exploring your own query
        // history is exactly the sort of thing a DuckWatch user does, and it
        // is their query, not DuckWatch's.
        let mut draft = sample_draft(Utc::now());
        draft.user_agent = Some("duckdb/v1.5.5(linux_amd64)".to_string());
        draft.query_text = "with params as (select 1) \
             select user_name, count(*) from md_information_schema.query_history group by 1"
            .to_string();
        assert!(!draft.into_event(Uuid::new_v4(), Utc::now()).is_internal);
    }

    #[test]
    fn the_marker_is_recognized_wherever_it_sits() {
        // The history statement is assembled with clauses appended, and a
        // stored copy may have been reformatted on the way through.
        for text in [
            "-- duckwatch\nselect 1",
            "  -- duckwatch\n select 1 from t",
            "-- duckwatch select 1",
        ] {
            let mut draft = sample_draft(Utc::now());
            draft.user_agent = None;
            draft.query_text = text.to_string();
            assert!(
                draft.into_event(Uuid::new_v4(), Utc::now()).is_internal,
                "not recognized: {text}"
            );
        }
    }

    #[test]
    fn sort_parsing_accepts_the_known_values() {
        assert_eq!(SortKey::parse("started").unwrap(), SortKey::StartTime);
        assert_eq!(SortKey::parse("duration").unwrap(), SortKey::Duration);
        assert!(SortKey::parse("user").is_err());
        assert_eq!(
            SortDirection::parse("asc").unwrap(),
            SortDirection::Ascending
        );
        assert_eq!(
            SortDirection::parse("desc").unwrap(),
            SortDirection::Descending
        );
        assert!(SortDirection::parse("up").is_err());
    }

    #[test]
    fn parse_accepts_the_three_windows() {
        assert_eq!(TimeWindow::parse("1h").unwrap(), TimeWindow::Hour);
        assert_eq!(TimeWindow::parse("24h").unwrap(), TimeWindow::Day);
        assert_eq!(TimeWindow::parse("7d").unwrap(), TimeWindow::Week);
        assert_eq!(TimeWindow::parse("30d").unwrap(), TimeWindow::Month);
    }

    #[test]
    fn parse_rejects_an_unknown_window() {
        assert!(matches!(
            TimeWindow::parse("2h").unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn new_rejects_an_empty_or_backwards_range() {
        let now = Utc::now();
        assert!(TimeRange::new(now, now).is_err());
        assert!(TimeRange::new(now, now - Duration::hours(1)).is_err());
        assert!(TimeRange::new(now - Duration::hours(1), now).is_ok());
    }

    #[test]
    fn new_rejects_an_oversized_range() {
        let now = Utc::now();
        assert!(TimeRange::new(now - Duration::days(MAX_RANGE_DAYS), now).is_ok());
        assert!(TimeRange::new(now - Duration::days(MAX_RANGE_DAYS + 1), now).is_err());
    }

    #[test]
    fn previous_is_the_adjoining_period_of_equal_length() {
        let now = Utc::now();
        let range = TimeRange::new(now - Duration::hours(6), now).unwrap();
        let previous = range.previous();

        assert_eq!(previous.end, range.start);
        assert_eq!(previous.duration(), range.duration());
    }

    #[test]
    fn a_custom_range_buckets_like_a_matching_preset() {
        let now = Utc::now();
        let range = TimeRange::new(now - Duration::hours(24), now).unwrap();
        assert_eq!(range.bucket_size(), TimeWindow::Day.bucket_size());
    }

    #[test]
    fn bucket_size_keeps_a_readable_point_count() {
        let now = Utc::now();
        for days in [1, 7, 30, MAX_RANGE_DAYS] {
            let range = TimeRange::new(now - Duration::days(days), now).unwrap();
            let buckets = range.duration().num_seconds() / range.bucket_size().num_seconds();
            assert!(
                (20..=120).contains(&buckets),
                "{days} days gave {buckets} buckets"
            );
        }
    }

    #[test]
    fn bucket_size_divides_the_window() {
        for window in [
            TimeWindow::Hour,
            TimeWindow::Day,
            TimeWindow::Week,
            TimeWindow::Month,
        ] {
            let buckets = window.duration().num_seconds() / window.bucket_size().num_seconds();
            assert!((48..=60).contains(&buckets), "{buckets} buckets");
        }
    }
}
