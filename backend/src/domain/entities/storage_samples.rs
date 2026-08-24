use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::entities::pricing::RegionTier;

/// A storage measurement for one MotherDuck database, as the account reports
/// it. MotherDuck computes these periodically rather than continuously, so a
/// sample carries the time it was computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSampleDraft {
    pub database_name: String,
    pub active_bytes: i64,
    pub historical_bytes: i64,
    pub retained_for_clone_bytes: i64,
    pub failsafe_bytes: i64,
    pub computed_at: DateTime<Utc>,
}

impl StorageSampleDraft {
    pub fn into_sample(self, connection_id: Uuid, ingested_at: DateTime<Utc>) -> StorageSample {
        StorageSample {
            connection_id,
            database_name: self.database_name,
            active_bytes: self.active_bytes,
            historical_bytes: self.historical_bytes,
            retained_for_clone_bytes: self.retained_for_clone_bytes,
            failsafe_bytes: self.failsafe_bytes,
            computed_at: self.computed_at,
            ingested_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct StorageSample {
    pub connection_id: Uuid,
    pub database_name: String,
    pub active_bytes: i64,
    pub historical_bytes: i64,
    pub retained_for_clone_bytes: i64,
    pub failsafe_bytes: i64,
    pub computed_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
}

impl StorageSample {
    /// Every category MotherDuck reports occupies storage, so all of them
    /// count toward what the account holds. The categories are also exposed
    /// separately, since only some of them are usually worth acting on.
    pub fn total_bytes(&self) -> i64 {
        self.active_bytes
            .saturating_add(self.historical_bytes)
            .saturating_add(self.retained_for_clone_bytes)
            .saturating_add(self.failsafe_bytes)
    }
}

/// One database's storage, priced at the connection's tier.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct StorageRow {
    pub database_name: String,
    pub active_bytes: i64,
    pub historical_bytes: i64,
    pub retained_for_clone_bytes: i64,
    pub failsafe_bytes: i64,
    pub total_bytes: i64,
    pub estimated_monthly_cost_usd: f64,
    pub computed_at: DateTime<Utc>,
}

/// What the connection's account holds, and what keeping it costs per month.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct StorageSummary {
    pub databases: Vec<StorageRow>,
    pub total_bytes: i64,
    pub estimated_monthly_cost_usd: f64,
    /// When MotherDuck last computed these figures, or `None` before the
    /// first successful sample.
    pub computed_at: Option<DateTime<Utc>>,
}

impl StorageSummary {
    /// Prices the newest sample per database, largest first.
    pub fn from_samples(samples: Vec<StorageSample>, tier: RegionTier) -> Self {
        let mut databases: Vec<StorageRow> = samples
            .into_iter()
            .map(|sample| {
                let total_bytes = sample.total_bytes();
                StorageRow {
                    database_name: sample.database_name,
                    active_bytes: sample.active_bytes,
                    historical_bytes: sample.historical_bytes,
                    retained_for_clone_bytes: sample.retained_for_clone_bytes,
                    failsafe_bytes: sample.failsafe_bytes,
                    total_bytes,
                    estimated_monthly_cost_usd: tier
                        .estimate_storage_cost_usd_per_month(total_bytes),
                    computed_at: sample.computed_at,
                }
            })
            .collect();

        databases.sort_by(|a, b| {
            b.total_bytes
                .cmp(&a.total_bytes)
                .then_with(|| a.database_name.cmp(&b.database_name))
        });

        Self {
            total_bytes: databases.iter().map(|row| row.total_bytes).sum(),
            estimated_monthly_cost_usd: databases
                .iter()
                .map(|row| row.estimated_monthly_cost_usd)
                .sum(),
            computed_at: databases.iter().map(|row| row.computed_at).max(),
            databases,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, active: i64, historical: i64) -> StorageSample {
        StorageSampleDraft {
            database_name: name.to_string(),
            active_bytes: active,
            historical_bytes: historical,
            retained_for_clone_bytes: 0,
            failsafe_bytes: 0,
            computed_at: Utc::now(),
        }
        .into_sample(Uuid::new_v4(), Utc::now())
    }

    #[test]
    fn total_bytes_counts_every_category() {
        let mut one = sample("db", 10, 20);
        one.retained_for_clone_bytes = 30;
        one.failsafe_bytes = 40;
        assert_eq!(one.total_bytes(), 100);
    }

    #[test]
    fn the_summary_ranks_databases_and_sums_cost() {
        let summary = StorageSummary::from_samples(
            vec![
                sample("small", 1_000_000_000, 0),
                sample("large", 5_000_000_000, 1_000_000_000),
            ],
            RegionTier::Tier1,
        );

        assert_eq!(summary.databases[0].database_name, "large");
        assert_eq!(summary.total_bytes, 7_000_000_000);
        // Seven gigabytes at four cents each.
        assert!((summary.estimated_monthly_cost_usd - 0.28).abs() < 1e-9);
        assert!(summary.computed_at.is_some());
    }

    #[test]
    fn an_account_without_samples_costs_nothing() {
        let summary = StorageSummary::from_samples(vec![], RegionTier::Tier2);
        assert_eq!(summary.total_bytes, 0);
        assert_eq!(summary.estimated_monthly_cost_usd, 0.0);
        assert_eq!(summary.computed_at, None);
    }
}
