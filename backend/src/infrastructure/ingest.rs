use std::time::Duration;

use tokio::sync::watch;

use crate::application::use_cases::ingestion::IngestionUseCase;

/// Drives the ingestion use case on a fixed interval until the shutdown
/// signal fires. The first tick runs immediately, so a fresh deployment does
/// not wait a full interval before its first sync.
pub async fn run(
    ingestion: IngestionUseCase,
    poll_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(err) = ingestion.run_once().await {
                    tracing::error!("ingestion pass failed: {err}");
                }
            }
            _ = shutdown.changed() => {
                tracing::info!("Ingestion poller shutting down...");
                return;
            }
        }
    }
}
