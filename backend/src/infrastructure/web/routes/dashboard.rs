use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::application::use_cases::auth::AuthContext;
use crate::application::use_cases::dashboard::ListOptions;
use crate::domain::entities::query_events::{
    EventFilter, SortDirection, SortKey, TimeRange, TimeWindow,
};
use crate::domain::error::Error;
use crate::infrastructure::web::State as AppState;

const DEFAULT_LIST_LIMIT: u32 = 20;
const SEARCH_MAX_LEN: usize = 200;

#[derive(Debug, Deserialize)]
struct DashboardQuery {
    connection_id: Uuid,
    #[serde(default = "default_window")]
    window: String,
    limit: Option<u32>,
    /// Whether DuckWatch's own polling queries are included; off by default.
    #[serde(default)]
    internal: bool,
    sort: Option<String>,
    dir: Option<String>,
    /// Case-insensitive substring search over the query text.
    q: Option<String>,
    /// Exact MotherDuck user name.
    user: Option<String>,
    /// Exact query category (QUERY, DDL, DML, and so on).
    #[serde(rename = "type")]
    category: Option<String>,
    /// Minimum run time in milliseconds.
    min_ms: Option<i64>,
    /// Only runs of one query shape.
    shape: Option<String>,
    /// Start of an explicit range, RFC 3339. Overrides `window` with `to`.
    from: Option<DateTime<Utc>>,
    /// End of an explicit range, RFC 3339; defaults to now when only `from`
    /// is given.
    to: Option<DateTime<Utc>>,
}

