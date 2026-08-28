use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

use crate::application::services::motherduck::MotherDuckClient;
use crate::application::services::motherduck_connections::{
    MotherDuckConnectionService, SyncState,
};
use crate::application::services::query_events::QueryEventService;
use crate::application::services::query_shapes::QueryShapeService;
use crate::application::services::sql_analysis::SqlAnalyzer;
use crate::application::services::storage_samples::StorageSampleService;
use crate::domain::entities::insights::Antipattern;
use crate::domain::entities::query_events::QueryEvent;
use crate::domain::entities::query_shapes::QueryShape;
use crate::domain::error::{Error, Result};

/// Bounds for one sync pass, wired from configuration.
#[derive(Debug, Clone, Copy)]
pub struct IngestionSettings {
    /// How far behind the watermark each fetch restarts. MotherDuck's history
    /// view fills in with some delay, and re-reading is free because the
    /// event write is an upsert.
    pub overlap: Duration,
    /// Maximum rows fetched per connection per pass.
    pub batch_limit: u32,
    /// How many already stored queries each pass fingerprints, so events
    /// ingested before analysis existed catch up without a long migration.
    pub backfill_limit: u32,
    /// How long to wait between storage reads for one connection. Storage is
    /// recomputed by MotherDuck far less often than queries arrive.
    pub storage_interval: Duration,
}

/// One poll cycle over every enabled connection. The scheduler in
/// `infrastructure/ingest.rs` drives this on an interval; keeping the logic
/// here makes it unit-testable with mocks.
pub struct IngestionUseCase {
    connection_service: Box<dyn MotherDuckConnectionService>,
    motherduck_client: Box<dyn MotherDuckClient>,
    query_event_service: Box<dyn QueryEventService>,
    storage_sample_service: Box<dyn StorageSampleService>,
    query_shape_service: Box<dyn QueryShapeService>,
    sql_analyzer: Box<dyn SqlAnalyzer>,
    settings: IngestionSettings,
    poll_state: Mutex<HashMap<uuid::Uuid, PollState>>,
}

/// What the poller remembers about one connection between passes. Losing it
/// to a restart costs one extra storage read and one pass that re-reads the
/// overlap window, which is why it lives here rather than in a column.
#[derive(Debug, Default, Clone, Copy)]
struct PollState {
    /// When storage was last read, whether or not the read worked.
    storage_attempted_at: Option<DateTime<Utc>>,
    /// The last batch filled the limit, so the rows still owed are past the
    /// watermark rather than behind it.
    catching_up: bool,
}

impl IngestionUseCase {
    pub fn new(
        connection_service: Box<dyn MotherDuckConnectionService>,
        motherduck_client: Box<dyn MotherDuckClient>,
        query_event_service: Box<dyn QueryEventService>,
        storage_sample_service: Box<dyn StorageSampleService>,
        query_shape_service: Box<dyn QueryShapeService>,
        sql_analyzer: Box<dyn SqlAnalyzer>,
        settings: IngestionSettings,
    ) -> Self {
        Self {
            connection_service,
            motherduck_client,
            query_event_service,
            storage_sample_service,
            query_shape_service,
            sql_analyzer,
            settings,
            poll_state: Mutex::new(HashMap::new()),
        }
    }

    /// What the last pass left behind. A poisoned lock yields the default,
    /// which reads storage and applies the overlap, and both are safe.
    fn poll_state(&self, connection_id: uuid::Uuid) -> PollState {
        self.poll_state
            .lock()
            .ok()
            .and_then(|states| states.get(&connection_id).copied())
            .unwrap_or_default()
    }

    fn update_poll_state(&self, connection_id: uuid::Uuid, change: impl FnOnce(&mut PollState)) {
        if let Ok(mut states) = self.poll_state.lock() {
            change(states.entry(connection_id).or_default());
        }
    }

    /// Syncs every enabled connection. A failure on one connection is
    /// recorded on its row and does not stall the others; only listing the
    /// connections can fail the pass as a whole.
    pub async fn run_once(&self) -> Result<()> {
        let connections = self.connection_service.find_enabled().await?;

        for connection in connections {
            // Queries stored before analysis existed catch up a slice at a
            // time, and a failure here must not stop the sync below.
            match self.backfill_fingerprints(connection.id).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(
                    "fingerprinted {count} earlier queries for connection {}",
                    connection.id
                ),
                Err(err) => tracing::warn!(
                    "fingerprint backfill failed for connection {}: {err}",
                    connection.id
                ),
            }

            // Shapes recorded before anti-pattern analysis existed catch up
            // the same way, and likewise must not stop the sync below.
            match self.backfill_antipatterns(connection.id).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(
                    "examined {count} earlier shapes for connection {}",
                    connection.id
                ),
                Err(err) => tracing::warn!(
                    "anti-pattern backfill failed for connection {}: {err}",
                    connection.id
                ),
            }

