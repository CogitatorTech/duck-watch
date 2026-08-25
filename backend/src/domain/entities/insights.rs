use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::domain::entities::query_shapes::ShapeStats;

/// Something worth reviewing, found either in the query text or in how its
/// runs behaved.
///
/// These are signals to check, not conclusions. A flagged query is not always
/// wrong, so each one is shown next to what it cost, and is only raised when
/// that cost is big enough to be worth someone's time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "snake_case")]
pub enum Antipattern {
    /// `select *`, which reads every column in a store that would otherwise
    /// read only the ones asked for.
    SelectStar,
    /// A join with no condition, which pairs every row with every row.
    CrossJoin,
    /// A top-level select over a table with no `where` clause.
    NoFilter,
    /// `order by` with no `limit`, which sorts the whole result.
    OrderWithoutLimit,
    /// Runs that spilled to disk, meaning the Duckling ran out of memory.
    Spilling,
    /// The same statement run many times, where caching or materializing may
    /// pay for itself.
    RepeatedRuns,
}

impl Antipattern {
    /// The four flags found in the query text. These are stored per shape
    /// when it is analyzed; the other two come from how the runs behaved and
    /// are worked out at read time rather than stored.
    pub const STATIC: [Antipattern; 4] = [
        Antipattern::SelectStar,
        Antipattern::CrossJoin,
        Antipattern::NoFilter,
        Antipattern::OrderWithoutLimit,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Antipattern::SelectStar => "select_star",
            Antipattern::CrossJoin => "cross_join",
            Antipattern::NoFilter => "no_filter",
            Antipattern::OrderWithoutLimit => "order_without_limit",
            Antipattern::Spilling => "spilling",
            Antipattern::RepeatedRuns => "repeated_runs",
        }
    }

    /// Reads a stored flag back. An unknown name, such as one written by a
    /// later release, is ignored rather than failing the read.
    pub fn parse(raw: &str) -> Option<Self> {
        Antipattern::STATIC
            .into_iter()
            .chain([Antipattern::Spilling, Antipattern::RepeatedRuns])
            .find(|candidate| candidate.as_str() == raw)
    }
}

/// One flagged query shape, with what it actually cost over the period so the
/// reader can judge whether it is worth acting on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Insight {
    pub antipattern: Antipattern,
    pub fingerprint: String,
    pub example_sql: String,
    pub runs: i64,
    pub failure_count: i64,
    pub total_ms: i64,
    pub bytes_spilled: i64,
    pub estimated_cost_usd: f64,
    /// Share of the period's estimated compute cost, between 0 and 1.
    pub cost_share: f64,
    pub last_seen: DateTime<Utc>,
}

/// How many shapes raised one kind of finding, and what they cost between
/// them. One shape can raise more than one kind, so these totals must never be
/// added together; that would count the same shape twice.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct AntipatternTotal {
    pub antipattern: Antipattern,
    pub shapes: i64,
    pub estimated_cost_usd: f64,
}

/// Everything found over the period. The list itself is capped by the caller,
/// so the count and the per kind totals describe all of it rather than only
/// the part that fitted.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Insights {
    pub findings: Vec<Insight>,
    /// How many findings there are in total, before any cap.
    pub total: i64,
    pub totals: Vec<AntipatternTotal>,
}

/// A flag from the query text is only raised once the shape accounts for at
/// least this much of the period. Below that the finding is still true but
/// not worth anyone's time, and a list nobody acts on is just noise.
const MIN_COST_SHARE: f64 = 0.01;

/// A share is relative, so on a quiet account a shape can be most of a period
/// that cost a penny. Nothing under this is worth changing a query for,
/// whatever share it holds. Ten cents is a couple of minutes of a Standard
/// Duckling.
const MIN_COST_USD: f64 = 0.10;

/// Spilling means the Duckling ran out of memory and fell back to disk, which
/// is worth knowing about even on a cheap query. A gigabyte leaves out the
/// small spills that say nothing.
const MIN_SPILL_BYTES: i64 = 1_000_000_000;

/// A shape has to run at least this often before repetition is worth
/// reporting rather than just noting.
const MIN_REPEATED_RUNS: i64 = 50;

/// Repetition is only worth fixing when the repeated work adds up, so it has
/// a higher bar than a flag from the query text.
const MIN_REPEAT_COST_SHARE: f64 = 0.02;

