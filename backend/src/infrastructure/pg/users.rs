use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::users::UserService;
use crate::domain::entities::users::{Email, PasswordHash, User};
use crate::domain::error::{Error, Result};

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    org_id: Uuid,
    email: String,
    password_hash: String,
    is_superadmin: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl UserRow {
    fn into_user(self) -> Result<(User, PasswordHash)> {
        // A stored email that fails the domain invariant means corrupt data,
        // not caller input, so it surfaces as an internal error.
        let email = Email::new(&self.email)
            .map_err(|err| Error::External(anyhow::anyhow!("stored email is invalid: {err}")))?;
        Ok((
            User {
                id: self.id,
                org_id: self.org_id,
                email,
                is_superadmin: self.is_superadmin,
                created_at: self.created_at,
                updated_at: self.updated_at,
            },
            PasswordHash::new(self.password_hash),
        ))
    }
}

pub struct PgUserService {
    db: PgPool,
}

impl PgUserService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UserService for PgUserService {
    async fn find_by_email(&self, email: &Email) -> Result<(User, PasswordHash)> {
        let row = sqlx::query_as::<_, UserRow>(
            "select id, org_id, email, password_hash, is_superadmin, created_at, updated_at
             from users
             where email = $1",
        )
        .bind(email.as_str())
        .fetch_one(&self.db)
        .await?;

        row.into_user()
    }

    async fn find_by_id(&self, id: Uuid) -> Result<User> {
        let row = sqlx::query_as::<_, UserRow>(
            "select id, org_id, email, password_hash, is_superadmin, created_at, updated_at
             from users
             where id = $1",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into_user()?.0)
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::organizations::OrganizationService;
    use crate::domain::entities::organizations::OrganizationDraft;
    use crate::infrastructure::pg::organizations::PgOrganizationService;

    async fn seed_user(pool: &Pool<Postgres>, email: &str) -> User {
        let now = crate::infrastructure::pg::organizations::integration_tests::trunc_now();
        let organization = OrganizationDraft::new("acme")
            .unwrap()
            .into_new_organization(now);
        let user = User::new(organization.id, Email::new(email).unwrap(), now);
        PgOrganizationService::new(pool.clone())
            .create_with_owner(organization, user.clone(), PasswordHash::new("h".into()))
            .await
            .unwrap();
        user
    }

    #[sqlx::test]
    async fn find_by_email_returns_the_user_and_hash(pool: Pool<Postgres>) {
        let user = seed_user(&pool, "owner@example.com").await;
        let service = PgUserService::new(pool);

        let (found, hash) = service.find_by_email(&user.email).await.unwrap();
        assert_eq!(found, user);
        assert_eq!(hash, PasswordHash::new("h".into()));
    }

    #[sqlx::test]
    async fn find_by_id_returns_the_user(pool: Pool<Postgres>) {
        let user = seed_user(&pool, "owner@example.com").await;
        let service = PgUserService::new(pool);

        assert_eq!(service.find_by_id(user.id).await.unwrap(), user);
    }

    #[sqlx::test]
    #[should_panic(expected = "Repository(NotFound)")]
    async fn find_by_email_reports_a_missing_user(pool: Pool<Postgres>) {
        let service = PgUserService::new(pool);
        service
            .find_by_email(&Email::new("missing@example.com").unwrap())
            .await
            .unwrap();
    }
}
