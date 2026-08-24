use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::admin::AdminService;
use crate::domain::entities::motherduck_connections::MotherDuckConnection;
use crate::domain::entities::organizations::{Organization, OrganizationOverview};
use crate::domain::entities::pricing::RegionTier;
use crate::domain::error::Result;

/// Row shapes stay private to this layer, as everywhere else in `pg/`.
#[derive(sqlx::FromRow)]
struct OverviewOrgRow {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    user_count: i64,
}

#[derive(sqlx::FromRow)]
struct OverviewConnectionRow {
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

impl From<OverviewConnectionRow> for MotherDuckConnection {
    fn from(row: OverviewConnectionRow) -> Self {
        MotherDuckConnection {
            id: row.id,
            org_id: row.org_id,
            name: row.name,
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

pub struct PgAdminService {
    db: PgPool,
}

impl PgAdminService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AdminService for PgAdminService {
    async fn list_organization_overviews(&self) -> Result<Vec<OrganizationOverview>> {
        let org_rows = sqlx::query_as::<_, OverviewOrgRow>(
            "select o.id, o.name, o.created_at, o.updated_at,
                    count(u.id) as user_count
             from organizations o
             left join users u on u.org_id = o.id
             group by o.id
             order by o.created_at desc",
        )
        .fetch_all(&self.db)
        .await?;

        let connection_rows = sqlx::query_as::<_, OverviewConnectionRow>(
            "select id, org_id, name, region_tier, enabled, watermark_start_time, last_synced_at, last_success_at,
                    last_sync_error, created_at, updated_at
             from motherduck_connections
             order by created_at",
        )
        .fetch_all(&self.db)
        .await?;

        let mut overviews: Vec<OrganizationOverview> = org_rows
            .into_iter()
            .map(|row| OrganizationOverview {
                organization: Organization {
                    id: row.id,
                    name: row.name,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                },
                user_count: row.user_count,
                connections: Vec::new(),
            })
            .collect();

        for connection in connection_rows {
            if let Some(overview) = overviews
                .iter_mut()
                .find(|overview| overview.organization.id == connection.org_id)
            {
                overview.connections.push(connection.into());
            }
        }

        Ok(overviews)
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::motherduck_connections::MotherDuckConnectionService;
    use crate::application::services::organizations::OrganizationService;
    use crate::domain::entities::motherduck_connections::ConnectionDraft;
    use crate::domain::entities::organizations::OrganizationDraft;
    use crate::domain::entities::users::{Email, PasswordHash, User};
    use crate::infrastructure::crypto::SecretCipher;
    use crate::infrastructure::pg::motherduck_connections::PgMotherDuckConnectionService;
    use crate::infrastructure::pg::organizations::PgOrganizationService;
    use crate::infrastructure::pg::organizations::integration_tests::trunc_now;

    async fn seed_org(pool: &Pool<Postgres>, name: &str, email: &str) -> Organization {
        let now = trunc_now();
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

    #[sqlx::test]
    async fn overview_spans_every_organization(pool: Pool<Postgres>) {
        let first = seed_org(&pool, "acme", "a@example.com").await;
        let second = seed_org(&pool, "buzz", "b@example.com").await;

        let cipher = Arc::new(SecretCipher::from_base64_key(&STANDARD.encode([9u8; 32])).unwrap());
        let (connection, token) = ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(second.id, trunc_now());
        let connection = PgMotherDuckConnectionService::new(pool.clone(), cipher)
            .insert(connection, token)
            .await
            .unwrap();

        let overviews = PgAdminService::new(pool)
            .list_organization_overviews()
            .await
            .unwrap();

        assert_eq!(overviews.len(), 2);
        // Newest organization first.
        assert_eq!(overviews[0].organization.id, second.id);
        assert_eq!(overviews[0].user_count, 1);
        assert_eq!(overviews[0].connections, vec![connection]);
        assert_eq!(overviews[1].organization.id, first.id);
        assert_eq!(overviews[1].connections, vec![]);
    }

    #[sqlx::test]
    async fn overview_is_empty_without_organizations(pool: Pool<Postgres>) {
        let overviews = PgAdminService::new(pool)
            .list_organization_overviews()
            .await
            .unwrap();
        assert_eq!(overviews, vec![]);
    }
}
