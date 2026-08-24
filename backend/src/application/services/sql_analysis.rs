use async_trait::async_trait;

use crate::domain::entities::query_shapes::SqlAnalysis;

/// Turns statements into shape fingerprints. Parsing is CPU work on a
/// blocking library, so implementations take a whole batch and the caller
/// hands over one poll's worth at a time.
#[async_trait]
pub trait SqlAnalyzer: Send + Sync {
    /// Analyzes every statement, returning results aligned with the input by
    /// index. A statement that cannot be parsed still gets a fingerprint from
    /// the text based fallback, so nothing is left ungrouped, and still gets
    /// whatever flags can be read off its text.
    async fn analyze_batch(&self, statements: Vec<String>) -> Vec<SqlAnalysis>;
}

#[cfg(test)]
mockall::mock! {
    pub SqlAnalyzer {}
    #[async_trait]
    impl SqlAnalyzer for SqlAnalyzer {
        async fn analyze_batch(&self, statements: Vec<String>) -> Vec<SqlAnalysis>;
    }
}
