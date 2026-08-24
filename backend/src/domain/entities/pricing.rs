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
        let seconds = (duration_ms?.max(0) as f64) / 1000.0;
        let billable = match instance_type.trim().to_lowercase().as_str() {
            "pulse" => seconds.max(PULSE_MINIMUM_SECONDS),
            _ => seconds,
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
    /// floor applies per query, so the group bills at least one second each.
    pub fn estimate_group_cost_usd(
        self,
        instance_type: &str,
        total_ms: i64,
        query_count: i64,
    ) -> f64 {
        let Some(rate) = self.hourly_rate_usd(instance_type) else {
            return 0.0;
        };
        let seconds = (total_ms.max(0) as f64) / 1000.0;
        let billable = match instance_type.trim().to_lowercase().as_str() {
            "pulse" => seconds.max(query_count.max(0) as f64 * PULSE_MINIMUM_SECONDS),
            _ => seconds,
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

    #[test]
    fn a_group_bills_the_summed_time() {
        let cost = RegionTier::Tier1.estimate_group_cost_usd("standard", 7_200_000, 4);
        assert!((cost - 4.80).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn a_pulse_group_bills_at_least_one_second_per_query() {
        // Ten queries of 10 ms each still bill as ten seconds.
        let cost = RegionTier::Tier1.estimate_group_cost_usd("pulse", 100, 10);
        let ten_seconds = 10.0 / 3600.0 * 0.60;
        assert!((cost - ten_seconds).abs() < 1e-12, "cost was {cost}");
    }

    #[test]
    fn an_unknown_group_size_costs_nothing() {
        assert_eq!(
            RegionTier::Tier1.estimate_group_cost_usd("titan", 1000, 1),
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
            RegionTier::Tier1.estimate_cost_usd(Some("pulse"), None),
            None
        );
        assert_eq!(RegionTier::Tier1.estimate_cost_usd(None, Some(1000)), None);
    }
}