            if let Err(err) = self
                .sync_connection(connection.id, connection.watermark_start_time)
                .await
            {
                tracing::warn!("sync failed for connection {}: {err}", connection.id);
                let state = SyncState {
                    watermark_start_time: connection.watermark_start_time,
                    last_synced_at: Utc::now(),
                    last_success_at: None,
                    last_sync_error: Some(err.to_string()),
                    ingest_warning: None,
                };
                if let Err(err) = self
                    .connection_service
                    .update_sync_state(connection.id, state)
                    .await
                {
                    tracing::error!(
                        "could not record the sync error for connection {}: {err}",
                        connection.id
                    );
                }
            }
        }

        Ok(())
    }

    async fn sync_connection(
        &self,
        connection_id: uuid::Uuid,
        watermark: Option<chrono::DateTime<Utc>>,
    ) -> Result<()> {
        let token = self.connection_service.get_token(connection_id).await?;
        let state = self.poll_state(connection_id);

        // The overlap re-reads the head of the history, because MotherDuck
        // publishes rows there late. A batch that filled the limit last time
        // means the rows still owed sit past the watermark rather than behind
        // it, and reading that window again would crowd them out for good.
        let since = match watermark {
            Some(mark) if state.catching_up => Some(mark),
            Some(mark) => Some(mark - self.settings.overlap),
            None => None,
        };

        let page = self
            .motherduck_client
            .fetch_query_history(&token, since, self.settings.batch_limit)
            .await?;
        let drafts = page.drafts;
        // Rows the fetch dropped because it could not read them. They are
        // gone from the figures for good, so the count is reported through
        // the sync state below rather than left to a log line.
        let unreadable_rows = page.rows_returned.saturating_sub(drafts.len());

        let now = Utc::now();
        // Judged from what MotherDuck returned, not from what survived the
        // read. A single unreadable row would otherwise make a full batch
        // look short, clear the catching-up flag, and send the next pass back
        // over the overlap window it had just worked through.
        let filled_the_batch = page.rows_returned >= self.settings.batch_limit as usize;
        // History is read oldest first, so a batch cut short by the limit can
        // end earlier than the watermark it started from. Storing that would
        // send the next fetch further back still, and the one after that
        // further again, until ingestion never reaches the present.
        let new_watermark = drafts
            .iter()
            .map(|draft| draft.start_time)
            .max()
            .map(|newest| watermark.map_or(newest, |mark| mark.max(newest)))
            .or(watermark);
        let mut events: Vec<QueryEvent> = drafts
            .into_iter()
            .map(|draft| draft.into_event(connection_id, now))
            .collect();

        self.assign_fingerprints(connection_id, &mut events).await?;
        self.query_event_service.upsert_batch(events).await?;
        // Recorded only once the batch is stored. A full batch that failed to
        // store left its rows nowhere, so the next pass has to read the
        // overlap window again rather than skip it.
        self.update_poll_state(connection_id, |state| state.catching_up = filled_the_batch);

        // Storage needs a wider permission than the query history, so a
        // token without it still gets working query ingestion. It is also
        // read on its own slower schedule, because MotherDuck recomputes it
        // every one to six hours.
        // A connection never read before is always due, so a newly added one
        // fills its storage panel on the first pass rather than an hour later.
        // This runs before the stall check below for the same independence; a
        // wedged query watermark must not freeze the storage panel with it.
        let storage_due = state
            .storage_attempted_at
            .is_none_or(|last| now - last >= self.settings.storage_interval);
        if storage_due {
            // Recorded whether or not it works. A token without the storage
            // permission fails every pass, and that is the case where backing
            // off matters most.
            self.update_poll_state(connection_id, |state| {
                state.storage_attempted_at = Some(now)
            });
            if let Err(err) = self.sync_storage(connection_id, &token, now).await {
                tracing::info!("storage unavailable for connection {connection_id}: {err}");
            }
        }

        // A catching-up pass starts at the watermark itself, so a full batch
        // that moves nothing means no later `since` can ever step past this
        // instant. That takes either rows past the watermark the fetch could
        // not read, which skipping cannot get around because they fill every
        // page, or more queries in one millisecond than the batch limit
        // allows. The two need different action, so they are told apart here
        // and reported rather than left to stall in silence.
        if state.catching_up && filled_the_batch && new_watermark == watermark {
            if unreadable_rows > 0 {
                return Err(Error::External(anyhow::anyhow!(
                    "{unreadable_rows} query history rows past the watermark could not be read, \
                     so ingestion cannot move past them"
                )));
            }
            return Err(Error::External(anyhow::anyhow!(
                "more than {} queries share one instant, so ingestion cannot move past it; \
                 raise ingest_batch_limit",
                self.settings.batch_limit
            )));
        }

        // A pass that skipped rows still succeeded, but the figures are
        // missing those rows for good, so the reader is told through the
        // warning field the health banner reads. The error field would say
        // syncing is broken, which it is not.
        let skipped_note = (unreadable_rows > 0).then(|| {
            format!(
                "{unreadable_rows} query history rows could not be read and were skipped, \
                 so the figures are missing them"
            )
        });
        self.connection_service
            .update_sync_state(
                connection_id,
                SyncState {
                    watermark_start_time: new_watermark,
                    last_synced_at: now,
                    last_success_at: Some(now),
                    last_sync_error: None,
                    ingest_warning: skipped_note,
                },
            )
            .await
    }
}

