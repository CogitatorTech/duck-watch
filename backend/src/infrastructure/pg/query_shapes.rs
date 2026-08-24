use async_trait::async_trait;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::query_shapes::QueryShapeService;
use crate::domain::entities::insights::Antipattern;
use crate::domain::entities::query_shapes::{QueryShape, ShapeStatement, UnflaggedShape};
use crate::domain::error::Result;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct UnflaggedShapeRow {
    fingerprint: String,
    example_sql: String,
}

impl From<UnflaggedShapeRow> for UnflaggedShape {
    fn from(row: UnflaggedShapeRow) -> Self {
        UnflaggedShape {
            fingerprint: row.fingerprint,
            example_sql: row.example_sql,
        }
    }
}

/// PostgreSQL cannot unnest an array column alongside scalars, so each
/// shape's flags travel as one comma separated string and are split back
/// apart in SQL. The names never contain a comma.
fn join_flags(antipatterns: &[Antipattern]) -> String {
    antipatterns
        .iter()
        .map(Antipattern::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(sqlx::FromRow)]
struct ShapeStatementRow {
    fingerprint: String,
    example_sql: String,
    parsed: bool,
    first_seen: chrono::DateTime<chrono::Utc>,
}

impl From<ShapeStatementRow> for ShapeStatement {
    fn from(row: ShapeStatementRow) -> Self {
        ShapeStatement {
            fingerprint: row.fingerprint,
            example_sql: row.example_sql,
            parsed: row.parsed,
            first_seen: row.first_seen,
        }
    }
}

pub struct PgQueryShapeService {
    db: PgPool,
}

impl PgQueryShapeService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl QueryShapeService for PgQueryShapeService {
    async fn upsert_batch(&self, shapes: Vec<QueryShape>) -> Result<u64> {
        if shapes.is_empty() {
            return Ok(0);
        }

        let mut connection_ids = Vec::with_capacity(shapes.len());
        let mut fingerprints = Vec::with_capacity(shapes.len());
        let mut normalized = Vec::with_capacity(shapes.len());
        let mut examples = Vec::with_capacity(shapes.len());
        let mut parsed = Vec::with_capacity(shapes.len());
        let mut flags = Vec::with_capacity(shapes.len());
        let mut first_seen = Vec::with_capacity(shapes.len());

        for shape in shapes {
            connection_ids.push(shape.connection_id);
            fingerprints.push(shape.fingerprint);
            normalized.push(shape.normalized_sql);
            examples.push(shape.example_sql);
            parsed.push(shape.parsed);
            flags.push(join_flags(&shape.antipatterns));
            first_seen.push(shape.first_seen);
        }

        // A shape is written once. Later runs keep the original example and
        // first seen time, so the record of when a shape appeared survives.
        let result = sqlx::query(
            "insert into query_shapes (connection_id, fingerprint, normalized_sql, example_sql,
                 parsed, antipatterns, first_seen)
             select connection_id, fingerprint, normalized_sql, example_sql, parsed,
                    case when flags = '' then '{}'::varchar[] else string_to_array(flags, ',') end,
                    first_seen
             from unnest($1::uuid[], $2::varchar[], $3::text[], $4::text[], $5::bool[],
                 $6::text[], $7::timestamptz[])
                 as t(connection_id, fingerprint, normalized_sql, example_sql, parsed, flags,
                      first_seen)
             on conflict (connection_id, fingerprint) do nothing",
        )
        .bind(&connection_ids)
        .bind(&fingerprints)
        .bind(&normalized)
        .bind(&examples)
        .bind(&parsed)
        .bind(&flags)
        .bind(&first_seen)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    async fn find_statement(
        &self,
        connection_id: Uuid,
        fingerprint: &str,
    ) -> Result<ShapeStatement> {
        let row = sqlx::query_as::<_, ShapeStatementRow>(
            "select fingerprint, example_sql, parsed, first_seen
             from query_shapes
             where connection_id = $1 and fingerprint = $2",
        )
        .bind(connection_id)
        .bind(fingerprint)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }

    async fn find_unflagged(&self, connection_id: Uuid, limit: u32) -> Result<Vec<UnflaggedShape>> {
        let rows = sqlx::query_as::<_, UnflaggedShapeRow>(
            "select fingerprint, example_sql
             from query_shapes
             where connection_id = $1 and antipatterns is null
             order by first_seen
             limit $2",
        )
        .bind(connection_id)
        .bind(i64::from(limit))
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(UnflaggedShape::from).collect())
    }

    async fn set_antipatterns(
        &self,
        connection_id: Uuid,
        assignments: Vec<(String, Vec<Antipattern>)>,
    ) -> Result<u64> {
        if assignments.is_empty() {
            return Ok(0);
        }

        let mut fingerprints = Vec::with_capacity(assignments.len());
        let mut flags = Vec::with_capacity(assignments.len());
        for (fingerprint, antipatterns) in &assignments {
            fingerprints.push(fingerprint.clone());
            flags.push(join_flags(antipatterns));
        }

        // A shape with no flags stores an empty array rather than null, so
        // the backfill does not keep picking it up.
        let result = sqlx::query(
            "update query_shapes as shapes
             set antipatterns = case
                 when assigned.flags = '' then '{}'::varchar[]
                 else string_to_array(assigned.flags, ',')
             end
             from unnest($2::varchar[], $3::text[]) as assigned(fingerprint, flags)
             where shapes.connection_id = $1 and shapes.fingerprint = assigned.fingerprint",
        )
        .bind(connection_id)
        .bind(&fingerprints)
        .bind(&flags)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use chrono::Utc;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::motherduck_connections::MotherDuckConnectionService;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::pricing::RegionTier;
    use crate::infrastructure::crypto::SecretCipher;
    use crate::infrastructure::pg::motherduck_connections::PgMotherDuckConnectionService;

    async fn seed_connection(pool: &Pool<Postgres>) -> Uuid {
        let cipher = Arc::new(SecretCipher::from_base64_key(&STANDARD.encode([9u8; 32])).unwrap());
        let (connection, token) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(Utc::now());
        PgMotherDuckConnectionService::new(pool.clone(), cipher)
            .insert(connection, token)
            .await
            .unwrap()
            .id
    }

    fn shape(connection_id: Uuid, fingerprint: &str, antipatterns: Vec<Antipattern>) -> QueryShape {
        QueryShape {
            connection_id,
            fingerprint: fingerprint.to_string(),
            normalized_sql: "select * from t".into(),
            example_sql: "select * from t".into(),
            parsed: true,
            antipatterns,
            first_seen: Utc::now(),
        }
    }

    #[sqlx::test]
    async fn flags_written_with_a_new_shape_come_back(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool);

        service
            .upsert_batch(vec![shape(
                connection_id,
                "aaaa",
                vec![Antipattern::SelectStar, Antipattern::NoFilter],
            )])
            .await
            .unwrap();

        // Written with flags, so the backfill must not pick it up again.
        assert!(
            service
                .find_unflagged(connection_id, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test]
    async fn a_shape_with_no_flags_is_not_offered_to_the_backfill_twice(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool);

        service
            .upsert_batch(vec![shape(connection_id, "aaaa", vec![])])
            .await
            .unwrap();

        assert!(
            service
                .find_unflagged(connection_id, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test]
    async fn find_statement_returns_the_whole_query(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool);

        // Longer than the cut the shape lists apply, which is the reason
        // this read exists.
        let long = format!("select {}", "x".repeat(5000));
        let mut stored = shape(connection_id, "aaaa", vec![]);
        stored.example_sql = long.clone();
        service.upsert_batch(vec![stored]).await.unwrap();

        let found = service.find_statement(connection_id, "aaaa").await.unwrap();

        assert_eq!(found.fingerprint, "aaaa");
        assert_eq!(found.example_sql, long);
        assert!(found.parsed);
    }

    #[sqlx::test]
    async fn find_statement_does_not_reach_another_connection(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool);
        service
            .upsert_batch(vec![shape(connection_id, "aaaa", vec![])])
            .await
            .unwrap();

        assert!(
            service
                .find_statement(Uuid::new_v4(), "aaaa")
                .await
                .is_err()
        );
        assert!(service.find_statement(connection_id, "zzzz").await.is_err());
    }

    #[sqlx::test]
    async fn the_backfill_finds_and_then_clears_unexamined_shapes(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool.clone());
        service
            .upsert_batch(vec![shape(connection_id, "aaaa", vec![])])
            .await
            .unwrap();
        // Return the row to the state a release before this column left it.
        sqlx::query("update query_shapes set antipatterns = null")
            .execute(&pool)
            .await
            .unwrap();

        let pending = service.find_unflagged(connection_id, 10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].fingerprint, "aaaa");

        let written = service
            .set_antipatterns(
                connection_id,
                vec![("aaaa".to_string(), vec![Antipattern::SelectStar])],
            )
            .await
            .unwrap();

        assert_eq!(written, 1);
        assert!(
            service
                .find_unflagged(connection_id, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test]
    async fn a_shape_examined_and_found_clean_is_not_revisited(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgQueryShapeService::new(pool.clone());
        service
            .upsert_batch(vec![shape(connection_id, "aaaa", vec![])])
            .await
            .unwrap();
        sqlx::query("update query_shapes set antipatterns = null")
            .execute(&pool)
            .await
            .unwrap();

        service
            .set_antipatterns(connection_id, vec![("aaaa".to_string(), vec![])])
            .await
            .unwrap();

        assert!(
            service
                .find_unflagged(connection_id, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
