use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::entities::pricing::RegionTier;
use crate::domain::error::{Error, Result};

// `u64` because `validator` length rules in the web layer expect that type.
pub const CONNECTION_NAME_MAX_LEN: u64 = 128;
pub const TOKEN_MAX_LEN: u64 = 4096;

/// A MotherDuck service token. It never serializes and never prints, and the
/// entity below does not carry it, so it cannot leak through a response or a
/// log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct MotherDuckToken(String);

impl MotherDuckToken {
    pub fn new(raw: &str) -> Result<Self> {
        let token = raw.trim();
        if token.is_empty() {
            return Err(Error::validation("token must not be empty"));
        }
        if token.chars().count() as u64 > TOKEN_MAX_LEN {
            return Err(Error::validation(format!(
                "token must be at most {TOKEN_MAX_LEN} characters"
            )));
        }
        Ok(Self(token.to_string()))
    }

    /// Exposes the token to the encryption and connection code paths.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for MotherDuckToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MotherDuckToken(<redacted>)")
    }
}

/// A customer's MotherDuck account hooked up for ingestion. The service token
/// is stored encrypted and only reachable through the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct MotherDuckConnection {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    /// Which MotherDuck price tier this account is billed at, used for the
    /// dashboard's cost estimates.
    pub region_tier: RegionTier,
    pub enabled: bool,
    pub watermark_start_time: Option<DateTime<Utc>>,
    /// When the poller last tried, whether or not it worked.
    pub last_synced_at: Option<DateTime<Utc>>,
    /// When the poller last succeeded. Separate from the attempt above,
    /// because otherwise a connection that has been failing for days is
    /// indistinguishable from one that succeeded a moment ago.
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_sync_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// How ingestion is going for one connection. Every figure on the dashboard
/// is only as fresh as the last sync that worked, so this applies to all of
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "snake_case")]
pub enum IngestionHealth {
    /// The poller skips this connection, so nothing new arrives.
    Disabled,
    /// Connected, but the first sync has not finished yet.
    Pending,
    /// The last attempt reported an error.
    Failing,
    /// No error, but nothing has succeeded recently either.
    Stale,
    Healthy,
}

/// A connection and how its ingestion is going, which the dashboard shows
/// above the numbers that depend on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct ConnectionStatus {
    #[serde(flatten)]
    pub connection: MotherDuckConnection,
    pub health: IngestionHealth,
    /// Seconds since the last successful sync, or `None` if none has ever
    /// succeeded.
    pub seconds_since_success: Option<i64>,
    /// How far behind the newest ingested query is. MotherDuck publishes its
    /// query history with some delay of its own, so a small lag is normal.
    pub seconds_behind: Option<i64>,
    /// How long without a success counts as stale, so the interface can say
    /// what the judgment was based on.
    pub stale_after_seconds: i64,
}

impl MotherDuckConnection {
    /// Classifies ingestion for this connection. `stale_after` is how long a
    /// connection may go without a successful sync before it is called stale.
    pub fn status(&self, now: DateTime<Utc>, stale_after: chrono::Duration) -> ConnectionStatus {
        let seconds_since_success = self
            .last_success_at
            .map(|at| (now - at).num_seconds().max(0));
        let seconds_behind = self
            .watermark_start_time
            .map(|at| (now - at).num_seconds().max(0));

        let health = if !self.enabled {
            IngestionHealth::Disabled
        } else if self.last_sync_error.is_some() {
            // An error is the most actionable thing to report, even on a
            // connection that has never yet succeeded.
            IngestionHealth::Failing
        } else if self.last_success_at.is_none() {
            match self.last_synced_at {
                // Tries with no success and no error means the success came
                // before this column existed, so its age is unknown.
                Some(_) => IngestionHealth::Stale,
                None => IngestionHealth::Pending,
            }
        } else if seconds_since_success.unwrap_or(0) > stale_after.num_seconds() {
            IngestionHealth::Stale
        } else {
            IngestionHealth::Healthy
        };

        ConnectionStatus {
            connection: self.clone(),
            health,
            seconds_since_success,
            seconds_behind,
            stale_after_seconds: stale_after.num_seconds(),
        }
    }
}

/// The fields a caller supplies when connecting a MotherDuck account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionDraft {
    name: String,
    token: MotherDuckToken,
    region_tier: RegionTier,
}