impl IngestionUseCase {
    /// Analyzes a batch, stamps each event with its shape, and records any
    /// shape not seen before.
    async fn assign_fingerprints(
        &self,
        connection_id: uuid::Uuid,
        events: &mut [QueryEvent],
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let statements: Vec<String> = events
            .iter()
            .map(|event| event.query_text.clone())
            .collect();
        let analyses = self.sql_analyzer.analyze_batch(statements).await;
        if analyses.len() != events.len() {
            // Analysis failed as a whole; the backfill pass will retry these.
            tracing::warn!("sql analysis returned no results for connection {connection_id}");
            return Ok(());
        }

        let mut shapes = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (event, analysis) in events.iter_mut().zip(analyses) {
            let fingerprint = analysis.fingerprint.as_str().to_string();
            if seen.insert(fingerprint.clone()) {
                shapes.push(QueryShape {
                    connection_id,
                    fingerprint: fingerprint.clone(),
                    normalized_sql: analysis.normalized_sql,
                    example_sql: event.query_text.clone(),
                    parsed: analysis.parsed,
                    antipatterns: analysis.antipatterns,
                    first_seen: event.start_time,
                });
            }
            event.fingerprint = Some(fingerprint);
        }

        self.query_shape_service.upsert_batch(shapes).await?;
        Ok(())
    }

    /// Works through shapes recorded before anti-pattern analysis existed, a
    /// slice per pass. A shape found clean is recorded as clean, so it is not
    /// examined again.
    async fn backfill_antipatterns(&self, connection_id: uuid::Uuid) -> Result<u64> {
        let pending = self
            .query_shape_service
            .find_unflagged(connection_id, self.settings.backfill_limit)
            .await?;
        if pending.is_empty() {
            return Ok(0);
        }

        let statements: Vec<String> = pending
            .iter()
            .map(|shape| shape.example_sql.clone())
            .collect();
        let analyses = self.sql_analyzer.analyze_batch(statements).await;
        if analyses.len() != pending.len() {
            return Ok(0);
        }

        let assignments: Vec<(String, Vec<Antipattern>)> = pending
            .into_iter()
            .zip(analyses)
            .map(|(shape, analysis)| (shape.fingerprint, analysis.antipatterns))
            .collect();

        self.query_shape_service
            .set_antipatterns(connection_id, assignments)
            .await
    }

    /// Works through queries stored before analysis existed, a slice per pass.
    async fn backfill_fingerprints(&self, connection_id: uuid::Uuid) -> Result<u64> {
        let pending = self
            .query_event_service
            .find_unfingerprinted(connection_id, self.settings.backfill_limit)
            .await?;
        if pending.is_empty() {
            return Ok(0);
        }

        let statements: Vec<String> = pending
            .iter()
            .map(|query| query.query_text.clone())
            .collect();
        let analyses = self.sql_analyzer.analyze_batch(statements).await;
        if analyses.len() != pending.len() {
            return Ok(0);
        }

        let mut assignments = Vec::with_capacity(pending.len());
        let mut shapes = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (query, analysis) in pending.into_iter().zip(analyses) {
            let fingerprint = analysis.fingerprint.as_str().to_string();
            if seen.insert(fingerprint.clone()) {
                shapes.push(QueryShape {
                    connection_id,
                    fingerprint: fingerprint.clone(),
                    normalized_sql: analysis.normalized_sql,
                    example_sql: query.query_text,
                    parsed: analysis.parsed,
                    antipatterns: analysis.antipatterns,
                    first_seen: query.start_time,
                });
            }
            assignments.push((query.md_query_id, fingerprint));
        }

        self.query_shape_service.upsert_batch(shapes).await?;
        self.query_event_service
            .set_fingerprints(connection_id, assignments)
            .await
    }

