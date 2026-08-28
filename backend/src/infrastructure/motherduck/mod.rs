use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::application::services::motherduck::{MotherDuckClient, QueryHistoryPage};
use crate::domain::entities::motherduck_connections::MotherDuckToken;
use crate::domain::entities::query_events::{DUCKWATCH_USER_AGENT, QueryEventDraft};
use crate::domain::entities::storage_samples::StorageSampleDraft;
use crate::domain::error::{Error, Result};

/// Timestamps travel as epoch milliseconds and intervals as milliseconds, so
/// no timestamp text format has to round-trip through the driver.
const HISTORY_QUERY: &str = "-- duckwatch
    select
        query_id::varchar as query_id,
        query_text,
        query_type,
        cast(epoch_ms(start_time) as bigint) as start_ms,
        cast(epoch_ms(end_time) as bigint) as end_ms,
        cast(epoch(execution_time) * 1000 as bigint) as execution_time_ms,
        cast(epoch(wait_time) * 1000 as bigint) as wait_time_ms,
        cast(epoch(total_elapsed_time) * 1000 as bigint) as total_elapsed_time_ms,
        error_type,
        error_message,
        user_name,
        instance_type,
        duckling_id,
        session_name,
        cast(bytes_uploaded as bigint) as bytes_uploaded,
        cast(bytes_downloaded as bigint) as bytes_downloaded,
        cast(bytes_spilled_to_disk as bigint) as bytes_spilled_to_disk,
        user_agent
    from md_information_schema.query_history";

/// Deleted databases still appear in the view, so they are filtered out; a
/// dropped database costs nothing to keep.
const STORAGE_QUERY: &str = "-- duckwatch
    select
        database_name,
        cast(active_bytes as bigint) as active_bytes,
        cast(historical_bytes as bigint) as historical_bytes,
        cast(retained_for_clone_bytes as bigint) as retained_for_clone_bytes,
        cast(failsafe_bytes as bigint) as failsafe_bytes,
        cast(epoch_ms(computed_ts) as bigint) as computed_ms
    from md_information_schema.storage_info
    where deleted_ts is null";

/// Touching the view proves both the token and the plan, and costs nothing.
const CONNECTION_TEST_QUERY: &str =
    "-- duckwatch\n select count(*) from md_information_schema.query_history";

/// Talks to MotherDuck through the blocking `duckdb` crate. Every call opens a
/// fresh connection inside `spawn_blocking`; connection reuse is a later
/// optimization the poller does not need yet.
pub struct DuckDbMotherDuckClient;

/// Percent-encodes everything outside the URL-unreserved set, so the token
/// survives the DSN query string.
fn percent_encode(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// The DSN, and therefore the token, can appear inside driver error strings,
/// so every error is scrubbed before it leaves this module.
fn scrub(message: String, token: &MotherDuckToken) -> String {
    message
        .replace(&percent_encode(token.reveal()), "<redacted>")
        .replace(token.reveal(), "<redacted>")
}

fn open_raw(token: &MotherDuckToken) -> std::result::Result<duckdb::Connection, duckdb::Error> {
    let dsn = format!("md:?motherduck_token={}", percent_encode(token.reveal()));
    // The custom user agent tags DuckWatch's own traffic in the customer's
    // query history, so ingestion can mark it as internal.
    let config = duckdb::Config::default().custom_user_agent(DUCKWATCH_USER_AGENT)?;
    duckdb::Connection::open_with_flags(dsn, config)
}

fn open(token: &MotherDuckToken) -> Result<duckdb::Connection> {
    open_raw(token).map_err(|err| Error::External(anyhow::anyhow!(scrub(err.to_string(), token))))
}

fn timestamp_from_ms(ms: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms)
        .ok_or_else(|| Error::External(anyhow::anyhow!("timestamp out of range: {ms}")))
}

/// The filter clause for a fetch starting at `since`, with the value to bind.
///
/// The comparison is inclusive. `since` is often the ingestion watermark,
/// which is the newest `start_time` already stored, and other rows can share
/// that millisecond. An exclusive filter would skip those for good on a pass
/// that starts exactly at the watermark, which is what a connection still
/// catching up does. Re-reading a row costs nothing, because the event write
/// is an upsert.
fn history_filter(since: Option<DateTime<Utc>>) -> (&'static str, Option<i64>) {
    match since {
        Some(since) => (
            " where epoch_ms(start_time) >= ?",
            Some(since.timestamp_millis()),
        ),
        None => ("", None),
    }
}

