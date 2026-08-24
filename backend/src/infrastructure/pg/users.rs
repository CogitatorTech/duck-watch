use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::users::UserService;
use crate::domain::entities::users::{Email, PasswordHash, User};
use crate::domain::error::{Error, RepositoryErrorType, Result};

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    password_hash: String,
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
                email,
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
            "select id, email, password_hash, created_at, updated_at
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
            "select id, email, password_hash, created_at, updated_at
             from users
             where id = $1",
        )
        .bind(id)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into_user()?.0)
    }

    async fn any_exists(&self) -> Result<bool> {
        let (exists,): (bool,) = sqlx::query_as("select exists (select 1 from users)")
            .fetch_one(&self.db)
            .await?;
        Ok(exists)
    }

    async fn create(&self, user: User, password_hash: PasswordHash) -> Result<User> {
        // The insert only lands while the table is empty, so two requests
        // racing to claim a fresh instance cannot both succeed. The second
        // one affects no rows and reports a conflict.
        let row = sqlx::query_as::<_, UserRow>(
            "insert into users (id, email, password_hash, created_at, updated_at)
             select $1, $2, $3, $4, $4
             where not exists (select 1 from users)
             returning id, email, password_hash, created_at, updated_at",
        )
        .bind(user.id)
        .bind(user.email.as_str())
        .bind(password_hash.as_str())
        .bind(user.created_at)
        .fetch_optional(&self.db)
        .await?;

        match row {
            Some(row) => Ok(row.into_user()?.0),
            None => Err(Error::Repository(RepositoryErrorType::Conflict)),
        }
    }
}

#[cfg(all(test, feature = "integration-tests"))]
pub mod integration_tests {
    use sqlx::{Pool, Postgres};

    use super::*;

    fn account(email: &str) -> User {
        User::new(Email::new(email).unwrap(), trunc_now())
    }

    /// PostgreSQL stores microseconds, so a comparison against a value that
    /// never went through the database needs the same precision.
    pub fn trunc_now() -> DateTime<Utc> {
        let now = Utc::now();
        now - chrono::Duration::nanoseconds(now.timestamp_subsec_nanos() as i64 % 1000)
    }

    #[sqlx::test]
    async fn create_then_find_round_trips_the_account(pool: Pool<Postgres>) {
        let service = PgUserService::new(pool);
        let user = account("owner@example.com");

        service
            .create(user.clone(), PasswordHash::new("hash".into()))
            .await
            .unwrap();

        let (found, hash) = service.find_by_email(&user.email).await.unwrap();
        assert_eq!(found.id, user.id);
        assert_eq!(found.email, user.email);
        assert_eq!(hash.as_str(), "hash");
        assert_eq!(service.find_by_id(user.id).await.unwrap().id, user.id);
    }

    #[sqlx::test]
    async fn any_exists_reports_whether_the_instance_is_claimed(pool: Pool<Postgres>) {
        let service = PgUserService::new(pool);
        assert!(!service.any_exists().await.unwrap());

        service
            .create(account("owner@example.com"), PasswordHash::new("h".into()))
            .await
            .unwrap();

        assert!(service.any_exists().await.unwrap());
    }

    #[sqlx::test]
    async fn a_second_account_is_refused(pool: Pool<Postgres>) {
        // Otherwise anyone reaching a running instance could claim it as
        // their own after the owner already had.
        let service = PgUserService::new(pool);
        service
            .create(account("first@example.com"), PasswordHash::new("h".into()))
            .await
            .unwrap();

        let err = service
            .create(account("second@example.com"), PasswordHash::new("h".into()))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            Error::Repository(RepositoryErrorType::Conflict)
        ));

        assert!(
            service
                .find_by_email(&Email::new("second@example.com").unwrap())
                .await
                .is_err()
        );
    }

    #[sqlx::test]
    async fn find_by_email_reports_a_missing_account(pool: Pool<Postgres>) {
        let service = PgUserService::new(pool);
        assert!(
            service
                .find_by_email(&Email::new("nobody@example.com").unwrap())
                .await
                .is_err()
        );
    }
}
