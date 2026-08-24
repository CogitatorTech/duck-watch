use async_trait::async_trait;
use serde_json::Value;

use crate::application::services::sql_analysis::SqlAnalyzer;
use crate::domain::entities::insights::Antipattern;
use crate::domain::entities::query_shapes::{
    QueryFingerprint, SqlAnalysis, normalize_without_parser,
};

/// Fingerprints statements by parsing them with DuckDB itself, which is the
/// same dialect MotherDuck speaks. The parser runs locally in memory, so this
/// needs no token and no network.
pub struct DuckDbSqlAnalyzer;

/// Source offsets would make two identically shaped statements differ purely
/// because of formatting, so they are stripped before hashing.
const POSITION_KEY: &str = "query_location";

fn serialize_sql(
    connection: &duckdb::Connection,
    sql: &str,
) -> std::result::Result<String, duckdb::Error> {
    // The function takes a constant rather than a bound parameter, so the
    // statement is escaped and inlined. It is parsed, never executed.
    let escaped = sql.replace('\'', "''");
    connection.query_row(
        &format!("select json_serialize_sql('{escaped}')"),
        [],
        |row| row.get::<_, String>(0),
    )
}

/// Blanks every literal, drops source offsets, and collapses runs of
/// identical siblings so lists of different lengths agree.
fn blank_literals(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("class").and_then(Value::as_str) == Some("CONSTANT")
                && let Some(Value::Object(inner)) = map.get_mut("value")
            {
                inner.insert("value".to_string(), Value::String("?".to_string()));
            }
            map.remove(POSITION_KEY);
            for (_, child) in map.iter_mut() {
                blank_literals(child);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                blank_literals(item);
            }
            items.dedup();
        }
        _ => {}
    }
}

/// The parsed shape of a statement and the flags it carries, or `None` when
/// DuckDB will not serialize it. DuckDB serializes SELECT statements only, so
/// a statement such as `create table ... as select ...` takes the text
/// fallback, which still removes comments, formatting, case, and literals.
///
/// The flags are read after the literals are blanked, so they describe the
/// shape rather than any one run of it.
fn parsed_shape(connection: &duckdb::Connection, sql: &str) -> Option<(String, Vec<Antipattern>)> {
    let json = serialize_sql(connection, sql).ok()?;
    let mut value: Value = serde_json::from_str(&json).ok()?;
    // A parse failure comes back as data rather than as an error.
    if value.get("error").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    blank_literals(&mut value);
    let antipatterns = parsed_antipatterns(&value);
    Some((value.to_string(), antipatterns))
}

/// Walks the whole tree looking for a node whose `key` equals `value`, which
/// is how the flags that are worth raising anywhere in a statement, such as a
/// cross join inside a subquery, are found.
fn contains_node(value: &Value, key: &str, expected: &str) -> bool {
    match value {
        Value::Object(map) => {
            if map.get(key).and_then(Value::as_str) == Some(expected) {
                return true;
            }
            map.values()
                .any(|child| contains_node(child, key, expected))
        }
        Value::Array(items) => items.iter().any(|item| contains_node(item, key, expected)),
        _ => false,
    }
}

/// The statement's own select node, which is where a missing `where` clause
/// or an unbounded sort actually matters. A subquery without a filter is
/// usually deliberate, so those are left alone.
fn top_level_select(ast: &Value) -> Option<&Value> {
    let node = ast.get("statements")?.get(0)?.get("node")?;
    match node.get("type").and_then(Value::as_str) {
        Some("SELECT_NODE") => Some(node),
        _ => None,
    }
}

fn has_modifier(node: &Value, modifier: &str) -> bool {
    node.get("modifiers")
        .and_then(Value::as_array)
        .is_some_and(|modifiers| {
            modifiers
                .iter()
                .any(|entry| entry.get("type").and_then(Value::as_str) == Some(modifier))
        })
}

/// Reads the flags a parsed statement carries. Verified against DuckDB's own
/// serialized form: a `select *` column is `class: "STAR"`, a comma join is
/// `ref_type: "CROSS"` while its `join_type` still reads `INNER`, and `order
/// by` and `limit` appear as `ORDER_MODIFIER` and `LIMIT_MODIFIER` entries.
fn parsed_antipatterns(ast: &Value) -> Vec<Antipattern> {
    let mut found = Vec::new();

    if contains_node(ast, "class", "STAR") {
        found.push(Antipattern::SelectStar);
    }
    if contains_node(ast, "ref_type", "CROSS") {
        found.push(Antipattern::CrossJoin);
    }

    if let Some(node) = top_level_select(ast) {
        // `from_table` is present but typed `EMPTY` for a select with no
        // from clause, which has nothing to filter.
        let reads_a_table = node
            .get("from_table")
            .and_then(|from| from.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "EMPTY");

        if reads_a_table && node.get("where_clause").is_some_and(Value::is_null) {
            found.push(Antipattern::NoFilter);
        }
        if has_modifier(node, "ORDER_MODIFIER") && !has_modifier(node, "LIMIT_MODIFIER") {
            found.push(Antipattern::OrderWithoutLimit);
        }
    }

    found
}