impl ConnectionDraft {
    pub fn new(name: &str, token: &str, region_tier: RegionTier) -> Result<Self> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::validation("connection name must not be empty"));
        }
        if name.chars().count() as u64 > CONNECTION_NAME_MAX_LEN {
            return Err(Error::validation(format!(
                "connection name must be at most {CONNECTION_NAME_MAX_LEN} characters"
            )));
        }

        Ok(Self {
            name: name.to_string(),
            token: MotherDuckToken::new(token)?,
            region_tier,
        })
    }

    pub fn token(&self) -> &MotherDuckToken {
        &self.token
    }

    /// Builds a new enabled connection for an organization, handing the token
    /// back separately so it can go straight to encryption.
    pub fn into_new_connection(
        self,
        org_id: Uuid,
        now: DateTime<Utc>,
    ) -> (MotherDuckConnection, MotherDuckToken) {
        (
            MotherDuckConnection {
                id: Uuid::new_v4(),
                org_id,
                name: self.name,
                region_tier: self.region_tier,
                enabled: true,
                watermark_start_time: None,
                last_synced_at: None,
                last_success_at: None,
                last_sync_error: None,
                created_at: now,
                updated_at: now,
            },
            self.token,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trims_the_name_and_token() {
        let draft = ConnectionDraft::new("  prod  ", "  tok  ", RegionTier::Tier1).unwrap();
        let (connection, token) = draft.into_new_connection(Uuid::new_v4(), Utc::now());
        assert_eq!(connection.name, "prod");
        assert_eq!(connection.region_tier, RegionTier::Tier1);
        assert_eq!(token.reveal(), "tok");
        assert!(connection.enabled);
        assert_eq!(connection.watermark_start_time, None);
    }

    #[test]
    fn new_rejects_a_blank_name() {
        assert!(matches!(
            ConnectionDraft::new("  ", "tok", RegionTier::Tier1).unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn new_rejects_a_blank_token() {
        assert!(matches!(
            ConnectionDraft::new("prod", "  ", RegionTier::Tier1).unwrap_err(),
            Error::Validation(_)
        ));
    }

    fn connection() -> MotherDuckConnection {
        ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(Uuid::new_v4(), Utc::now())
            .0
    }

    fn stale_after() -> chrono::Duration {
        chrono::Duration::minutes(5)
    }

    #[test]
    fn a_connection_awaiting_its_first_sync_is_pending() {
        let status = connection().status(Utc::now(), stale_after());
        assert_eq!(status.health, IngestionHealth::Pending);
        assert_eq!(status.seconds_since_success, None);
        assert_eq!(status.seconds_behind, None);
    }

    #[test]
    fn a_recent_success_is_healthy() {
        let now = Utc::now();
        let mut connection = connection();
        connection.last_synced_at = Some(now);
        connection.last_success_at = Some(now - chrono::Duration::seconds(30));
        connection.watermark_start_time = Some(now - chrono::Duration::seconds(90));

        let status = connection.status(now, stale_after());

        assert_eq!(status.health, IngestionHealth::Healthy);
        assert_eq!(status.seconds_since_success, Some(30));
        assert_eq!(status.seconds_behind, Some(90));
        assert_eq!(status.stale_after_seconds, 300);
    }

    #[test]
    fn a_success_older_than_the_threshold_is_stale() {
        let now = Utc::now();
        let mut connection = connection();
        connection.last_synced_at = Some(now);
        connection.last_success_at = Some(now - chrono::Duration::minutes(20));

        assert_eq!(
            connection.status(now, stale_after()).health,
            IngestionHealth::Stale
        );
    }

    #[test]
    fn a_recorded_error_wins_over_a_recent_success() {
        // The poller writes the attempt time whether or not it worked, so a
        // failing connection can still look fresh by attempt alone.
        let now = Utc::now();
        let mut connection = connection();
        connection.last_synced_at = Some(now);
        connection.last_success_at = Some(now - chrono::Duration::seconds(10));
        connection.last_sync_error = Some("permission denied".into());

        assert_eq!(
            connection.status(now, stale_after()).health,
            IngestionHealth::Failing
        );
    }

    #[test]
    fn a_disabled_connection_reports_as_disabled_rather_than_stale() {
        let now = Utc::now();
        let mut connection = connection();
        connection.enabled = false;
        connection.last_success_at = Some(now - chrono::Duration::days(30));

        assert_eq!(
            connection.status(now, stale_after()).health,
            IngestionHealth::Disabled
        );
    }

    #[test]
    fn attempts_without_a_recorded_success_are_stale_rather_than_healthy() {
        // Rows that synced before `last_success_at` existed: the sync worked,
        // but how recently is unknown, so it must not claim to be healthy.
        let now = Utc::now();
        let mut connection = connection();
        connection.last_synced_at = Some(now);

        assert_eq!(
            connection.status(now, stale_after()).health,
            IngestionHealth::Stale
        );
    }

    #[test]
    fn a_clock_skewed_success_does_not_report_negative_age() {
        let now = Utc::now();
        let mut connection = connection();
        connection.last_success_at = Some(now + chrono::Duration::seconds(5));

        assert_eq!(
            connection.status(now, stale_after()).seconds_since_success,
            Some(0)
        );
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = MotherDuckToken::new("secret").unwrap();
        assert_eq!(format!("{token:?}"), "MotherDuckToken(<redacted>)");
    }
}
