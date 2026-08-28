use axum::http::{HeaderValue, Method, header};
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tower_http::cors::CorsLayer;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    /// Comma-separated list of allowed origins, or `*` to allow any origin.
    #[serde(default = "default_cors_origins")]
    pub cors_origins: Vec<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_pretty")]
    pub log_pretty: bool,
    pub database_url: String,
    #[serde(default = "default_database_max_connections")]
    pub database_max_connections: u32,
    /// How long to wait for a connection before giving up. It bounds the
    /// startup migration step, so an unreachable database fails quickly.
    #[serde(default = "default_database_acquire_timeout_seconds")]
    pub database_acquire_timeout_seconds: u64,
    /// Lifetime of a login session; the default is 30 days.
    #[serde(default = "default_session_ttl_hours")]
    pub session_ttl_hours: i64,
    /// Base64 encoded 32-byte key for encrypting stored MotherDuck tokens.
    /// Losing it orphans every stored token, so back it up with the database.
    pub token_encryption_key: String,
    /// How often the poller syncs every enabled connection.
    #[serde(default = "default_ingest_poll_interval_seconds")]
    pub ingest_poll_interval_seconds: u64,
    /// How far behind the watermark each fetch restarts, to catch rows that
    /// reach MotherDuck's history view late.
    #[serde(default = "default_ingest_overlap_minutes")]
    pub ingest_overlap_minutes: i64,
    /// Maximum query history rows fetched per connection per pass.
    #[serde(default = "default_ingest_batch_limit")]
    pub ingest_batch_limit: u32,
    /// How many already stored queries each pass fingerprints, until the
    /// history recorded before analysis existed has caught up.
    #[serde(default = "default_ingest_backfill_limit")]
    pub ingest_backfill_limit: u32,
    /// How often storage is re-read, in seconds. MotherDuck recomputes its
    /// storage view every one to six hours, so reading it on the query
    /// interval bills the account for hundreds of identical reads a day.
    #[serde(default = "default_ingest_storage_interval_seconds")]
    pub ingest_storage_interval_seconds: i64,
}

impl Config {
    /// Reads the configuration from the process environment.
    pub fn from_env() -> Result<Self, envy::Error> {
        envy::from_env::<Config>()
    }

    /// How long a connection may go without a successful sync before the
    /// dashboard calls it stale. A single missed poll is not news, so this
    /// allows several, and never trips in under five minutes however often
    /// the poller is configured to run.
    pub fn ingest_stale_after(&self) -> chrono::Duration {
        let seconds = self.ingest_poll_interval_seconds.saturating_mul(5).max(300);
        chrono::Duration::seconds(seconds.min(i64::MAX as u64) as i64)
    }

    /// Builds the connection pool without waiting for the database to accept a
    /// connection, so startup does not depend on the database being ready yet.
    pub fn get_pg_pool(&self) -> Result<PgPool, sqlx::Error> {
        PgPoolOptions::new()
            .max_connections(self.database_max_connections)
            .acquire_timeout(Duration::from_secs(self.database_acquire_timeout_seconds))
            .connect_lazy(&self.database_url)
    }

    pub fn get_cors_layer(&self) -> CorsLayer {
        let cors = CorsLayer::new()
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE]);

        match self.cors_origins.iter().any(|origin| origin == "*") {
            true => cors.allow_origin(tower_http::cors::Any),
            false => {
                let origins: Vec<HeaderValue> = self
                    .cors_origins
                    .iter()
                    .filter_map(|origin| origin.parse::<HeaderValue>().ok())
                    .collect();
                cors.allow_origin(origins)
            }
        }
    }
}

fn default_port() -> u16 {
    8080
}

/// An hour matches the fastest refresh MotherDuck documents, so the figures
/// are never more than one refresh behind.
fn default_ingest_storage_interval_seconds() -> i64 {
    3600
}

fn default_cors_origins() -> Vec<String> {
    vec![]
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_pretty() -> bool {
    false
}

fn default_database_max_connections() -> u32 {
    5
}

fn default_database_acquire_timeout_seconds() -> u64 {
    10
}

fn default_session_ttl_hours() -> i64 {
    720
}

fn default_ingest_poll_interval_seconds() -> u64 {
    60
}

fn default_ingest_overlap_minutes() -> i64 {
    15
}

fn default_ingest_batch_limit() -> u32 {
    5000
}

fn default_ingest_backfill_limit() -> u32 {
    1000
}
