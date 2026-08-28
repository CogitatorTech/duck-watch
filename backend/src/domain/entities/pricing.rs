use serde::Serialize;

use crate::domain::error::{Error, Result};

/// MotherDuck prices compute by region tier. Tier 1 is the US regions, tier 2
/// is Europe, and tier 3 is Asia Pacific.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum RegionTier {
    /// The US regions, and the default when none is chosen.
    #[default]
    Tier1,
    Tier2,
    Tier3,
}

/// Business plan rates in US dollars per Duckling hour, as published by
/// MotherDuck: https://motherduck.com/docs/about-motherduck/billing/pricing/
/// Keep the order aligned with `INSTANCE_TYPES`.
const INSTANCE_TYPES: [&str; 5] = ["pulse", "standard", "jumbo", "mega", "giga"];
const TIER_1_RATES: [f64; 5] = [0.60, 2.40, 4.80, 12.00, 24.00];
const TIER_2_RATES: [f64; 5] = [0.73, 2.93, 5.86, 14.65, 29.30];
const TIER_3_RATES: [f64; 5] = [0.77, 3.10, 6.19, 15.48, 30.96];

/// Storage rates in US dollars per gigabyte-month, by tier.
const TIER_STORAGE_RATES: [f64; 3] = [0.04, 0.043, 0.044];

const BYTES_PER_GB: f64 = 1_000_000_000.0;

/// Pulse bills per compute unit second with a one second floor, so a very
/// short query still costs a second.
const PULSE_MINIMUM_SECONDS: f64 = 1.0;

/// The same floor in milliseconds, so the store can count the runs that fall
/// under it without knowing what it is for. A test keeps the two in step.
pub const PULSE_MINIMUM_MS: i64 = 1000;

/// What a group of runs on one Duckling size needs to be priced.
///
/// The Pulse floor applies to each run, so a total and a count are not enough
/// to work out what a group owes. The store also counts the runs that came in
/// under the floor and how long they took, which is all the tier needs to
/// raise each of them to the minimum without seeing them one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupRuntime {
    /// Billed time across the group, before any floor.
    pub total_ms: i64,
    /// Runs that came in under the floor, and the time they took. A run with
    /// no duration reported counts here too, so it still bills the minimum
    /// rather than nothing at all.
    pub sub_second_count: i64,
    pub sub_second_ms: i64,
}

impl RegionTier {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_lowercase().as_str() {
            "tier1" => Ok(Self::Tier1),
            "tier2" => Ok(Self::Tier2),
            "tier3" => Ok(Self::Tier3),
            _ => Err(Error::validation(
                "region must be one of tier1, tier2, or tier3",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tier1 => "tier1",
            Self::Tier2 => "tier2",
            Self::Tier3 => "tier3",
        }
    }

    fn rates(self) -> [f64; 5] {
        match self {
            Self::Tier1 => TIER_1_RATES,
            Self::Tier2 => TIER_2_RATES,
            Self::Tier3 => TIER_3_RATES,
        }
    }

    /// The hourly rate for a Duckling size, or `None` for a size MotherDuck
    /// introduced after this table was written.
    pub fn hourly_rate_usd(self, instance_type: &str) -> Option<f64> {
        let needle = instance_type.trim().to_lowercase();
        INSTANCE_TYPES
            .iter()
            .position(|name| *name == needle)
            .map(|index| self.rates()[index])
    }

    /// Estimates what a run of `duration_ms` on `instance_type` costs.
    ///
    /// This is an attribution, not a bill: MotherDuck charges Standard and
    /// larger Ducklings for wall-clock uptime rather than per query, so
    /// concurrent queries share compute that this attributes to each of them.
    pub fn estimate_cost_usd(
        self,
        instance_type: Option<&str>,
        duration_ms: Option<i64>,
    ) -> Option<f64> {
        let instance_type = instance_type?;
        let rate = self.hourly_rate_usd(instance_type)?;
        let billable = match instance_type.trim().to_lowercase().as_str() {
            // A Pulse run with no reported duration still bills the floor,
            // the same way the group figures count it, so a row in the query
            // table never shows nothing while the tiles charge a second.
            "pulse" => {
                ((duration_ms.unwrap_or(0).max(0) as f64) / 1000.0).max(PULSE_MINIMUM_SECONDS)
            }
            _ => (duration_ms?.max(0) as f64) / 1000.0,
        };
        Some(billable / 3600.0 * rate)
    }

