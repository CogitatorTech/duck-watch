use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::organizations::OrganizationService;
use crate::domain::entities::organizations::Organization;
use crate::domain::entities::users::{PasswordHash, User};
use crate::domain::error::Result;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct OrganizationRow {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OrganizationRow> for Organization {
    fn from(row: OrganizationRow) -> Self {
        Organization {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub struct PgOrganizationService {
    db: PgPool,
}

impl PgOrganizationService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrganizationService for PgOrganizationService {
    async fn create_with_owner(
        &self,
        organization: Organization,
        user: User,
        password_hash: PasswordHash,
    ) -> Result<(Organization, User)> {
        // One transaction, so a duplicate email cannot leave an organization
        // without any user.
        let mut tx = self.db.begin().await?;

        sqlx::query(
            "insert into organizations (id, name, created_at, updated_at)
             values ($1, $2, $3, $4)",
        )
        .bind(organization.id)
        .bind(&organization.name)
        .bind(organization.created_at)
        .bind(organization.updated_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "insert into users (id, org_id, email, password_hash, created_at, updated_at)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(user.id)
        .bind(user.org_id)
        .bind(user.email.as_str())
        .bind(password_hash.as_str())
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok((organization, user))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Organization> {
        let row = sqlx::query_as::<_, OrganizationRow>(
            "select id, name, created_at, updated_at
             from organizations
             where id = $1",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
pub mod integration_tests {
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::domain::entities::organizations::OrganizationDraft;
    use crate::domain::entities::users::Email;

    pub fn trunc_now() -> chrono::DateTime<Utc> {
        use chrono::DurationRound;
        Utc::now()
            .duration_trunc(chrono::Duration::microseconds(1))
            .unwrap()
    }

    pub fn sample_account(email: &str) -> (Organization, User, PasswordHash) {
        // PostgreSQL stores microseconds, so the seed matches what comes back.
        let now = trunc_now();
        let organization = OrganizationDraft::new("acme")
            .unwrap()
            .into_new_organization(now);
        let user = User::new(organization.id, Email::new(email).unwrap(), now);
        (organization, user, PasswordHash::new("hash".to_string()))
    }

    #[sqlx::test]
    async fn create_with_owner_persists_both_rows(pool: Pool<Postgres>) {
        let service = PgOrganizationService::new(pool);
        let (organization, user, hash) = sample_account("owner@example.com");

        let (created_org, created_user) = service
            .create_with_owner(organization.clone(), user.clone(), hash)
            .await
            .unwrap();

        assert_eq!(created_org, organization);
        assert_eq!(created_user, user);
        assert_eq!(
            service.find_by_id(organization.id).await.unwrap(),
            organization
        );
    }

    #[sqlx::test]
    #[should_panic(expected = "Repository(Conflict)")]
    async fn create_with_owner_rejects_a_duplicate_email(pool: Pool<Postgres>) {
        let service = PgOrganizationService::new(pool);
        let (organization, user, hash) = sample_account("owner@example.com");
        service
            .create_with_owner(organization, user, hash.clone())
            .await
            .unwrap();

        let (second_org, second_user, _) = sample_account("owner@example.com");
        service
            .create_with_owner(second_org, second_user, hash)
            .await
            .unwrap();
    }

    #[sqlx::test]
    async fn a_duplicate_email_leaves_no_orphan_organization(pool: Pool<Postgres>) {
        let service = PgOrganizationService::new(pool.clone());
        let (organization, user, hash) = sample_account("owner@example.com");
        service
            .create_with_owner(organization, user, hash.clone())
            .await
            .unwrap();

        let (second_org, second_user, _) = sample_account("owner@example.com");
        let second_org_id = second_org.id;
        assert!(
            service
                .create_with_owner(second_org, second_user, hash)
                .await
                .is_err()
        );

        assert!(service.find_by_id(second_org_id).await.is_err());
    }

    #[sqlx::test]
    #[should_panic(expected = "Repository(NotFound)")]
    async fn find_by_id_reports_a_missing_organization(pool: Pool<Postgres>) {
        let service = PgOrganizationService::new(pool);
        service.find_by_id(Uuid::new_v4()).await.unwrap();
    }
}