/// Trims a text filter; blank means absent.
fn text_filter(raw: &Option<String>) -> Option<String> {
    raw.as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn default_window() -> String {
    "24h".to_string()
}

impl DashboardQuery {
    /// An explicit `from` wins over the preset window, so the presets stay
    /// the quick path and a custom range stays exact.
    fn range(&self) -> Result<TimeRange, Error> {
        match (self.from, self.to) {
            (None, None) => Ok(TimeRange::from_window(
                TimeWindow::parse(&self.window)?,
                Utc::now(),
            )),
            (from, to) => TimeRange::new(
                from.ok_or_else(|| Error::validation("from is required when to is given"))?,
                to.unwrap_or_else(Utc::now),
            ),
        }
    }

    /// Which events every read on this request covers, so the tiles, the
    /// chart, and the tables always describe the same set of queries.
    fn filter(&self) -> Result<EventFilter, Error> {
        let search = text_filter(&self.q);
        if let Some(ref value) = search
            && value.chars().count() > SEARCH_MAX_LEN
        {
            return Err(Error::validation(format!(
                "q must be at most {SEARCH_MAX_LEN} characters"
            )));
        }
        if self.min_ms.is_some_and(|value| value < 0) {
            return Err(Error::validation("min_ms must not be negative"));
        }

        Ok(EventFilter {
            range: self.range()?,
            include_internal: self.internal,
            search,
            user_name: text_filter(&self.user),
            query_type: text_filter(&self.category),
            min_duration_ms: self.min_ms,
            fingerprint: text_filter(&self.shape),
        })
    }

    /// The list endpoints differ only in their default ordering, passed here.
    fn list_options(&self, default_sort: SortKey) -> Result<ListOptions, Error> {
        let sort = match self.sort.as_deref() {
            Some(raw) => SortKey::parse(raw)?,
            None => default_sort,
        };
        let direction = match self.dir.as_deref() {
            Some(raw) => SortDirection::parse(raw)?,
            None => SortDirection::Descending,
        };
        Ok(ListOptions {
            filter: self.filter()?,
            limit: self.limit.unwrap_or(DEFAULT_LIST_LIMIT),
            sort,
            direction,
        })
    }
}

async fn get_summary(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let summary = state
        .dashboard
        .summary(context, query.connection_id, query.filter()?)
        .await?;
    Ok((StatusCode::OK, Json(summary)))
}

async fn get_latency(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let buckets = state
        .dashboard
        .latency_buckets(context, query.connection_id, query.filter()?)
        .await?;
    Ok((StatusCode::OK, Json(buckets)))
}

async fn get_slow_queries(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let options = query.list_options(SortKey::Duration)?;
    let events = state
        .dashboard
        .top_slow(context, query.connection_id, options)
        .await?;
    Ok((StatusCode::OK, Json(events)))
}

async fn get_failures(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let options = query.list_options(SortKey::StartTime)?;
    let events = state
        .dashboard
        .recent_failures(context, query.connection_id, options)
        .await?;
    Ok((StatusCode::OK, Json(events)))
}

#[derive(Debug, Deserialize)]
struct ConnectionQuery {
    connection_id: Uuid,
}

async fn get_shapes(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let shapes = state
        .dashboard
        .shapes(
            context,
            query.connection_id,
            query.filter()?,
            query.limit.unwrap_or(DEFAULT_LIST_LIMIT),
        )
        .await?;
    Ok((StatusCode::OK, Json(shapes)))
}

#[derive(Debug, Deserialize)]
struct ShapeQuery {
    connection_id: Uuid,
    fingerprint: String,
}

async fn get_shape_statement(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<ShapeQuery>,
) -> Result<impl IntoResponse, Error> {
    let statement = state
        .dashboard
        .shape_statement(context, query.connection_id, query.fingerprint)
        .await?;
    Ok((StatusCode::OK, Json(statement)))
}

async fn get_insights(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let insights = state
        .dashboard
        .insights(
            context,
            query.connection_id,
            query.filter()?,
            query.limit.unwrap_or(DEFAULT_LIST_LIMIT),
        )
        .await?;
    Ok((StatusCode::OK, Json(insights)))
}

async fn get_storage(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<ConnectionQuery>,
) -> Result<impl IntoResponse, Error> {
    let storage = state
        .dashboard
        .storage(context, query.connection_id)
        .await?;
    Ok((StatusCode::OK, Json(storage)))
}

async fn get_attribution(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let attribution = state
        .dashboard
        .attribution(context, query.connection_id, query.filter()?)
        .await?;
    Ok((StatusCode::OK, Json(attribution)))
}

async fn get_filters(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<DashboardQuery>,
) -> Result<impl IntoResponse, Error> {
    let values = state
        .dashboard
        .filter_values(context, query.connection_id, query.range()?)
        .await?;
    Ok((StatusCode::OK, Json(values)))
}

#[derive(Debug, Deserialize)]
struct EventDetailQuery {
    connection_id: Uuid,
    query_id: Uuid,
}

async fn get_event(
    State(state): State<AppState>,
    context: AuthContext,
    Query(query): Query<EventDetailQuery>,
) -> Result<impl IntoResponse, Error> {
    let event = state
        .dashboard
        .get_event(context, query.connection_id, query.query_id)
        .await?;
    Ok((StatusCode::OK, Json(event)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/summary", get(get_summary))
        .route("/latency", get(get_latency))
        .route("/slow-queries", get(get_slow_queries))
        .route("/failures", get(get_failures))
        .route("/event", get(get_event))
        .route("/filters", get(get_filters))
        .route("/attribution", get(get_attribution))
        .route("/storage", get(get_storage))
        .route("/shapes", get(get_shapes))
        .route("/insights", get(get_insights))
        .route("/shape", get(get_shape_statement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::use_cases::dashboard::MockDashboardUseCase;
    use crate::domain::entities::query_events::DashboardSummary;
    use crate::infrastructure::web::get_mock_state_with_dashboard;
    use crate::infrastructure::web::routes::extract_body_response;

    fn context() -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            is_superadmin: false,
        }
    }

    fn query(window: &str) -> DashboardQuery {
        DashboardQuery {
            connection_id: Uuid::new_v4(),
            window: window.to_string(),
            limit: None,
            internal: false,
            sort: None,
            dir: None,
            q: None,
            user: None,
            category: None,
            min_ms: None,
            shape: None,
            from: None,
            to: None,
        }
    }

    #[tokio::test]
    async fn get_summary_returns_the_numbers() {
        let mut dashboard = MockDashboardUseCase::new();
        dashboard.expect_summary().return_once(|_, _, _| {
            Ok(DashboardSummary {
                query_count: 3,
                failure_count: 1,
                p50_ms: Some(12.0),
                p95_ms: Some(80.0),
                instance_types: vec![],
                estimated_cost_usd: 0.0,
            })
        });
        let state = get_mock_state_with_dashboard(dashboard);

        let response = get_summary(State(state), context(), Query(query("24h")))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = extract_body_response::<serde_json::Value>(response.into_body())
            .await
            .unwrap();
        assert_eq!(body["query_count"], 3);
    }

    #[tokio::test]
    async fn an_unknown_window_maps_to_422() {
        let dashboard = MockDashboardUseCase::new();
        let state = get_mock_state_with_dashboard(dashboard);

        let response = get_summary(State(state), context(), Query(query("2h")))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn slow_queries_forward_the_sort_and_internal_flags() {
        let mut dashboard = MockDashboardUseCase::new();
        dashboard
            .expect_top_slow()
            .withf(|_, _, options| {
                options.sort == SortKey::StartTime
                    && options.direction == SortDirection::Ascending
                    && options.filter.include_internal
            })
            .return_once(|_, _, _| Ok(vec![]));
        let state = get_mock_state_with_dashboard(dashboard);

        let mut request = query("24h");
        request.internal = true;
        request.sort = Some("started".into());
        request.dir = Some("asc".into());

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_custom_range_overrides_the_preset() {
        let mut dashboard = MockDashboardUseCase::new();
        let from = Utc::now() - chrono::Duration::days(3);
        dashboard
            .expect_top_slow()
            .withf(move |_, _, options| {
                (options.filter.range.start - from).num_seconds().abs() < 1
                    && options.filter.range.end > options.filter.range.start
            })
            .return_once(|_, _, _| Ok(vec![]));
        let state = get_mock_state_with_dashboard(dashboard);

        let mut request = query("1h");
        request.from = Some(from);

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_backwards_range_maps_to_422() {
        let dashboard = MockDashboardUseCase::new();
        let state = get_mock_state_with_dashboard(dashboard);

        let now = Utc::now();
        let mut request = query("24h");
        request.from = Some(now);
        request.to = Some(now - chrono::Duration::hours(1));

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn a_blank_search_is_treated_as_absent() {
        let mut dashboard = MockDashboardUseCase::new();
        dashboard
            .expect_top_slow()
            .withf(|_, _, options| options.filter.search.is_none())
            .return_once(|_, _, _| Ok(vec![]));
        let state = get_mock_state_with_dashboard(dashboard);

        let mut request = query("24h");
        request.q = Some("   ".into());

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn an_overlong_search_maps_to_422() {
        let dashboard = MockDashboardUseCase::new();
        let state = get_mock_state_with_dashboard(dashboard);

        let mut request = query("24h");
        request.q = Some("a".repeat(SEARCH_MAX_LEN + 1));

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn an_unknown_sort_maps_to_422() {
        let dashboard = MockDashboardUseCase::new();
        let state = get_mock_state_with_dashboard(dashboard);

        let mut request = query("24h");
        request.sort = Some("user".into());

        let response = get_slow_queries(State(state), context(), Query(request))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