/// The same flags read off normalized text, for the statements DuckDB will
/// not serialize. Only the ones that can be spotted reliably without a parse
/// are attempted; the rest are simply not reported for these statements.
fn text_antipatterns(normalized: &str) -> Vec<Antipattern> {
    let mut found = Vec::new();

    // `normalize_without_parser` has already lowercased the text, collapsed
    // its whitespace, and blanked its literals.
    if normalized.contains("select *") {
        found.push(Antipattern::SelectStar);
    }
    if normalized.contains(" order by ") && !normalized.contains(" limit ") {
        found.push(Antipattern::OrderWithoutLimit);
    }

    found
}

fn analyze_blocking(statements: &[String]) -> Vec<SqlAnalysis> {
    let connection = duckdb::Connection::open_in_memory().ok();

    statements
        .iter()
        .map(|sql| {
            let normalized_sql = normalize_without_parser(sql);
            let parsed = connection
                .as_ref()
                .and_then(|connection| parsed_shape(connection, sql));

            match parsed {
                Some((shape, antipatterns)) => SqlAnalysis {
                    fingerprint: QueryFingerprint::from_normalized(&shape),
                    normalized_sql,
                    parsed: true,
                    antipatterns,
                },
                None => SqlAnalysis {
                    antipatterns: text_antipatterns(&normalized_sql),
                    fingerprint: QueryFingerprint::from_normalized(&normalized_sql),
                    normalized_sql,
                    parsed: false,
                },
            }
        })
        .collect()
}

