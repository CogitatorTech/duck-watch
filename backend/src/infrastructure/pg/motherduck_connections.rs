use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::motherduck_connections::{
    MotherDuckConnectionService, SyncState,
};
use crate::domain::entities::motherduck_connections::{MotherDuckConnection, MotherDuckToken};
use crate::domain::entities::pricing::RegionTier;
use crate::domain::error::Result;
use crate::infrastructure::crypto::SecretCipher;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive. The token columns are only selected by
/// `get_token`, which decrypts them on the way out.
#[derive(sqlx::FromRow)]
struct ConnectionRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    region_tier: String,
    enabled: bool,
    watermark_start_time: Option<DateTime<Utc>>,
    last_synced_at: Option<DateTime<Utc>>,
    last_success_at: Option<DateTime<Utc>>,
    last_sync_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ConnectionRow> for MotherDuckConnection {
    fn from(row: ConnectionRow) -> Self {
        MotherDuckConnection {
            id: row.id,
            org_id: row.org_id,
            name: row.name,
            // A tier written by an older release, or by hand, falls back to
            // the default rather than failing the read.
            region_tier: RegionTier::parse(&row.region_tier).unwrap_or_default(),
            enabled: row.enabled,
            watermark_start_time: row.watermark_start_time,
            last_synced_at: row.last_synced_at,
            last_success_at: row.last_success_at,
            last_sync_error: row.last_sync_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    token_ciphertext: Vec<u8>,
    token_nonce: Vec<u8>,
}

pub struct PgMotherDuckConnectionService {
    db: PgPool,
    cipher: Arc<SecretCipher>,
}

impl PgMotherDuckConnectionService {
    pub fn new(db: PgPool, cipher: Arc<SecretCipher>) -> Self {
        Self { db, cipher }
    }
}

#[async_trait]
impl MotherDuckConnectionService for PgMotherDuckConnectionService {
    async fn find_all_by_org(&self, org_id: Uuid) -> Result<Vec<MotherDuckConnection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "select id, org_id, name, region_tier, enabled, watermark_start_time, last_synced_at,
                    last_success_at, last_sync_error, created_at, updated_at
             from motherduck_connections
             where org_id = $1
             order by created_at desc",
        )
        .bind(org_id)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(MotherDuckConnection::from).collect())
    }

    async fn find_by_id_and_org(&self, id: Uuid, org_id: Uuid) -> Result<MotherDuckConnection> {
        let row = sqlx::query_as::<_, ConnectionRow>(
            "select id, org_id, name, region_tier, enabled, watermark_start_time, last_synced_at,
                    last_success_at, last_sync_error, created_at, updated_at
             from motherduck_connections
             where id = $1 and org_id = $2",
        )
        .bind(id)
        .bind(org_id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }

    async fn insert(
        &self,
        connection: MotherDuckConnection,
        token: MotherDuckToken,
    ) -> Result<MotherDuckConnection> {
        let (ciphertext, nonce) = self.cipher.encrypt(token.reveal())?;

        let row = sqlx::query_as::<_, ConnectionRow>(
            "insert into motherduck_connections
                 (id, org_id, name, token_ciphertext, token_nonce, region_tier, enabled,
                  watermark_start_time, last_synced_at, last_success_at, last_sync_error,
                  created_at, updated_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             returning id, org_id, name, region_tier, enabled, watermark_start_time,
                       last_synced_at, last_success_at, last_sync_error, created_at,
                       updated_at",
        )
        .bind(connection.id)
        .bind(connection.org_id)
        .bind(&connection.name)
        .bind(&ciphertext)
        .bind(&nonce)
        .bind(connection.region_tier.as_str())
        .bind(connection.enabled)
        .bind(connection.watermark_start_time)
        .bind(connection.last_synced_at)
        .bind(connection.last_success_at)
        .bind(&connection.last_sync_error)
        .bind(connection.created_at)
        .bind(connection.updated_at)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }

    async fn delete(&self, id: Uuid, org_id: Uuid) -> Result<()> {
        sqlx::query_as::<_, ConnectionRow>(
            "delete from motherduck_connections
             where id = $1 and org_id = $2
             returning id, org_id, name, region_tier, enabled, watermark_start_time,
                       last_synced_at, last_success_at, last_sync_error, created_at,
                       updated_at",
        )
        .bind(id)
        .bind(org_id)
        .fetch_one(&self.db)
        .await?;

        Ok(())
    }

    async fn find_enabled(&self) -> Result<Vec<MotherDuckConnection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(
            "select id, org_id, name, region_tier, enabled, watermark_start_time, last_synced_at,
                    last_success_at, last_sync_error, created_at, updated_at
             from motherduck_connections
             where enabled
             order by created_at",
        )
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().map(MotherDuckConnection::from).collect())
    }

    async fn get_token(&self, id: Uuid) -> Result<MotherDuckToken> {
        let row = sqlx::query_as::<_, TokenRow>(
            "select token_ciphertext, token_nonce
             from motherduck_connections
             where id = $1",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;

        let plaintext = self
            .cipher
            .decrypt(&row.token_ciphertext, &row.token_nonce)?;
        MotherDuckToken::new(&plaintext)
    }

    async fn update_sync_state(&self, id: Uuid, state: SyncState) -> Result<()> {
        sqlx::query(
            "update motherduck_connections
             set watermark_start_time = $2,
                 last_synced_at = $3,
                 last_success_at = coalesce($4, last_success_at),
                 last_sync_error = $5,
                 updated_at = $3
             where id = $1",
        )
        .bind(id)
        .bind(state.watermark_start_time)
        .bind(state.last_synced_at)
        .bind(state.last_success_at)
        .bind(&state.last_sync_error)
        .execute(&self.db)
        .await?;

        Ok(())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::organizations::OrganizationService;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::organizations::{Organization, OrganizationDraft};
    use crate::domain::entities::users::{Email, PasswordHash, User};
    use crate::infrastructure::pg::organizations::PgOrganizationService;

    fn cipher() -> Arc<SecretCipher> {
        Arc::new(SecretCipher::from_base64_key(&STANDARD.encode([9u8; 32])).unwrap())
    }

    async fn seed_org(pool: &Pool<Postgres>, name: &str, email: &str) -> Organization {
        let now = Utc::now();
        let organization = OrganizationDraft::new(name)
            .unwrap()
            .into_new_organization(now);
        let user = User::new(organization.id, Email::new(email).unwrap(), now);
        PgOrganizationService::new(pool.clone())
            .create_with_owner(organization.clone(), user, PasswordHash::new("h".into()))
            .await
            .unwrap();
        organization
    }

    fn sample_connection(org_id: Uuid) -> (MotherDuckConnection, MotherDuckToken) {
        ConnectionDraft::new("prod", "md-token", RegionTier::Tier2)
            .unwrap()
            .into_new_connection(org_id, Utc::now())
    }

    #[sqlx::test]
    async fn insert_then_get_token_round_trips_the_secret(pool: Pool<Postgres>) {
        let org = seed_org(&pool, "acme", "a@example.com").await;
        let service = PgMotherDuckConnectionService::new(pool, cipher());
        let (connection, token) = sample_connection(org.id);

        let inserted = service.insert(connection, token).await.unwrap();

        let decrypted = service.get_token(inserted.id).await.unwrap();
        assert_eq!(decrypted.reveal(), "md-token");
        assert_eq!(inserted.region_tier, RegionTier::Tier2);
    }

    #[sqlx::test]
    async fn find_all_by_org_scopes_to_the_organization(pool: Pool<Postgres>) {
        let org_a = seed_org(&pool, "acme", "a@example.com").await;
        let org_b = seed_org(&pool, "buzz", "b@example.com").await;
        let service = PgMotherDuckConnectionService::new(pool, cipher());
        let (connection, token) = sample_connection(org_a.id);
        let inserted = service.insert(connection, token).await.unwrap();

        assert_eq!(
            service.find_all_by_org(org_a.id).await.unwrap(),
            vec![inserted.clone()]
        );
        assert_eq!(service.find_all_by_org(org_b.id).await.unwrap(), vec![]);
        assert!(
            service
                .find_by_id_and_org(inserted.id, org_b.id)
                .await
                .is_err()
        );
    }

    #[sqlx::test]
    async fn update_sync_state_moves_the_watermark(pool: Pool<Postgres>) {
        let org = seed_org(&pool, "acme", "a@example.com").await;
        let service = PgMotherDuckConnectionService::new(pool, cipher());
        let (connection, token) = sample_connection(org.id);
        let inserted = service.insert(connection, token).await.unwrap();

        let now = crate::infrastructure::pg::organizations::integration_tests::trunc_now();
        service
            .update_sync_state(
                inserted.id,
                SyncState {
                    watermark_start_time: Some(now),
                    last_synced_at: now,
                    last_success_at: Some(now),
                    last_sync_error: None,
                },
            )
            .await
            .unwrap();

        let found = service
            .find_by_id_and_org(inserted.id, org.id)
            .await
            .unwrap();
        assert_eq!(found.watermark_start_time, Some(now));
        assert_eq!(found.last_synced_at, Some(now));
        assert_eq!(found.last_success_at, Some(now));
        assert_eq!(found.last_sync_error, None);
    }

    #[sqlx::test]
    async fn a_failed_sync_keeps_the_last_success(pool: Pool<Postgres>) {
        let org = seed_org(&pool, "acme", "a@example.com").await;
        let service = PgMotherDuckConnectionService::new(pool, cipher());
        let (connection, token) = sample_connection(org.id);
        let inserted = service.insert(connection, token).await.unwrap();

        let succeeded_at = crate::infrastructure::pg::organizations::integration_tests::trunc_now();
        service
            .update_sync_state(
                inserted.id,
                SyncState {
                    watermark_start_time: Some(succeeded_at),
                    last_synced_at: succeeded_at,
                    last_success_at: Some(succeeded_at),
                    last_sync_error: None,
                },
            )
            .await
            .unwrap();

        // A later failure passes no success time, which must leave the
        // recorded one alone; otherwise nothing shows how long the data has
        // been stale.
        let failed_at = succeeded_at + chrono::Duration::minutes(5);
        service
            .update_sync_state(
                inserted.id,
                SyncState {
                    watermark_start_time: Some(succeeded_at),
                    last_synced_at: failed_at,
                    last_success_at: None,
                    last_sync_error: Some("permission denied".into()),
                },
            )
            .await
            .unwrap();

        let found = service
            .find_by_id_and_org(inserted.id, org.id)
            .await
            .unwrap();
        assert_eq!(found.last_synced_at, Some(failed_at));
        assert_eq!(found.last_success_at, Some(succeeded_at));
        assert_eq!(found.last_sync_error.as_deref(), Some("permission denied"));
    }

    #[sqlx::test]
    #[should_panic(expected = "Repository(NotFound)")]
    async fn delete_reports_a_missing_connection(pool: Pool<Postgres>) {
        let org = seed_org(&pool, "acme", "a@example.com").await;
        let service = PgMotherDuckConnectionService::new(pool, cipher());
        service.delete(Uuid::new_v4(), org.id).await.unwrap();
    }
}