    async fn sync_storage(
        &self,
        connection_id: uuid::Uuid,
        token: &crate::domain::entities::motherduck_connections::MotherDuckToken,
        now: chrono::DateTime<Utc>,
    ) -> Result<()> {
        let drafts = self.motherduck_client.fetch_storage(token).await?;
        let samples = drafts
            .into_iter()
            .map(|draft| draft.into_sample(connection_id, now))
            .collect();
        self.storage_sample_service.upsert_batch(samples).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use mockall::predicate;
    use uuid::Uuid;

    use super::*;
    use crate::application::services::motherduck::{MockMotherDuckClient, QueryHistoryPage};
    use crate::application::services::motherduck_connections::MockMotherDuckConnectionService;
    use crate::application::services::query_events::MockQueryEventService;
    use crate::application::services::query_shapes::MockQueryShapeService;
    use crate::application::services::sql_analysis::MockSqlAnalyzer;
    use crate::application::services::storage_samples::MockStorageSampleService;
    use crate::domain::entities::motherduck_connections::{
        ConnectionDraft, MotherDuckConnection, MotherDuckToken,
    };
    use crate::domain::entities::pricing::RegionTier;
    use crate::domain::entities::query_events::QueryEventDraft;
    use crate::domain::entities::query_shapes::UnflaggedShape;
    use crate::domain::entities::query_shapes::{
        QueryFingerprint, SqlAnalysis, UnfingerprintedQuery, normalize_without_parser,
    };
    use crate::domain::error::Error;

    /// Analysis runs on every pass, so each test needs a stub that hands back
    /// one result per statement.
    fn analyzer_stub() -> MockSqlAnalyzer {
        let mut analyzer = MockSqlAnalyzer::new();
        analyzer.expect_analyze_batch().returning(|statements| {
            statements
                .iter()
                .map(|sql| {
                    let normalized = normalize_without_parser(sql);
                    SqlAnalysis {
                        fingerprint: QueryFingerprint::from_normalized(&normalized),
                        normalized_sql: normalized,
                        parsed: true,
                        antipatterns: Vec::new(),
                    }
                })
                .collect()
        });
        analyzer
    }

    /// Like `analyzer_stub`, but reporting the one flag the backfill test
    /// needs, so the assertion is about the use case rather than the parser.
    fn flagging_analyzer_stub() -> MockSqlAnalyzer {
        let mut analyzer = MockSqlAnalyzer::new();
        analyzer.expect_analyze_batch().returning(|statements| {
            statements
                .iter()
                .map(|sql| {
                    let normalized = normalize_without_parser(sql);
                    let antipatterns = match normalized.contains("select *") {
                        true => vec![Antipattern::SelectStar],
                        false => Vec::new(),
                    };
                    SqlAnalysis {
                        fingerprint: QueryFingerprint::from_normalized(&normalized),
                        normalized_sql: normalized,
                        parsed: true,
                        antipatterns,
                    }
                })
                .collect()
        });
        analyzer
    }

    fn shape_stub() -> MockQueryShapeService {
        let mut service = MockQueryShapeService::new();
        service
            .expect_upsert_batch()
            .returning(|batch| Ok(batch.len() as u64));
        expect_no_flag_backfill(&mut service);
        service
    }

    /// The anti-pattern backfill runs on every pass, so a test that is not
    /// about it needs it to find nothing.
    fn expect_no_flag_backfill(shapes: &mut MockQueryShapeService) {
        shapes.expect_find_unflagged().returning(|_, _| Ok(vec![]));
    }

    /// Storage is sampled on every pass, so each test needs a stub for it.
    fn storage_stub() -> MockStorageSampleService {
        let mut service = MockStorageSampleService::new();
        service
            .expect_upsert_batch()
            .returning(|batch| Ok(batch.len() as u64));
        service
    }

    fn settings() -> IngestionSettings {
        IngestionSettings {
            overlap: Duration::minutes(15),
            batch_limit: 1000,
            backfill_limit: 500,
            storage_interval: Duration::hours(1),
        }
    }

    /// Backfill runs before each sync, so an event service mock needs to
    /// answer it even when a test is about something else.
    fn expect_no_backfill(events: &mut MockQueryEventService) {
        events
            .expect_find_unfingerprinted()
            .returning(|_, _| Ok(vec![]));
    }

    fn connection(watermark: Option<DateTime<Utc>>) -> MotherDuckConnection {
        let (mut connection, _) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(Utc::now());
        connection.watermark_start_time = watermark;
        connection
    }

    /// A page holding exactly the drafts given, as if MotherDuck returned no
    /// rows DuckWatch could not read.
    fn page(drafts: Vec<QueryEventDraft>) -> QueryHistoryPage {
        QueryHistoryPage {
            rows_returned: drafts.len(),
            drafts,
        }
    }

    fn draft(start_time: DateTime<Utc>) -> QueryEventDraft {
        QueryEventDraft {
            md_query_id: Uuid::new_v4(),
            query_text: "select 1".into(),
            query_type: None,
            start_time,
            end_time: None,
            execution_time_ms: None,
            wait_time_ms: None,
            total_elapsed_time_ms: None,
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
    }

    #[tokio::test]
    async fn run_once_fetches_from_the_overlapped_watermark_and_advances_it() {
        let watermark = Utc::now();
        let connection = connection(Some(watermark));
        let connection_id = connection.id;
        let newest = watermark + Duration::minutes(10);

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .with(predicate::eq(connection_id))
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(move |id, state| {
                *id == connection_id
                    && state.watermark_start_time == Some(newest)
                    && state.last_sync_error.is_none()
            })
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        let expected_since = watermark - Duration::minutes(15);
        client
            .expect_fetch_query_history()
            .withf(move |_, since, limit| *since == Some(expected_since) && *limit == 1000)
            .return_once(move |_, _, _| Ok(page(vec![draft(watermark), draft(newest)])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events
            .expect_upsert_batch()
            .withf(move |batch| batch.len() == 2 && batch[0].connection_id == connection_id)
            .return_once(|batch| Ok(batch.len() as u64));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_empty_batch_keeps_the_watermark() {
        let watermark = Utc::now();
        let connection = connection(Some(watermark));
        let connection_id = connection.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(move |id, state| {
                *id == connection_id && state.watermark_start_time == Some(watermark)
            })
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().return_once(|_| Ok(0));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    /// Builds a use case whose mocks answer any number of passes, so a test
    /// can run several and watch how often storage is read.
    /// A limit small enough that a couple of drafts fill a batch, which is
    /// what puts the poller into catching-up mode.
    fn small_batch_settings() -> IngestionSettings {
        IngestionSettings {
            batch_limit: 2,
            ..settings()
        }
    }

    fn repeatable(
        client: MockMotherDuckClient,
        connection: MotherDuckConnection,
    ) -> IngestionUseCase {
        repeatable_with(client, connection, settings())
    }

    fn repeatable_with(
        client: MockMotherDuckClient,
        connection: MotherDuckConnection,
        settings: IngestionSettings,
    ) -> IngestionUseCase {
        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .returning(|_, _| Ok(()));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().returning(|_| Ok(0));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings,
        )
    }

    #[tokio::test]
    async fn a_truncated_batch_does_not_move_the_watermark_backwards() {
        // History is read oldest first, so an account busy enough to fill the
        // batch inside the overlap window hands back nothing but rows that
        // predate the watermark.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));
        let connection_id = connection.id;
        let older = watermark - Duration::minutes(5);

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(move |id, state| {
                *id == connection_id && state.watermark_start_time == Some(watermark)
            })
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(move |_, _, _| {
                Ok(page(vec![
                    draft(older - Duration::minutes(1)),
                    draft(older),
                ]))
            });

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events
            .expect_upsert_batch()
            .return_once(|batch| Ok(batch.len() as u64));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            small_batch_settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_full_batch_makes_the_next_pass_skip_the_overlap() {
        // Otherwise a connection whose overlap window alone fills the batch
        // reads the same rows forever and never reaches the present.
        let watermark = Utc::now();
        let newest = watermark + Duration::minutes(30);

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        let with_overlap = watermark - Duration::minutes(15);
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(with_overlap))
            .times(1)
            .returning(move |_, _, _| Ok(page(vec![draft(watermark), draft(newest)])));
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark))
            .times(1)
            .returning(|_, _, _| Ok(page(vec![])));

        let use_case = repeatable_with(client, connection(Some(watermark)), small_batch_settings());
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn a_full_batch_that_fails_to_store_does_not_skip_the_overlap() {
        // The batch that filled the limit was never written, so the rows it
        // carried exist nowhere but past reads. Starting the next pass at the
        // watermark would skip the overlap window for good and lose the late
        // published rows in it.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .returning(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        let with_overlap = watermark - Duration::minutes(15);
        // Both passes must read the overlap window, because the first one
        // stored nothing.
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(with_overlap))
            .times(1)
            .returning(move |_, _, _| {
                Ok(page(vec![
                    draft(watermark),
                    draft(watermark + Duration::minutes(1)),
                ]))
            });
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(with_overlap))
            .times(1)
            .returning(|_, _, _| Ok(page(vec![])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events
            .expect_upsert_batch()
            .times(1)
            .returning(|_| Err(Error::External(anyhow::anyhow!("connection reset"))));
        events.expect_upsert_batch().times(1).returning(|_| Ok(0));

        let use_case = IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            small_batch_settings(),
        );
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn an_unreadable_row_does_not_make_a_full_batch_look_short() {
        // Fullness decides whether the next pass skips the overlap. Judging
        // it from the drafts rather than from what MotherDuck returned would
        // let one dropped row send the poller back over the window it had
        // just worked through, and keep it there.
        let watermark = Utc::now();
        let newest = watermark + Duration::minutes(30);

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        let with_overlap = watermark - Duration::minutes(15);
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(with_overlap))
            .times(1)
            .returning(move |_, _, _| {
                // Two rows came back against a limit of two, but one of them
                // could not be read.
                Ok(QueryHistoryPage {
                    drafts: vec![draft(newest)],
                    rows_returned: 2,
                })
            });
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark))
            .times(1)
            .returning(|_, _, _| Ok(page(vec![])));

        let use_case = repeatable_with(client, connection(Some(watermark)), small_batch_settings());
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn a_full_batch_of_nothing_but_ties_is_reported_rather_than_stalling() {
        // Every row shares the watermark's instant, so no later starting
        // point can step past them. Left silent this reads as a healthy
        // connection that never moves again.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(|_, state| state.last_sync_error.is_none())
            .times(1)
            .returning(|_, _| Ok(()));
        connections
            .expect_update_sync_state()
            .withf(|_, state| {
                state
                    .last_sync_error
                    .as_deref()
                    .is_some_and(|error| error.contains("share one instant"))
            })
            .times(1)
            .returning(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark - Duration::minutes(15)))
            .times(1)
            .returning(move |_, _, _| {
                Ok(page(vec![
                    draft(watermark),
                    draft(watermark + Duration::minutes(1)),
                ]))
            });
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark))
            .times(1)
            .returning(move |_, _, _| Ok(page(vec![draft(watermark), draft(watermark)])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().returning(|_| Ok(0));

        let use_case = IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            small_batch_settings(),
        );
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn a_stall_behind_unreadable_rows_names_them_rather_than_ties() {
        // Every readable row sits at the watermark and the rows past it fail
        // to read on every pass, so skipping cannot get around them. Calling
        // that a tie problem would send the operator to raise the batch
        // limit, which cannot help.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(|_, state| state.last_sync_error.is_none())
            .times(1)
            .returning(|_, _| Ok(()));
        connections
            .expect_update_sync_state()
            .withf(|_, state| {
                state
                    .last_sync_error
                    .as_deref()
                    .is_some_and(|error| error.contains("could not be read"))
            })
            .times(1)
            .returning(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark - Duration::minutes(15)))
            .times(1)
            .returning(move |_, _, _| {
                Ok(page(vec![
                    draft(watermark),
                    draft(watermark + Duration::minutes(1)),
                ]))
            });
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark))
            .times(1)
            .returning(move |_, _, _| {
                // A full page, but the row past the watermark was dropped as
                // unreadable, so the watermark cannot move.
                Ok(QueryHistoryPage {
                    drafts: vec![draft(watermark)],
                    rows_returned: 2,
                })
            });

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().returning(|_| Ok(0));

        let use_case = IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            small_batch_settings(),
        );
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn skipped_rows_are_reported_as_a_warning_rather_than_a_failure() {
        // The skipped rows are missing from the figures for good, so a pass
        // that dropped some succeeds while saying so where the health banner
        // can read it. Reporting them through the error field instead would
        // make a working connection read as a failing one.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));
        let newest = watermark + Duration::minutes(10);

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .withf(move |_, state| {
                state.watermark_start_time == Some(newest)
                    && state.last_success_at.is_some()
                    && state.last_sync_error.is_none()
                    && state
                        .ingest_warning
                        .as_deref()
                        .is_some_and(|warning| warning.contains("skipped"))
            })
            .times(1)
            .returning(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(move |_, _, _| {
                Ok(QueryHistoryPage {
                    drafts: vec![draft(newest)],
                    rows_returned: 2,
                })
            });

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().returning(|_| Ok(0));

        let use_case = IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        );
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn a_stalled_watermark_does_not_stop_the_storage_read() {
        // The stall guard reports through the sync error, but storage reads
        // fail on their own schedule and must keep working while the query
        // watermark is wedged.
        let watermark = Utc::now();
        let connection = connection(Some(watermark));

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .returning(move || Ok(vec![connection.clone()]));
        connections
            .expect_get_token()
            .returning(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .returning(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        // Storage is due on both passes, and the second pass reports
        // the stall, so the second read is the assertion.
        client
            .expect_fetch_storage()
            .times(2)
            .returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark - Duration::minutes(15)))
            .times(1)
            .returning(move |_, _, _| {
                Ok(page(vec![
                    draft(watermark),
                    draft(watermark + Duration::minutes(1)),
                ]))
            });
        client
            .expect_fetch_query_history()
            .withf(move |_, since, _| *since == Some(watermark))
            .times(1)
            .returning(move |_, _, _| Ok(page(vec![draft(watermark), draft(watermark)])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().returning(|_| Ok(0));

        let mut storage = MockStorageSampleService::new();
        storage
            .expect_upsert_batch()
            .returning(|batch| Ok(batch.len() as u64));

        let use_case = IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            IngestionSettings {
                batch_limit: 2,
                // Storage due on both passes, so the stalled second pass has
                // to attempt it too for the expectation above to hold.
                storage_interval: Duration::zero(),
                ..settings()
            },
        );
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn storage_is_read_once_per_interval_rather_than_once_per_pass() {
        // MotherDuck recomputes storage every one to six hours, so reading it
        // on every pass bills the account for identical reads.
        let mut client = MockMotherDuckClient::new();
        client
            .expect_fetch_query_history()
            .returning(|_, _, _| Ok(page(vec![])));
        client
            .expect_fetch_storage()
            .times(1)
            .returning(|_| Ok(vec![]));

        let use_case = repeatable(client, connection(Some(Utc::now())));
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn a_storage_read_that_fails_still_waits_out_the_interval() {
        // A token without the storage permission fails every time, so backing
        // off matters most there.
        let mut client = MockMotherDuckClient::new();
        client
            .expect_fetch_query_history()
            .returning(|_, _, _| Ok(page(vec![])));
        client
            .expect_fetch_storage()
            .times(1)
            .returning(|_| Err(Error::External(anyhow::anyhow!("permission denied"))));

        let use_case = repeatable(client, connection(Some(Utc::now())));
        use_case.run_once().await.unwrap();
        use_case.run_once().await.unwrap();
    }

    #[tokio::test]
    async fn the_first_sync_fetches_everything() {
        let connection = connection(None);

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .withf(|_, since, _| since.is_none())
            .return_once(|_, _, _| Ok(page(vec![])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().return_once(|_| Ok(0));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn queries_are_stamped_with_their_shape() {
        let connection = connection(None);

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .return_once(|_, _| Ok(()));

        // Two runs of one shape, differing only in a literal, plus a third
        // statement against a different table.
        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client.expect_fetch_query_history().return_once(|_, _, _| {
            let mut first = draft(Utc::now());
            first.query_text = "select a from t where d = '2026-06-01'".into();
            let mut second = draft(Utc::now());
            second.query_text = "select a from t where d = '2026-07-01'".into();
            let mut other = draft(Utc::now());
            other.query_text = "select a from other".into();
            Ok(page(vec![first, second, other]))
        });

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events
            .expect_upsert_batch()
            .withf(|batch: &Vec<QueryEvent>| {
                let fingerprints: Vec<_> = batch
                    .iter()
                    .map(|event| event.fingerprint.clone())
                    .collect();
                // The pair shares a fingerprint; the third does not.
                fingerprints.iter().all(Option::is_some)
                    && fingerprints[0] == fingerprints[1]
                    && fingerprints[0] != fingerprints[2]
            })
            .return_once(|batch| Ok(batch.len() as u64));

        // Only the two distinct shapes are recorded, not all three runs.
        let mut shapes = MockQueryShapeService::new();
        expect_no_flag_backfill(&mut shapes);
        shapes
            .expect_upsert_batch()
            .withf(|batch| batch.len() == 2)
            .return_once(|batch| Ok(batch.len() as u64));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shapes),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn earlier_shapes_are_examined_for_antipatterns_a_slice_at_a_time() {
        let connection = connection(None);
        let connection_id = connection.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().return_once(|_| Ok(0));

        let mut shapes = MockQueryShapeService::new();
        shapes.expect_upsert_batch().returning(|_| Ok(0));
        shapes.expect_find_unflagged().return_once(|_, _| {
            Ok(vec![
                UnflaggedShape {
                    fingerprint: "aaaa".into(),
                    example_sql: "select * from t".into(),
                },
                UnflaggedShape {
                    fingerprint: "bbbb".into(),
                    example_sql: "select a from t where a = 1 limit 10".into(),
                },
            ])
        });
        shapes
            .expect_set_antipatterns()
            .withf(move |id, assignments| {
                // Both are recorded, the clean one with an empty list, so
                // neither comes back on the next pass.
                *id == connection_id
                    && assignments.len() == 2
                    && assignments[0].0 == "aaaa"
                    && assignments[0].1.contains(&Antipattern::SelectStar)
                    && assignments[1].0 == "bbbb"
                    && assignments[1].1.is_empty()
            })
            .return_once(|_, assignments| Ok(assignments.len() as u64));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shapes),
            Box::new(flagging_analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn earlier_queries_are_fingerprinted_a_slice_at_a_time() {
        let connection = connection(None);
        let connection_id = connection.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![])));

        let pending_id = Uuid::new_v4();
        let mut events = MockQueryEventService::new();
        events
            .expect_find_unfingerprinted()
            .return_once(move |_, _| {
                Ok(vec![UnfingerprintedQuery {
                    md_query_id: pending_id,
                    query_text: "select a from t where d = '2026-06-01'".into(),
                    start_time: Utc::now(),
                }])
            });
        events
            .expect_set_fingerprints()
            .withf(move |id, assignments| {
                *id == connection_id
                    && assignments.len() == 1
                    && assignments[0].0 == pending_id
                    && !assignments[0].1.is_empty()
            })
            .return_once(|_, assignments| Ok(assignments.len() as u64));
        events.expect_upsert_batch().return_once(|_| Ok(0));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn storage_without_permission_does_not_fail_the_sync() {
        let connection = connection(None);
        let connection_id = connection.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        // The pass still records a clean sync despite storage being refused.
        connections
            .expect_update_sync_state()
            .withf(move |id, state| *id == connection_id && state.last_sync_error.is_none())
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![draft(Utc::now())])));
        client
            .expect_fetch_storage()
            .return_once(|_| Err(Error::validation("needs the storage permission")));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events
            .expect_upsert_batch()
            .return_once(|batch| Ok(batch.len() as u64));

        // Nothing is written when the read fails.
        let mut storage = MockStorageSampleService::new();
        storage.expect_upsert_batch().never();

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn storage_samples_are_stored_against_the_connection() {
        let connection = connection(None);
        let connection_id = connection.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![connection]));
        connections
            .expect_get_token()
            .return_once(|_| MotherDuckToken::new("tok"));
        connections
            .expect_update_sync_state()
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![])));
        client.expect_fetch_storage().return_once(|_| {
            Ok(vec![
                crate::domain::entities::storage_samples::StorageSampleDraft {
                    database_name: "analytics".into(),
                    active_bytes: 1_000_000_000,
                    historical_bytes: 0,
                    retained_for_clone_bytes: 0,
                    failsafe_bytes: 0,
                    computed_at: Utc::now(),
                },
            ])
        });

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().return_once(|_| Ok(0));

        let mut storage = MockStorageSampleService::new();
        storage
            .expect_upsert_batch()
            .withf(move |samples| {
                samples.len() == 1
                    && samples[0].connection_id == connection_id
                    && samples[0].database_name == "analytics"
            })
            .return_once(|batch| Ok(batch.len() as u64));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_failing_connection_records_the_error_and_does_not_stall_others() {
        let watermark = Utc::now();
        let broken = connection(Some(watermark));
        let healthy = connection(None);
        let broken_id = broken.id;
        let healthy_id = healthy.id;

        let mut connections = MockMotherDuckConnectionService::new();
        connections
            .expect_find_enabled()
            .return_once(move || Ok(vec![broken, healthy]));
        connections
            .expect_get_token()
            .with(predicate::eq(broken_id))
            .return_once(|_| Err(Error::External(anyhow::anyhow!("decrypt failed"))));
        connections
            .expect_get_token()
            .with(predicate::eq(healthy_id))
            .return_once(|_| MotherDuckToken::new("tok"));
        // The broken connection records its error and keeps its watermark.
        connections
            .expect_update_sync_state()
            .withf(move |id, state| {
                *id == broken_id
                    && state.last_sync_error.is_some()
                    && state.watermark_start_time == Some(watermark)
            })
            .return_once(|_, _| Ok(()));
        // The healthy one still syncs.
        connections
            .expect_update_sync_state()
            .withf(move |id, state| *id == healthy_id && state.last_sync_error.is_none())
            .return_once(|_, _| Ok(()));

        let mut client = MockMotherDuckClient::new();
        client.expect_fetch_storage().returning(|_| Ok(vec![]));
        client
            .expect_fetch_query_history()
            .return_once(|_, _, _| Ok(page(vec![])));

        let mut events = MockQueryEventService::new();
        expect_no_backfill(&mut events);
        events.expect_upsert_batch().return_once(|_| Ok(0));

        IngestionUseCase::new(
            Box::new(connections),
            Box::new(client),
            Box::new(events),
            Box::new(storage_stub()),
            Box::new(shape_stub()),
            Box::new(analyzer_stub()),
            settings(),
        )
        .run_once()
        .await
        .unwrap();
    }
}