#[async_trait]
impl SqlAnalyzer for DuckDbSqlAnalyzer {
    async fn analyze_batch(&self, statements: Vec<String>) -> Vec<SqlAnalysis> {
        match tokio::task::spawn_blocking(move || analyze_blocking(&statements)).await {
            Ok(analyses) => analyses,
            Err(err) => {
                tracing::error!("sql analysis task failed: {err}");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(statements: &[&str]) -> Vec<SqlAnalysis> {
        analyze_blocking(&statements.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn statements_differing_only_in_literals_share_a_fingerprint() {
        let analyses = shapes(&[
            "select a from t where d = '2026-06-01' and n = 42",
            "select a from t where d = '2026-07-01' and n = 99",
        ]);

        assert!(analyses.iter().all(|analysis| analysis.parsed));
        assert_eq!(analyses[0].fingerprint, analyses[1].fingerprint);
    }

    #[test]
    fn formatting_and_comments_do_not_change_the_shape() {
        let analyses = shapes(&[
            "select a from t where n = 1",
            "-- a note\nSELECT   a\n  FROM t\n  WHERE n = 1 /* trailing */",
        ]);
        assert_eq!(analyses[0].fingerprint, analyses[1].fingerprint);
    }

    #[test]
    fn lists_of_different_lengths_share_a_shape() {
        let analyses = shapes(&[
            "select a from t where n in (1, 2, 3)",
            "select a from t where n in (4, 5, 6, 7, 8)",
        ]);
        assert_eq!(analyses[0].fingerprint, analyses[1].fingerprint);
    }

    #[test]
    fn a_different_table_is_a_different_shape() {
        let analyses = shapes(&[
            "select a from t where n = 1",
            "select a from other where n = 1",
        ]);
        assert_ne!(analyses[0].fingerprint, analyses[1].fingerprint);
    }

    #[test]
    fn duckdb_specific_syntax_parses() {
        let analyses = shapes(&[
            "select unnest(cast(x as json[])) as v from apollo.tc3db.__creates \
             qualify row_number() over () = 1",
        ]);
        assert!(analyses[0].parsed, "duckdb dialect must parse");
    }

    #[test]
    fn statements_that_are_not_selects_group_by_text() {
        // DuckDB serializes only SELECT statements, so these take the
        // fallback, which must still ignore literals and formatting.
        let analyses = shapes(&[
            "create or replace table report as select a from t where d = '2026-06-01'",
            "CREATE OR REPLACE TABLE report AS\n  SELECT a FROM t WHERE d = '2026-07-01'",
            "create or replace table other as select a from t where d = '2026-06-01'",
        ]);

        assert!(!analyses[0].parsed, "a ctas statement takes the fallback");
        assert_eq!(
            analyses[0].fingerprint, analyses[1].fingerprint,
            "literals and formatting must not split a shape"
        );
        assert_ne!(
            analyses[0].fingerprint, analyses[2].fingerprint,
            "a different target table is a different shape"
        );
    }

    #[test]
    fn an_unparseable_statement_still_gets_a_fingerprint() {
        let analyses = shapes(&["select from where", "select from where"]);

        assert!(!analyses[0].parsed, "the fallback should have run");
        // Two identical unparseable statements still group together.
        assert_eq!(analyses[0].fingerprint, analyses[1].fingerprint);
        assert!(!analyses[0].fingerprint.as_str().is_empty());
    }

    fn flags(sql: &str) -> Vec<Antipattern> {
        shapes(&[sql]).remove(0).antipatterns
    }

    #[test]
    fn select_star_is_flagged() {
        assert!(flags("select * from t where a = 1").contains(&Antipattern::SelectStar));
        assert!(!flags("select a, b from t where a = 1").contains(&Antipattern::SelectStar));
    }

    #[test]
    fn a_comma_join_is_flagged_as_a_cross_join() {
        // DuckDB still reports `join_type: INNER` for a comma join, so the
        // flag has to come from `ref_type` instead.
        assert!(flags("select a from t, u where t.id = u.id").contains(&Antipattern::CrossJoin));
    }

    #[test]
    fn a_join_with_a_condition_is_not_a_cross_join() {
        assert!(!flags("select a from t join u on t.id = u.id").contains(&Antipattern::CrossJoin));
    }

    #[test]
    fn a_select_over_a_table_with_no_where_is_flagged() {
        assert!(flags("select a from t").contains(&Antipattern::NoFilter));
        assert!(!flags("select a from t where a = 1").contains(&Antipattern::NoFilter));
    }

    #[test]
    fn a_select_with_no_table_is_not_flagged_as_unfiltered() {
        // There is nothing to filter, so this must not be reported.
        assert!(!flags("select 1").contains(&Antipattern::NoFilter));
    }

    #[test]
    fn a_filtered_subquery_does_not_clear_an_unfiltered_outer_select() {
        let found = flags("select a from (select b from u where b = 1)");
        assert!(found.contains(&Antipattern::NoFilter));
    }

    #[test]
    fn an_unfiltered_subquery_alone_is_not_flagged() {
        // Only the statement's own select matters; an inner scan is usually
        // deliberate, and flagging it would bury the findings that matter.
        let found = flags("select a from (select b from u) where a = 1");
        assert!(!found.contains(&Antipattern::NoFilter));
    }

    #[test]
    fn sorting_without_a_limit_is_flagged() {
        assert!(flags("select a from t order by a").contains(&Antipattern::OrderWithoutLimit));
        assert!(
            !flags("select a from t order by a limit 10").contains(&Antipattern::OrderWithoutLimit)
        );
    }

    #[test]
    fn a_shapes_flags_do_not_depend_on_its_literals() {
        assert_eq!(
            flags("select * from t where d = '2026-06-01'"),
            flags("select * from t where d = '2026-07-01'")
        );
    }

    #[test]
    fn an_unparseable_statement_still_gets_the_flags_its_text_shows() {
        // A create-table-as statement takes the text fallback, which can
        // still see the `select *` inside it.
        let found = flags("create or replace table report as select * from t");
        assert!(found.contains(&Antipattern::SelectStar));
    }

    #[test]
    fn the_text_fallback_does_not_guess_at_the_flags_it_cannot_see() {
        // Without a parse there is no reliable way to tell a cross join or an
        // unfiltered scan from a filtered one, so neither is reported.
        let found = flags("create or replace table report as select a from t, u");
        assert!(!found.contains(&Antipattern::CrossJoin));
        assert!(!found.contains(&Antipattern::NoFilter));
    }

    #[test]
    fn a_clean_statement_carries_no_flags() {
        assert_eq!(flags("select a, b from t where a = 1 limit 10"), vec![]);
    }

    #[test]
    fn a_statement_with_quotes_cannot_break_out_of_the_call() {
        // The statement is escaped before being inlined into the parse call.
        let analyses = shapes(&["select 'it''s fine', ') , 1) --'"]);
        assert_eq!(analyses.len(), 1);
    }
}
