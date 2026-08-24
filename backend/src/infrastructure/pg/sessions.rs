use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::application::services::sessions::SessionService;
use crate::domain::entities::sessions::Session;
use crate::domain::error::Result;

/// Row shape as stored in PostgreSQL, kept separate so the domain entity
/// carries no `sqlx` derive.
#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    user_id: Uuid,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Session {
            id: row.id,
            user_id: row.user_id,
            expires_at: row.expires_at,
            created_at: row.created_at,
        }
    }
}

pub struct PgSessionService {
    db: PgPool,
}

impl PgSessionService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SessionService for PgSessionService {
    async fn insert(&self, session: Session, token_hash: Vec<u8>) -> Result<Session> {
        let row = sqlx::query_as::<_, SessionRow>(
            "insert into sessions (id, user_id, token_hash, expires_at, created_at)
             values ($1, $2, $3, $4, $5)
             returning id, user_id, expires_at, created_at",
        )
        .bind(session.id)
        .bind(session.user_id)
        .bind(&token_hash)
        .bind(session.expires_at)
        .bind(session.created_at)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }

    async fn find_by_token_hash(&self, token_hash: &[u8]) -> Result<Session> {
        let row = sqlx::query_as::<_, SessionRow>(
            "select id, user_id, expires_at, created_at
             from sessions
             where token_hash = $1",
        )
        .bind(token_hash)
        .fetch_one(&self.db)
        .await?;

        Ok(row.into())
    }

    async fn delete_by_token_hash(&self, token_hash: &[u8]) -> Result<()> {
        // Logging out an already deleted session is not an error worth
        // reporting, so the affected row count goes unchecked.
        sqlx::query("delete from sessions where token_hash = $1")
            .bind(token_hash)
            .execute(&self.db)
            .await?;

        Ok(())
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use chrono::Duration;
    use sqlx::{Pool, Postgres};

    use super::*;
    use crate::application::services::users::UserService;
    use crate::domain::entities::sessions::SessionToken;
    use crate::domain::entities::users::{Email, PasswordHash, User};
    use crate::infrastructure::pg::users::PgUserService;

    /// Sessions reference the account, so one has to exist first.
    async fn seed_user(pool: &Pool<Postgres>) -> User {
        let user = User::new(Email::new("owner@example.com").unwrap(), Utc::now());
        PgUserService::new(pool.clone())
            .create(user.clone(), PasswordHash::new("h".into()))
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn insert_then_find_by_token_hash(pool: Pool<Postgres>) {
        let user = seed_user(&pool).await;
        let service = PgSessionService::new(pool);
        let token = SessionToken::generate();
        let session = Session::new(user.id, Duration::hours(1), Utc::now());

        let inserted = service.insert(session.clone(), token.hash()).await.unwrap();

        assert_eq!(inserted.id, session.id);
        let found = service.find_by_token_hash(&token.hash()).await.unwrap();
        assert_eq!(found.id, session.id);
        assert_eq!(found.user_id, user.id);
    }

    #[sqlx::test]
    async fn delete_by_token_hash_removes_the_session(pool: Pool<Postgres>) {
        let user = seed_user(&pool).await;
        let service = PgSessionService::new(pool);
        let token = SessionToken::generate();
        let session = Session::new(user.id, Duration::hours(1), Utc::now());
        service.insert(session, token.hash()).await.unwrap();

        service.delete_by_token_hash(&token.hash()).await.unwrap();

        assert!(service.find_by_token_hash(&token.hash()).await.is_err());
    }

    #[sqlx::test]
    async fn delete_by_token_hash_tolerates_a_missing_session(pool: Pool<Postgres>) {
        let service = PgSessionService::new(pool);
        service
            .delete_by_token_hash(&SessionToken::generate().hash())
            .await
            .unwrap();
    }
}