/// Finds what is worth reviewing across the period's query shapes, most
/// expensive first. One shape can raise more than one finding, since a query
/// can both repeat and run out of memory.
pub fn detect(shapes: &[ShapeStats]) -> Insights {
    let mut insights: Vec<Insight> = Vec::new();

    for shape in shapes {
        let mut raise = |antipattern: Antipattern| {
            insights.push(Insight {
                antipattern,
                fingerprint: shape.fingerprint.clone(),
                example_sql: shape.example_sql.clone(),
                runs: shape.runs,
                failure_count: shape.failure_count,
                total_ms: shape.total_ms,
                bytes_spilled: shape.bytes_spilled,
                estimated_cost_usd: shape.estimated_cost_usd,
                cost_share: shape.cost_share,
                last_seen: shape.last_seen,
            });
        };

        if shape.bytes_spilled >= MIN_SPILL_BYTES {
            raise(Antipattern::Spilling);
        }

        // Spilling is exempt from the cost floor above: running out of
        // memory is worth knowing about even when the query is cheap,
        // because the fix is a bigger Duckling rather than a cheaper query.
        let worth_attention = shape.estimated_cost_usd >= MIN_COST_USD;

        if worth_attention
            && shape.runs >= MIN_REPEATED_RUNS
            && shape.cost_share >= MIN_REPEAT_COST_SHARE
        {
            raise(Antipattern::RepeatedRuns);
        }

        if worth_attention && shape.cost_share >= MIN_COST_SHARE {
            for antipattern in &shape.antipatterns {
                raise(*antipattern);
            }
        }
    }

    // Most expensive first, with the fingerprint and flag breaking ties so
    // the order does not jump around between refreshes.
    insights.sort_by(|a, b| {
        b.estimated_cost_usd
            .partial_cmp(&a.estimated_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
            .then_with(|| a.antipattern.as_str().cmp(b.antipattern.as_str()))
    });

    Insights {
        total: insights.len() as i64,
        totals: totals_by_antipattern(&insights),
        findings: insights,
    }
}

/// Adds up each kind over every finding of that kind, most expensive first.
fn totals_by_antipattern(insights: &[Insight]) -> Vec<AntipatternTotal> {
    let mut totals: Vec<AntipatternTotal> = Vec::new();

    for insight in insights {
        match totals
            .iter_mut()
            .find(|total| total.antipattern == insight.antipattern)
        {
            Some(total) => {
                total.shapes += 1;
                total.estimated_cost_usd += insight.estimated_cost_usd;
            }
            None => totals.push(AntipatternTotal {
                antipattern: insight.antipattern,
                shapes: 1,
                estimated_cost_usd: insight.estimated_cost_usd,
            }),
        }
    }

    totals.sort_by(|a, b| {
        b.estimated_cost_usd
            .partial_cmp(&a.estimated_cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.antipattern.as_str().cmp(b.antipattern.as_str()))
    });
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(fingerprint: &str) -> ShapeStats {
        ShapeStats {
            fingerprint: fingerprint.into(),
            example_sql: "select * from t".into(),
            runs: 1,
            failure_count: 0,
            total_ms: 1000,
            max_ms: 1000,
            bytes_spilled: 0,
            antipatterns: Vec::new(),
            estimated_cost_usd: 1.0,
            cost_share: 0.5,
            last_seen: Utc::now(),
        }
    }

    fn kinds(insights: &Insights) -> Vec<Antipattern> {
        insights
            .findings
            .iter()
            .map(|insight| insight.antipattern)
            .collect()
    }

    #[test]
    fn a_clean_shape_raises_nothing() {
        assert!(detect(&[shape("aaaa")]).findings.is_empty());
    }

    #[test]
    fn a_costly_shape_raises_the_flags_read_off_its_text() {
        let mut one = shape("aaaa");
        one.antipatterns = vec![Antipattern::SelectStar, Antipattern::NoFilter];

        assert_eq!(
            kinds(&detect(&[one])),
            vec![Antipattern::NoFilter, Antipattern::SelectStar]
        );
    }

    #[test]
    fn a_cheap_shape_does_not_raise_a_text_flag() {
        let mut one = shape("aaaa");
        one.antipatterns = vec![Antipattern::SelectStar];
        one.cost_share = 0.001;

        assert!(detect(&[one]).findings.is_empty());
    }

    #[test]
    fn most_of_a_period_that_cost_nothing_is_not_worth_reviewing() {
        // A share is relative. On a quiet account one shape can be 80% of a
        // period that cost a penny, which is not a reason to change a query.
        let mut one = shape("aaaa");
        one.antipatterns = vec![Antipattern::SelectStar, Antipattern::NoFilter];
        one.cost_share = 0.806;
        one.estimated_cost_usd = 0.004;

        assert!(detect(&[one]).findings.is_empty());
    }

    #[test]
    fn repetition_also_needs_the_cost_to_be_real() {
        let mut one = shape("aaaa");
        one.runs = 500;
        one.cost_share = 0.90;
        one.estimated_cost_usd = 0.004;

        assert!(detect(&[one]).findings.is_empty());
    }

    #[test]
    fn spilling_is_raised_even_when_the_shape_is_cheap() {
        // Running out of memory is worth knowing about regardless of cost,
        // because the fix is a bigger Duckling rather than a cheaper query.
        let mut one = shape("aaaa");
        one.cost_share = 0.0001;
        one.estimated_cost_usd = 0.0001;
        one.bytes_spilled = 6_800_000_000;

        assert_eq!(kinds(&detect(&[one])), vec![Antipattern::Spilling]);
    }

    #[test]
    fn a_small_spill_is_not_a_finding() {
        let mut one = shape("aaaa");
        one.bytes_spilled = 5_000_000;

        assert!(detect(&[one]).findings.is_empty());
    }

    #[test]
    fn repetition_needs_both_the_runs_and_the_cost() {
        let mut frequent_and_dear = shape("aaaa");
        frequent_and_dear.runs = 500;
        frequent_and_dear.cost_share = 0.30;

        let mut frequent_but_trivial = shape("bbbb");
        frequent_but_trivial.runs = 500;
        frequent_but_trivial.cost_share = 0.001;

        let mut dear_but_rare = shape("cccc");
        dear_but_rare.runs = 2;
        dear_but_rare.cost_share = 0.30;

        assert_eq!(
            kinds(&detect(&[frequent_and_dear])),
            vec![Antipattern::RepeatedRuns]
        );
        assert!(detect(&[frequent_but_trivial]).findings.is_empty());
        assert!(detect(&[dear_but_rare]).findings.is_empty());
    }

    #[test]
    fn one_shape_can_raise_several_findings() {
        let mut one = shape("aaaa");
        one.runs = 200;
        one.cost_share = 0.40;
        one.bytes_spilled = 2_000_000_000;
        one.antipatterns = vec![Antipattern::SelectStar];

        assert_eq!(
            kinds(&detect(&[one])),
            vec![
                Antipattern::RepeatedRuns,
                Antipattern::SelectStar,
                Antipattern::Spilling,
            ]
        );
    }

    #[test]
    fn findings_are_ranked_by_what_they_cost() {
        let mut cheap = shape("aaaa");
        cheap.antipatterns = vec![Antipattern::SelectStar];
        cheap.estimated_cost_usd = 0.5;

        let mut dear = shape("bbbb");
        dear.antipatterns = vec![Antipattern::CrossJoin];
        dear.estimated_cost_usd = 40.0;

        let found = detect(&[cheap, dear]).findings;
        assert_eq!(found[0].fingerprint, "bbbb");
        assert_eq!(found[1].fingerprint, "aaaa");
    }

    #[test]
    fn the_totals_count_every_finding_of_each_kind() {
        let mut spilling_and_starred = shape("aaaa");
        spilling_and_starred.bytes_spilled = 2_000_000_000;
        spilling_and_starred.antipatterns = vec![Antipattern::SelectStar];
        spilling_and_starred.estimated_cost_usd = 10.0;

        let mut spilling_only = shape("bbbb");
        spilling_only.bytes_spilled = 3_000_000_000;
        spilling_only.estimated_cost_usd = 4.0;

        let found = detect(&[spilling_and_starred, spilling_only]);

        assert_eq!(found.total, 3);
        // Dearest kind first: two spills at $10 and $4 against one star at $10.
        assert_eq!(found.totals[0].antipattern, Antipattern::Spilling);
        assert_eq!(found.totals[0].shapes, 2);
        assert!((found.totals[0].estimated_cost_usd - 14.0).abs() < 1e-9);
        assert_eq!(found.totals[1].antipattern, Antipattern::SelectStar);
        assert_eq!(found.totals[1].shapes, 1);
    }

    #[test]
    fn a_clean_period_has_no_totals() {
        let found = detect(&[shape("aaaa")]);
        assert_eq!(found.total, 0);
        assert!(found.totals.is_empty());
    }

    #[test]
    fn every_flag_round_trips_through_its_stored_name() {
        for antipattern in Antipattern::STATIC
            .into_iter()
            .chain([Antipattern::Spilling, Antipattern::RepeatedRuns])
        {
            assert_eq!(Antipattern::parse(antipattern.as_str()), Some(antipattern));
        }
    }

    #[test]
    fn an_unknown_stored_flag_is_ignored() {
        assert_eq!(Antipattern::parse("invented_later"), None);
    }
}
