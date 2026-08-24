use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::domain::entities::insights::Antipattern;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Identifies a family of queries that differ only in their literals, so a
/// statement run a thousand times with different dates counts once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryFingerprint(String);

/// Sixteen hex characters is short enough to index comfortably and long
/// enough that a collision needs billions of distinct shapes.
const FINGERPRINT_BYTES: usize = 8;

impl QueryFingerprint {
    /// Hashes an already normalized form. Callers normalize first, either
    /// from a parsed statement or with `normalize_without_parser` below.
    pub fn from_normalized(normalized: &str) -> Self {
        let digest = Sha256::digest(normalized.as_bytes());
        Self(hex(&digest[..FINGERPRINT_BYTES]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut acc, byte| {
        use std::fmt::Write;
        // Writing to a String cannot fail.
        let _ = write!(acc, "{byte:02x}");
        acc
    })
}

/// Normalizes a statement without parsing it: comments go, literals become
/// `?`, and whitespace and case are flattened. This is the fallback for
/// statements the parser rejects, and it produces the readable form stored
/// alongside every shape.
pub fn normalize_without_parser(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();

    while let Some(current) = chars.next() {
        match current {
            // A line comment runs to the end of the line.
            '-' if chars.peek() == Some(&'-') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                push_space(&mut out);
            }
            // A block comment runs to its terminator, or to the end of an
            // unterminated statement.
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                push_space(&mut out);
            }
            // A string literal, where '' escapes a quote.
            '\'' => {
                loop {
                    match chars.next() {
                        Some('\'') => {
                            if chars.peek() == Some(&'\'') {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                out.push('?');
            }
            // A quoted identifier names a real column, so it is kept.
            '"' => {
                out.push('"');
                loop {
                    match chars.next() {
                        Some('"') => {
                            out.push('"');
                            if chars.peek() == Some(&'"') {
                                chars.next();
                                out.push('"');
                            } else {
                                break;
                            }
                        }
                        Some(other) => out.push(other),
                        None => break,
                    }
                }
            }
            // A digit is a literal only when it does not continue an
            // identifier, so a table called `tbiz104` survives intact.
            digit if digit.is_ascii_digit() && !continues_identifier(&out) => {
                while let Some(next) = chars.peek() {
                    if next.is_ascii_digit() || *next == '.' {
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push('?');
            }
            space if space.is_whitespace() => push_space(&mut out),
            other => out.push(other.to_ascii_lowercase()),
        }
    }

    collapse_placeholder_lists(out.trim())
}

fn push_space(out: &mut String) {
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
}

fn continues_identifier(out: &str) -> bool {
    out.chars()
        .last()
        .is_some_and(|last| last.is_alphanumeric() || last == '_')
}

/// Collapses `?, ?, ?` to a single `?`, so lists of different lengths share
/// one shape.
fn collapse_placeholder_lists(sql: &str) -> String {
    let mut out = sql.to_string();
    while out.contains("?, ?") {
        out = out.replace("?, ?", "?");
    }
    out
}

/// What analysis learned about one statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlAnalysis {
    pub fingerprint: QueryFingerprint,
    /// A readable normalized form, kept for display and debugging.
    pub normalized_sql: String,
    /// False when the statement could not be parsed and the fallback ran.
    pub parsed: bool,
    /// Habits read off the statement itself, such as `select *`.
    pub antipatterns: Vec<Antipattern>,
}

/// One query shape as stored, with a real example so the interface can show
/// something a person recognizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryShape {
    pub connection_id: Uuid,
    pub fingerprint: String,
    pub normalized_sql: String,
    pub example_sql: String,
    pub parsed: bool,
    pub antipatterns: Vec<Antipattern>,
    pub first_seen: DateTime<Utc>,
}

/// A stored event still waiting for a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfingerprintedQuery {
    pub md_query_id: Uuid,
    pub query_text: String,
    pub start_time: DateTime<Utc>,
}

/// One shape's full statement. List responses cut long statements to keep the
/// payload small, so the copy action reads the whole one from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct ShapeStatement {
    pub fingerprint: String,
    pub example_sql: String,
    /// False when the statement could not be parsed and the text fallback ran.
    pub parsed: bool,
    pub first_seen: DateTime<Utc>,
}

/// A stored shape that has not been examined for anti-patterns yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnflaggedShape {
    pub fingerprint: String,
    /// A real statement from the family, which is what gets re-analyzed.
    pub example_sql: String,
}

/// One group and Duckling size pair for a shape, which the application prices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeCell {
    pub fingerprint: String,
    pub instance_type: String,
    pub example_sql: String,
    pub runs: i64,
    pub failure_count: i64,
    pub total_ms: i64,
    pub max_ms: i64,
    /// What these runs wrote to disk when they ran out of memory.
    pub bytes_spilled: i64,
    /// The flags read off this shape's statement when it was analyzed. They
    /// belong to the shape rather than the cell, so every cell of one shape
    /// carries the same list.
    pub antipatterns: Vec<Antipattern>,
    pub last_seen: DateTime<Utc>,
}