fn fetch_blocking(
    token: &MotherDuckToken,
    since: Option<DateTime<Utc>>,
    limit: u32,
) -> Result<QueryHistoryPage> {
    let connection = open(token)?;

    let (filter, since_ms) = history_filter(since);
    let sql = format!("{HISTORY_QUERY}{filter} order by start_time limit {limit}");

    let map_row = |row: &duckdb::Row<'_>| -> std::result::Result<QueryEventDraft, duckdb::Error> {
        Ok(QueryEventDraft {
            // The uuid is parsed after extraction, below, to keep this
            // closure on the driver's error type.
            md_query_id: Uuid::nil(),
            query_text: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            query_type: row.get(2)?,
            start_time: DateTime::from_timestamp_millis(row.get::<_, i64>(3)?).unwrap_or_default(),
            end_time: row
                .get::<_, Option<i64>>(4)?
                .and_then(DateTime::from_timestamp_millis),
            execution_time_ms: row.get(5)?,
            wait_time_ms: row.get(6)?,
            total_elapsed_time_ms: row.get(7)?,
            error_type: row.get(8)?,
            error_message: row.get(9)?,
            user_name: row.get(10)?,
            instance_type: row.get(11)?,
            duckling_id: row.get(12)?,
            session_name: row.get(13)?,
            bytes_uploaded: row.get(14)?,
            bytes_downloaded: row.get(15)?,
            bytes_spilled_to_disk: row.get(16)?,
            user_agent: row.get(17)?,
        })
    };

    let run = || -> std::result::Result<Vec<(String, i64, QueryEventDraft)>, duckdb::Error> {
        let mut statement = connection.prepare(&sql)?;
        let extract = |row: &duckdb::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(3)?,
                map_row(row)?,
            ))
        };
        match since_ms {
            Some(ms) => statement.query_map([ms], extract)?.collect(),
            None => statement.query_map([], extract)?.collect(),
        }
    };

    let raw_rows =
        run().map_err(|err| Error::External(anyhow::anyhow!(scrub(err.to_string(), token))))?;

    let rows_returned = raw_rows.len();
    Ok(QueryHistoryPage {
        drafts: readable_drafts(raw_rows),
        rows_returned,
    })
}

/// Keeps the rows DuckWatch can read and drops the rest.
///
/// A row with an unreadable id or timestamp used to fail the whole fetch,
/// which left the watermark where it was, so the next pass read the same row
/// and the connection never moved again. Skipping costs one row; failing
/// costs every row after it. If every row in a batch were unreadable the
/// watermark would still not move, but that needs the whole view to be
/// corrupt rather than one row.
fn readable_drafts(raw_rows: Vec<(String, i64, QueryEventDraft)>) -> Vec<QueryEventDraft> {
    let mut unreadable: Vec<String> = Vec::new();
    let drafts: Vec<QueryEventDraft> = raw_rows
        .into_iter()
        .filter_map(|(id, start_ms, mut draft)| {
            match (Uuid::parse_str(&id), timestamp_from_ms(start_ms)) {
                (Ok(md_query_id), Ok(start_time)) => {
                    draft.md_query_id = md_query_id;
                    draft.start_time = start_time;
                    Some(draft)
                }
                _ => {
                    unreadable.push(id);
                    None
                }
            }
        })
        .collect();

    if !unreadable.is_empty() {
        tracing::warn!(
            "skipped {} query history rows that could not be read, starting with id {}",
            unreadable.len(),
            unreadable.first().map_or("", String::as_str)
        );
    }
    drafts
}

fn fetch_storage_blocking(token: &MotherDuckToken) -> Result<Vec<StorageSampleDraft>> {
    let connection = open(token)?;

    let run = || -> std::result::Result<Vec<(StorageSampleDraft, i64)>, duckdb::Error> {
        let mut statement = connection.prepare(STORAGE_QUERY)?;
        let rows = statement.query_map([], |row| {
            Ok((
                StorageSampleDraft {
                    database_name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    active_bytes: row.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                    historical_bytes: row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                    retained_for_clone_bytes: row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                    failsafe_bytes: row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
                    // Replaced below, once the epoch can report an error.
                    computed_at: DateTime::UNIX_EPOCH,
                },
                row.get::<_, Option<i64>>(5)?.unwrap_or_default(),
            ))
        })?;
        rows.collect()
    };

    let raw_rows = run().map_err(|err| {
        // A token without the organization wide storage permission fails
        // here, which the caller treats as storage being unavailable rather
        // than as the sync failing.
        Error::validation(format!(
            "could not read MotherDuck storage (the token needs permission to view \
             organization storage): {}",
            scrub(err.to_string(), token)
        ))
    })?;

    raw_rows
        .into_iter()
        .map(|(mut draft, computed_ms)| {
            draft.computed_at = timestamp_from_ms(computed_ms)?;
            Ok(draft)
        })
        .collect()
}

