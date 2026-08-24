use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::storage_samples::StorageSampleService;
use crate::domain::entities::storage_samples::StorageSample;
use crate::domain::error::Result;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct StorageSampleRow {
    connection_id: Uuid,
    database_name: String,
    active_bytes: i64,
    historical_bytes: i64,
    retained_for_clone_bytes: i64,
    failsafe_bytes: i64,
    computed_at: DateTime<Utc>,
    ingested_at: DateTime<Utc>,
}

impl From<StorageSampleRow> for StorageSample {
    fn from(row: StorageSampleRow) -> Self {
        StorageSample {
            connection_id: row.connection_id,
            database_name: row.database_name,
            active_bytes: row.active_bytes,
            historical_bytes: row.historical_bytes,
            retained_for_clone_bytes: row.retained_for_clone_bytes,
            failsafe_bytes: row.failsafe_bytes,
            computed_at: row.computed_at,
            ingested_at: row.ingested_at,
        }
    }
}

pub struct PgStorageSampleService {
    db: PgPool,
}

impl PgStorageSampleService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl StorageSampleService for PgStorageSampleService {
    async fn upsert_batch(&self, samples: Vec<StorageSample>) -> Result<u64> {
        if samples.is_empty() {
            return Ok(0);
        }

        let mut connection_ids = Vec::with_capacity(samples.len());
        let mut database_names = Vec::with_capacity(samples.len());
        let mut active = Vec::with_capacity(samples.len());
        let mut historical = Vec::with_capacity(samples.len());
        let mut cloned = Vec::with_capacity(samples.len());
        let mut failsafe = Vec::with_capacity(samples.len());
        let mut computed = Vec::with_capacity(samples.len());
        let mut ingested = Vec::with_capacity(samples.len());

        for sample in samples {
            connection_ids.push(sample.connection_id);
            database_names.push(sample.database_name);
            active.push(sample.active_bytes);
            historical.push(sample.historical_bytes);
            cloned.push(sample.retained_for_clone_bytes);
            failsafe.push(sample.failsafe_bytes);
            computed.push(sample.computed_at);
            ingested.push(sample.ingested_at);
        }

        // MotherDuck recomputes storage on its own schedule, so the poller
        // sees the same measurement repeatedly; re-reading one is a no-op.
        let result = sqlx::query(
            "insert into storage_samples (connection_id, database_name, active_bytes,
                 historical_bytes, retained_for_clone_bytes, failsafe_bytes, computed_at,
                 ingested_at)
             select * from unnest(
                 $1::uuid[], $2::varchar[], $3::bigint[], $4::bigint[], $5::bigint[],
                 $6::bigint[], $7::timestamptz[], $8::timestamptz[])
             on conflict (connection_id, database_name, computed_at) do nothing",
        )
        .bind(&connection_ids)
        .bind(&database_names)
        .bind(&active)
        .bind(&historical)
        .bind(&cloned)
        .bind(&failsafe)
        .bind(&computed)
        .bind(&ingested)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    async fn latest_by_connection(&self, connection_id: Uuid) -> Result<Vec<StorageSample>> {
        // One row per database: whichever measurement is newest.
        let rows = sqlx::query_as::<_, StorageSampleRow>(
            "select distinct on (database_name)
                    connection_id, database_name, active_bytes, historical_bytes,
                    retained_for_clone_bytes, failsafe_bytes, computed_at, ingested_at
             from storage_samples
             where connection_id = $1
             order by database_name, computed_at desc",
        )
        .bind(connection_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(StorageSample::from).collect())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use chrono::Duration;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::motherduck_connections::MotherDuckConnectionService;
    use crate::application::services::organizations::OrganizationService;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::organizations::OrganizationDraft;
    use crate::domain::entities::pricing::RegionTier;
    use crate::domain::entities::storage_samples::StorageSampleDraft;
    use crate::domain::entities::users::{Email, PasswordHash, User};
    use crate::infrastructure::crypto::SecretCipher;
    use crate::infrastructure::pg::motherduck_connections::PgMotherDuckConnectionService;
    use crate::infrastructure::pg::organizations::PgOrganizationService;

    async fn seed_connection(pool: &Pool<Postgres>) -> Uuid {
        let now = Utc::now();
        let organization = OrganizationDraft::new("acme")
            .unwrap()
            .into_new_organization(now);
        let user = User::new(organization.id, Email::new("a@example.com").unwrap(), now);
        PgOrganizationService::new(pool.clone())
            .create_with_owner(organization.clone(), user, PasswordHash::new("h".into()))
            .await
            .unwrap();

        let cipher = Arc::new(SecretCipher::from_base64_key(&STANDARD.encode([9u8; 32])).unwrap());
        let (connection, token) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(organization.id, now);
        PgMotherDuckConnectionService::new(pool.clone(), cipher)
            .insert(connection, token)
            .await
            .unwrap()
            .id
    }

    fn sample(
        connection_id: Uuid,
        name: &str,
        active: i64,
        computed_at: DateTime<Utc>,
    ) -> StorageSample {
        StorageSampleDraft {
            database_name: name.to_string(),
            active_bytes: active,
            historical_bytes: 0,
            retained_for_clone_bytes: 0,
            failsafe_bytes: 0,
            computed_at,
        }
        .into_sample(connection_id, Utc::now())
    }

    #[sqlx::test]
    async fn latest_by_connection_returns_the_newest_per_database(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgStorageSampleService::new(pool);
        let now = Utc::now();

        service
            .upsert_batch(vec![
                sample(connection_id, "analytics", 100, now - Duration::hours(2)),
                sample(connection_id, "analytics", 300, now),
                sample(connection_id, "staging", 50, now),
            ])
            .await
            .unwrap();

        let latest = service.latest_by_connection(connection_id).await.unwrap();

        assert_eq!(latest.len(), 2);
        let analytics = latest
            .iter()
            .find(|s| s.database_name == "analytics")
            .unwrap();
        assert_eq!(analytics.active_bytes, 300);
    }

    #[sqlx::test]
    async fn re_reading_a_measurement_is_a_no_op(pool: Pool<Postgres>) {
        let connection_id = seed_connection(&pool).await;
        let service = PgStorageSampleService::new(pool);
        let computed_at = Utc::now();
        let batch = vec![sample(connection_id, "analytics", 100, computed_at)];

        assert_eq!(service.upsert_batch(batch.clone()).await.unwrap(), 1);
        assert_eq!(service.upsert_batch(batch).await.unwrap(), 0);
        assert_eq!(
            service
                .latest_by_connection(connection_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
