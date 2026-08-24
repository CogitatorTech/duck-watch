use anyhow::Context;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::watch;

use crate::application::use_cases::admin::AdminUseCase;
use crate::application::use_cases::auth::AuthUseCase;
use crate::application::use_cases::connections::ConnectionsUseCase;
use crate::application::use_cases::dashboard::DashboardUseCase;
use crate::application::use_cases::ingestion::{IngestionSettings, IngestionUseCase};
use crate::config::Config;

mod argon2;
mod crypto;
mod ingest;
mod motherduck;
mod pg;
mod sql_analysis;
mod web;

pub async fn run(config: Config) -> anyhow::Result<()> {
    let pg_pool = config
        .get_pg_pool()
        .context("failed to build the database pool")?;

    sqlx::migrate!("./migrations")
        .run(&pg_pool)
        .await
        .context("failed to run database migrations")?;

    let cipher = Arc::new(
        crypto::SecretCipher::from_base64_key(&config.token_encryption_key)
            .context("failed to build the token cipher")?,
    );

    let auth = AuthUseCase::new(
        Box::new(pg::organizations::PgOrganizationService::new(
            pg_pool.clone(),
        )),
        Box::new(pg::users::PgUserService::new(pg_pool.clone())),
        Box::new(pg::sessions::PgSessionService::new(pg_pool.clone())),
        Box::new(argon2::Argon2PasswordHasher),
        chrono::Duration::hours(config.session_ttl_hours),
    );

    let connections = ConnectionsUseCase::new(
        Box::new(
            pg::motherduck_connections::PgMotherDuckConnectionService::new(
                pg_pool.clone(),
                cipher.clone(),
            ),
        ),
        Box::new(motherduck::DuckDbMotherDuckClient),
        config.ingest_stale_after(),
    );

    let dashboard = DashboardUseCase::new(
        Box::new(
            pg::motherduck_connections::PgMotherDuckConnectionService::new(
                pg_pool.clone(),
                cipher.clone(),
            ),
        ),
        Box::new(pg::query_events::PgQueryEventService::new(pg_pool.clone())),
        Box::new(pg::storage_samples::PgStorageSampleService::new(
            pg_pool.clone(),
        )),
        Box::new(pg::query_shapes::PgQueryShapeService::new(pg_pool.clone())),
    );

    let admin = AdminUseCase::new(Box::new(pg::admin::PgAdminService::new(pg_pool.clone())));

    let ingestion = IngestionUseCase::new(
        Box::new(
            pg::motherduck_connections::PgMotherDuckConnectionService::new(pg_pool.clone(), cipher),
        ),
        Box::new(motherduck::DuckDbMotherDuckClient),
        Box::new(pg::query_events::PgQueryEventService::new(pg_pool.clone())),
        Box::new(pg::storage_samples::PgStorageSampleService::new(
            pg_pool.clone(),
        )),
        Box::new(pg::query_shapes::PgQueryShapeService::new(pg_pool)),
        Box::new(sql_analysis::DuckDbSqlAnalyzer),
        IngestionSettings {
            overlap: chrono::Duration::minutes(config.ingest_overlap_minutes),
            batch_limit: config.ingest_batch_limit,
            backfill_limit: config.ingest_backfill_limit,
        },
    );

    // One watch channel fans the process signal out to the web server and the
    // ingestion poller.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    let poller = tokio::spawn(ingest::run(
        ingestion,
        Duration::from_secs(config.ingest_poll_interval_seconds),
        shutdown_rx.clone(),
    ));

    let mut web_shutdown = shutdown_rx;
    let state = web::State::new(
        Arc::new(auth),
        Arc::new(connections),
        Arc::new(dashboard),
        Arc::new(admin),
    );
    let result = web::run(config, state, async move {
        let _ = web_shutdown.changed().await;
    })
    .await;

    if let Err(err) = poller.await {
        tracing::error!("ingestion poller did not shut down cleanly: {err}");
    }

    result
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            tracing::error!("Failed to install Ctrl+C handler: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => tracing::error!("Failed to install SIGTERM handler: {err}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutting down...");
}