    /// What a gigabyte costs to keep for a month at this tier.
    pub fn storage_rate_usd_per_gb_month(self) -> f64 {
        match self {
            Self::Tier1 => TIER_STORAGE_RATES[0],
            Self::Tier2 => TIER_STORAGE_RATES[1],
            Self::Tier3 => TIER_STORAGE_RATES[2],
        }
    }

    /// What holding `bytes` costs for a month.
    ///
    /// Storage bills on average usage over a month, so this is a run rate
    /// from the newest measurement rather than a charge for any one period.
    pub fn estimate_storage_cost_usd_per_month(self, bytes: i64) -> f64 {
        (bytes.max(0) as f64) / BYTES_PER_GB * self.storage_rate_usd_per_gb_month()
    }

    /// Estimates the cost of a group of runs on one Duckling size. The Pulse
    /// floor applies per query, so each run under a second bills a full one
    /// and the rest keep their own time. Flooring the group total instead
    /// would undercharge any group that mixes short runs with long ones.
    pub fn estimate_group_cost_usd(self, instance_type: &str, runtime: GroupRuntime) -> f64 {
        let Some(rate) = self.hourly_rate_usd(instance_type) else {
            return 0.0;
        };
        let billable = match instance_type.trim().to_lowercase().as_str() {
            "pulse" => {
                let over_floor_ms = runtime.total_ms.max(0) - runtime.sub_second_ms.max(0);
                (over_floor_ms.max(0) as f64) / 1000.0
                    + (runtime.sub_second_count.max(0) as f64) * PULSE_MINIMUM_SECONDS
            }
            _ => (runtime.total_ms.max(0) as f64) / 1000.0,
        };
        billable / 3600.0 * rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_three_tiers() {
        assert_eq!(RegionTier::parse("tier1").unwrap(), RegionTier::Tier1);
        assert_eq!(RegionTier::parse(" TIER2 ").unwrap(), RegionTier::Tier2);
        assert_eq!(RegionTier::parse("tier3").unwrap(), RegionTier::Tier3);
        assert!(matches!(
            RegionTier::parse("tier4").unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn rates_differ_between_tiers() {
        assert_eq!(RegionTier::Tier1.hourly_rate_usd("standard"), Some(2.40));
        assert_eq!(RegionTier::Tier2.hourly_rate_usd("standard"), Some(2.93));
        assert_eq!(RegionTier::Tier3.hourly_rate_usd("Giga"), Some(30.96));
        assert_eq!(RegionTier::Tier1.hourly_rate_usd("titan"), None);
    }

    #[test]
    fn an_hour_on_standard_costs_the_hourly_rate() {
        let cost = RegionTier::Tier1
            .estimate_cost_usd(Some("standard"), Some(3_600_000))
            .unwrap();
        assert!((cost - 2.40).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn pulse_bills_at_least_one_second() {
        let short = RegionTier::Tier1
            .estimate_cost_usd(Some("pulse"), Some(10))
            .unwrap();
        let one_second = RegionTier::Tier1
            .estimate_cost_usd(Some("pulse"), Some(1000))
            .unwrap();
        assert!((short - one_second).abs() < 1e-12);
    }

    #[test]
    fn storage_costs_the_published_rate_per_gigabyte() {
        // A terabyte for a month at tier 1 is 1000 GB at four cents.
        let cost = RegionTier::Tier1.estimate_storage_cost_usd_per_month(1_000_000_000_000);
        assert!((cost - 40.0).abs() < 1e-9, "cost was {cost}");

        assert!(
            RegionTier::Tier3.estimate_storage_cost_usd_per_month(1_000_000_000)
                > RegionTier::Tier1.estimate_storage_cost_usd_per_month(1_000_000_000),
            "asia pacific storage costs more than the us"
        );
    }

    #[test]
    fn empty_storage_is_free() {
        assert_eq!(
            RegionTier::Tier1.estimate_storage_cost_usd_per_month(0),
            0.0
        );
        assert_eq!(
            RegionTier::Tier1.estimate_storage_cost_usd_per_month(-5),
            0.0
        );
    }

    fn runtime(total_ms: i64, sub_second_count: i64, sub_second_ms: i64) -> GroupRuntime {
        GroupRuntime {
            total_ms,
            sub_second_count,
            sub_second_ms,
        }
    }

    #[test]
    fn the_pulse_floor_reads_the_same_in_both_units() {
        assert_eq!(PULSE_MINIMUM_MS as f64 / 1000.0, PULSE_MINIMUM_SECONDS);
    }

    #[test]
    fn a_group_bills_the_summed_time() {
        let cost = RegionTier::Tier1.estimate_group_cost_usd("standard", runtime(7_200_000, 0, 0));
        assert!((cost - 4.80).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn a_pulse_group_bills_at_least_one_second_per_query() {
        // Ten queries of 10 ms each still bill as ten seconds.
        let cost = RegionTier::Tier1.estimate_group_cost_usd("pulse", runtime(100, 10, 100));
        let ten_seconds = 10.0 / 3600.0 * 0.60;
        assert!((cost - ten_seconds).abs() < 1e-12, "cost was {cost}");
    }

    #[test]
    fn a_pulse_group_floors_each_short_run_rather_than_the_total() {
        // One run of 0.1 s and one of 10 s. Raising the short run to a second
        // gives eleven seconds. Flooring the 10.1 s total against a floor of
        // two seconds would leave it at 10.1 and undercharge the group, which
        // is what the summary tiles and the attribution table used to do.
        let cost = RegionTier::Tier1.estimate_group_cost_usd("pulse", runtime(10_100, 1, 100));
        let eleven_seconds = 11.0 / 3600.0 * 0.60;
        assert!((cost - eleven_seconds).abs() < 1e-12, "cost was {cost}");

        let floored_total = 10.1 / 3600.0 * 0.60;
        assert!(
            cost > floored_total,
            "the group has to cost more than {floored_total}"
        );
    }

    #[test]
    fn a_pulse_group_of_long_runs_is_not_floored_at_all() {
        let cost = RegionTier::Tier1.estimate_group_cost_usd("pulse", runtime(20_000, 0, 0));
        let twenty_seconds = 20.0 / 3600.0 * 0.60;
        assert!((cost - twenty_seconds).abs() < 1e-12, "cost was {cost}");
    }

    #[test]
    fn an_unknown_group_size_costs_nothing() {
        assert_eq!(
            RegionTier::Tier1.estimate_group_cost_usd("titan", runtime(1000, 0, 0)),
            0.0
        );
    }

    #[test]
    fn an_unknown_size_or_missing_duration_has_no_estimate() {
        assert_eq!(
            RegionTier::Tier1.estimate_cost_usd(Some("titan"), Some(1000)),
            None
        );
        assert_eq!(
            RegionTier::Tier1.estimate_cost_usd(Some("standard"), None),
            None
        );
        assert_eq!(RegionTier::Tier1.estimate_cost_usd(None, Some(1000)), None);
    }

    #[test]
    fn a_pulse_run_with_no_duration_bills_the_floor() {
        // The group figures count such a run as under the floor, so the per
        // event estimate has to bill the same second or the visible rows
        // would never sum to the visible totals.
        let unreported = RegionTier::Tier1.estimate_cost_usd(Some("pulse"), None);
        let floored = RegionTier::Tier1.estimate_cost_usd(Some("pulse"), Some(PULSE_MINIMUM_MS));
        assert_eq!(unreported, floored);
        assert!(unreported.is_some());
    }
}