/// What one query shape accounted for over the period.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct ShapeStats {
    pub fingerprint: String,
    pub example_sql: String,
    pub runs: i64,
    pub failure_count: i64,
    pub total_ms: i64,
    pub max_ms: i64,
    pub bytes_spilled: i64,
    pub antipatterns: Vec<Antipattern>,
    pub estimated_cost_usd: f64,
    /// Share of the period's estimated cost, between 0 and 1.
    pub cost_share: f64,
    pub last_seen: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_become_placeholders() {
        let normalized = normalize_without_parser("select a from t where d = '2026-06-01'");
        assert_eq!(normalized, "select a from t where d = ?");
    }

    #[test]
    fn statements_differing_only_in_literals_share_a_fingerprint() {
        let one = normalize_without_parser("select a from t where d = '2026-06-01' and n = 42");
        let two = normalize_without_parser("select a from t where d = '2026-07-01' and n = 99");
        assert_eq!(one, two);
        assert_eq!(
            QueryFingerprint::from_normalized(&one),
            QueryFingerprint::from_normalized(&two)
        );
    }

    #[test]
    fn comments_case_and_whitespace_do_not_matter() {
        let plain = normalize_without_parser("select a from t");
        let dressed = normalize_without_parser("-- a note\nSELECT   a\n  FROM t /* trailing */");
        assert_eq!(plain, dressed);
    }

    #[test]
    fn lists_of_different_lengths_share_a_shape() {
        let short = normalize_without_parser("select a from t where n in (1, 2, 3)");
        let long = normalize_without_parser("select a from t where n in (4, 5, 6, 7, 8)");
        assert_eq!(short, long);
        assert_eq!(short, "select a from t where n in (?)");
    }

    #[test]
    fn a_different_table_is_a_different_shape() {
        let one = normalize_without_parser("select a from t where n = 1");
        let two = normalize_without_parser("select a from other where n = 1");
        assert_ne!(
            QueryFingerprint::from_normalized(&one),
            QueryFingerprint::from_normalized(&two)
        );
    }

    #[test]
    fn a_digit_inside_an_identifier_survives() {
        let normalized = normalize_without_parser("select * from apollo.tbiz104.__transactions");
        assert_eq!(normalized, "select * from apollo.tbiz104.__transactions");
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_literal() {
        let normalized = normalize_without_parser("select 'it''s fine' as note, 1");
        assert_eq!(normalized, "select ? as note, ?");
    }

    #[test]
    fn an_unterminated_literal_does_not_hang() {
        assert_eq!(normalize_without_parser("select 'unterminated"), "select ?");
        assert_eq!(normalize_without_parser("select 1 /* open"), "select ?");
    }

    #[test]
    fn a_quoted_identifier_keeps_its_case() {
        let normalized = normalize_without_parser("select \"MixedCase\" from t");
        assert_eq!(normalized, "select \"MixedCase\" from t");
    }

    #[test]
    fn a_fingerprint_is_sixteen_hex_characters() {
        let fingerprint = QueryFingerprint::from_normalized("select ?");
        assert_eq!(fingerprint.as_str().len(), 16);
        assert!(fingerprint.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }
}