#[async_trait]
impl MotherDuckClient for DuckDbMotherDuckClient {
    async fn test_connection(&self, token: &MotherDuckToken) -> Result<()> {
        let token = token.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            // Opening fails on a bad token, and touching the view fails on a
            // MotherDuck plan without query history access. Both are caller
            // problems, so both report as validation errors.
            let check = || -> std::result::Result<(), duckdb::Error> {
                let connection = open_raw(&token)?;
                connection.query_row(CONNECTION_TEST_QUERY, [], |_| Ok(()))
            };
            check().map_err(|err| {
                Error::validation(format!(
                    "MotherDuck rejected the connection (a valid service token, a \
                     Business plan, and the view query history permission are \
                     required): {}",
                    scrub(err.to_string(), &token)
                ))
            })
        })
        .await
        .map_err(|err| Error::External(anyhow::anyhow!("test connection task failed: {err}")))?
    }

    async fn fetch_query_history(
        &self,
        token: &MotherDuckToken,
        since: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<QueryHistoryPage> {
        let token = token.clone();
        tokio::task::spawn_blocking(move || fetch_blocking(&token, since, limit))
            .await
            .map_err(|err| Error::External(anyhow::anyhow!("fetch task failed: {err}")))?
    }

    async fn fetch_storage(&self, token: &MotherDuckToken) -> Result<Vec<StorageSampleDraft>> {
        let token = token.clone();
        tokio::task::spawn_blocking(move || fetch_storage_blocking(&token))
            .await
            .map_err(|err| Error::External(anyhow::anyhow!("storage task failed: {err}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::query_events::DUCKWATCH_SQL_MARKER;

    fn raw_row(id: &str, start_ms: i64) -> (String, i64, QueryEventDraft) {
        (
            id.to_string(),
            start_ms,
            QueryEventDraft {
                md_query_id: Uuid::nil(),
                query_text: "select 1".into(),
                query_type: None,
                start_time: DateTime::from_timestamp_millis(0).unwrap_or_default(),
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
            },
        )
    }

    #[test]
    fn one_unreadable_row_does_not_cost_the_whole_batch() {
        // Failing the fetch would leave the watermark unmoved, so the next
        // pass would read the same row and the connection would never
        // advance again.
        let good = Uuid::new_v4();
        let drafts = readable_drafts(vec![
            raw_row("not-a-uuid", 1_700_000_000_000),
            raw_row(&good.to_string(), 1_700_000_000_001),
            raw_row(&Uuid::new_v4().to_string(), i64::MAX),
        ]);

        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].md_query_id, good);
        assert_eq!(drafts[0].start_time.timestamp_millis(), 1_700_000_000_001);
    }

    #[test]
    fn the_history_filter_includes_the_instant_it_starts_from() {
        // A catching-up pass starts exactly at the watermark, and rows can
        // share that millisecond with it. An exclusive filter would drop them
        // and nothing would ever read them again.
        let (filter, bound) = history_filter(Some(
            DateTime::from_timestamp_millis(1_700_000_000_000).unwrap_or_default(),
        ));
        assert!(filter.contains(">="), "filter was {filter}");
        assert!(!filter.contains("> ?"), "filter was {filter}");
        assert_eq!(bound, Some(1_700_000_000_000));
    }

    #[test]
    fn the_history_filter_is_empty_without_a_starting_point() {
        let (filter, bound) = history_filter(None);
        assert!(filter.is_empty());
        assert_eq!(bound, None);
    }

    #[test]
    fn percent_encode_escapes_reserved_characters() {
        assert_eq!(percent_encode("abc-123._~"), "abc-123._~");
        assert_eq!(percent_encode("a&b=c d"), "a%26b%3Dc%20d");
    }

    #[test]
    fn every_statement_duckwatch_sends_carries_the_marker() {
        // The marker is how DuckWatch's own polling is kept out of the
        // dashboard before MotherDuck reports the user agent, so a statement
        // that loses it would show up as customer traffic.
        for statement in [HISTORY_QUERY, STORAGE_QUERY, CONNECTION_TEST_QUERY] {
            assert!(
                statement.contains(DUCKWATCH_SQL_MARKER),
                "statement is not marked: {statement}"
            );
        }
    }

    #[test]
    fn the_marker_survives_the_clauses_added_at_run_time() {
        // The history query is assembled before it is sent, so the marker has
        // to be on the part that always leads.
        let assembled = format!(
            "{HISTORY_QUERY} where epoch_ms(start_time) >= ? order by start_time limit 5000"
        );
        assert!(assembled.contains(DUCKWATCH_SQL_MARKER));
        // A leading line comment must not swallow the statement itself.
        assert!(assembled.lines().count() > 1);
    }

    #[test]
    fn scrub_redacts_the_token() {
        let token = MotherDuckToken::new("secret&token").unwrap();
        let scrubbed = scrub("failed with secret&token in dsn".to_string(), &token);
        assert!(!scrubbed.contains("secret&token"));
        assert!(scrubbed.contains("<redacted>"));

        let encoded = scrub("dsn had secret%26token".to_string(), &token);
        assert!(!encoded.contains("secret%26token"));
    }
}

/// A live check against a real MotherDuck account. Run it manually with
/// `MOTHERDUCK_TEST_TOKEN=... cargo test --features integration-tests -- --ignored`.
#[cfg(all(test, feature = "integration-tests"))]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "needs MOTHERDUCK_TEST_TOKEN and network access"]
    async fn fetch_query_history_returns_rows() {
        let raw = std::env::var("MOTHERDUCK_TEST_TOKEN").unwrap();
        let token = MotherDuckToken::new(&raw).unwrap();
        let client = DuckDbMotherDuckClient;

        client.test_connection(&token).await.unwrap();
        let page = client.fetch_query_history(&token, None, 10).await.unwrap();
        assert!(page.rows_returned <= 10);
        assert!(page.drafts.len() <= page.rows_returned);
    }
}
