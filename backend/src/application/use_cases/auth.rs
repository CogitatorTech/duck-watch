use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::application::services::organizations::OrganizationService;
use crate::application::services::password::PasswordHasher;
use crate::application::services::sessions::SessionService;
use crate::application::services::users::UserService;
use crate::domain::entities::organizations::{Organization, OrganizationDraft};
use crate::domain::entities::sessions::{Session, SessionToken};
use crate::domain::entities::users::{Email, User};
use crate::domain::error::{Error, RepositoryErrorType, Result};

/// The authenticated caller, resolved from a bearer token. Handlers scope
/// every query by `org_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub is_superadmin: bool,
}

/// A successful signup or login: the user plus the one-time visible token.
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: User,
    pub token: String,
}

/// The authenticated caller's own account, as served by `/me`.
#[derive(Debug, Serialize)]
pub struct Account {
    pub user: User,
    pub organization: Organization,
}

#[async_trait]
pub trait AuthUseCaseTrait: Send + Sync {
    async fn signup(&self, org_name: &str, email: &str, password: &str) -> Result<AuthResponse>;
    async fn login(&self, email: &str, password: &str) -> Result<AuthResponse>;
    async fn logout(&self, token: &str) -> Result<()>;
    async fn authenticate(&self, token: &str) -> Result<AuthContext>;
    async fn get_account(&self, context: AuthContext) -> Result<Account>;
}

pub struct AuthUseCase {
    organization_service: Box<dyn OrganizationService>,
    user_service: Box<dyn UserService>,
    session_service: Box<dyn SessionService>,
    password_hasher: Box<dyn PasswordHasher>,
    session_ttl: Duration,
}

impl AuthUseCase {
    pub fn new(
        organization_service: Box<dyn OrganizationService>,
        user_service: Box<dyn UserService>,
        session_service: Box<dyn SessionService>,
        password_hasher: Box<dyn PasswordHasher>,
        session_ttl: Duration,
    ) -> Self {
        Self {
            organization_service,
            user_service,
            session_service,
            password_hasher,
            session_ttl,
        }
    }

    async fn mint_session(&self, user: User) -> Result<AuthResponse> {
        let token = SessionToken::generate();
        let session = Session::new(user.id, self.session_ttl, Utc::now());
        self.session_service.insert(session, token.hash()).await?;
        Ok(AuthResponse {
            user,
            token: token.reveal().to_string(),
        })
    }
}

/// Turns a credential that resolves to nothing into a 401, and leaves every
/// other failure alone. A 401 makes the browser drop the session and return
/// to the login page, so a database that is merely unreachable must not
/// produce one; otherwise a blip signs every user out.
fn unknown_credential(err: Error) -> Error {
    match err {
        Error::Repository(RepositoryErrorType::NotFound) => Error::unauthorized(),
        other => other,
    }
}

#[async_trait]
impl AuthUseCaseTrait for AuthUseCase {
    async fn signup(&self, org_name: &str, email: &str, password: &str) -> Result<AuthResponse> {
        let now = Utc::now();
        let organization = OrganizationDraft::new(org_name)?.into_new_organization(now);
        let user = User::new(organization.id, Email::new(email)?, now);
        let password_hash = self.password_hasher.hash(password)?;

        let (_, user) = self
            .organization_service
            .create_with_owner(organization, user, password_hash)
            .await?;

        self.mint_session(user).await
    }

    async fn login(&self, email: &str, password: &str) -> Result<AuthResponse> {
        let email = Email::new(email)?;
        // An unknown email and a wrong password both come back as 401, so the
        // endpoint does not reveal which addresses have accounts.
        let (user, password_hash) = self
            .user_service
            .find_by_email(&email)
            .await
            .map_err(|_| Error::unauthorized())?;

        if !self.password_hasher.verify(password, &password_hash)? {
            return Err(Error::unauthorized());
        }

        self.mint_session(user).await
    }

    async fn logout(&self, token: &str) -> Result<()> {
        let token = SessionToken::from_raw(token);
        self.session_service
            .delete_by_token_hash(&token.hash())
            .await
    }

    async fn authenticate(&self, token: &str) -> Result<AuthContext> {
        let token = SessionToken::from_raw(token);
        let session = self
            .session_service
            .find_by_token_hash(&token.hash())
            .await
            .map_err(unknown_credential)?;

        if session.is_expired(Utc::now()) {
            return Err(Error::unauthorized());
        }

        // A session whose user has gone is a credential that identifies
        // nobody, so it is rejected rather than reported as a missing entity.
        let user = self
            .user_service
            .find_by_id(session.user_id)
            .await
            .map_err(unknown_credential)?;
        Ok(AuthContext {
            user_id: user.id,
            org_id: user.org_id,
            is_superadmin: user.is_superadmin,
        })
    }

    async fn get_account(&self, context: AuthContext) -> Result<Account> {
        let user = self.user_service.find_by_id(context.user_id).await?;
        let organization = self.organization_service.find_by_id(user.org_id).await?;
        Ok(Account { user, organization })
    }
}

#[cfg(test)]
mockall::mock! {
    pub AuthUseCase {}
    #[async_trait]
    impl AuthUseCaseTrait for AuthUseCase {
        async fn signup(&self, org_name: &str, email: &str, password: &str) -> Result<AuthResponse>;
        async fn login(&self, email: &str, password: &str) -> Result<AuthResponse>;
        async fn logout(&self, token: &str) -> Result<()>;
        async fn authenticate(&self, token: &str) -> Result<AuthContext>;
        async fn get_account(&self, context: AuthContext) -> Result<Account>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::services::organizations::MockOrganizationService;
    use crate::application::services::password::MockPasswordHasher;
    use crate::application::services::sessions::MockSessionService;
    use crate::application::services::users::MockUserService;
    use crate::domain::entities::users::PasswordHash;
    use crate::domain::error::RepositoryErrorType;

    struct Mocks {
        organizations: MockOrganizationService,
        users: MockUserService,
        sessions: MockSessionService,
        hasher: MockPasswordHasher,
    }

    impl Mocks {
        fn new() -> Self {
            Self {
                organizations: MockOrganizationService::new(),
                users: MockUserService::new(),
                sessions: MockSessionService::new(),
                hasher: MockPasswordHasher::new(),
            }
        }

        fn into_use_case(self) -> AuthUseCase {
            AuthUseCase::new(
                Box::new(self.organizations),
                Box::new(self.users),
                Box::new(self.sessions),
                Box::new(self.hasher),
                Duration::hours(1),
            )
        }
    }

    fn sample_user() -> User {
        User::new(
            Uuid::new_v4(),
            Email::new("owner@example.com").unwrap(),
            Utc::now(),
        )
    }

    #[tokio::test]
    async fn signup_creates_the_account_and_returns_a_token() {
        let mut mocks = Mocks::new();
        mocks
            .hasher
            .expect_hash()
            .return_once(|_| Ok(PasswordHash::new("hash".into())));
        mocks
            .organizations
            .expect_create_with_owner()
            .withf(|org, user, _| org.name == "acme" && user.org_id == org.id)
            .return_once(|org, user, _| Ok((org, user)));
        mocks
            .sessions
            .expect_insert()
            .return_once(|session, _| Ok(session));

        let response = mocks
            .into_use_case()
            .signup("acme", "Owner@Example.com", "password1")
            .await
            .unwrap();

        assert_eq!(response.user.email.as_str(), "owner@example.com");
        assert!(!response.token.is_empty());
    }

    #[tokio::test]
    async fn signup_propagates_a_duplicate_email_as_conflict() {
        let mut mocks = Mocks::new();
        mocks
            .hasher
            .expect_hash()
            .return_once(|_| Ok(PasswordHash::new("hash".into())));
        mocks
            .organizations
            .expect_create_with_owner()
            .return_once(|_, _, _| Err(Error::Repository(RepositoryErrorType::Conflict)));

        let err = mocks
            .into_use_case()
            .signup("acme", "owner@example.com", "password1")
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            Error::Repository(RepositoryErrorType::Conflict)
        ));
    }

    #[tokio::test]
    async fn login_returns_a_token_for_a_valid_password() {
        let user = sample_user();
        let mut mocks = Mocks::new();
        mocks
            .users
            .expect_find_by_email()
            .return_once(move |_| Ok((user, PasswordHash::new("hash".into()))));
        mocks.hasher.expect_verify().return_once(|_, _| Ok(true));
        mocks
            .sessions
            .expect_insert()
            .return_once(|session, _| Ok(session));

        let response = mocks
            .into_use_case()
            .login("owner@example.com", "password1")
            .await
            .unwrap();

        assert!(!response.token.is_empty());
    }

    #[tokio::test]
    async fn login_rejects_a_wrong_password() {
        let user = sample_user();
        let mut mocks = Mocks::new();
        mocks
            .users
            .expect_find_by_email()
            .return_once(move |_| Ok((user, PasswordHash::new("hash".into()))));
        mocks.hasher.expect_verify().return_once(|_, _| Ok(false));

        let err = mocks
            .into_use_case()
            .login("owner@example.com", "wrong")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Unauthorized));
    }

    #[tokio::test]
    async fn login_hides_an_unknown_email_behind_unauthorized() {
        let mut mocks = Mocks::new();
        mocks
            .users
            .expect_find_by_email()
            .return_once(|_| Err(Error::not_found()));

        let err = mocks
            .into_use_case()
            .login("missing@example.com", "password1")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Unauthorized));
    }

    #[tokio::test]
    async fn authenticate_resolves_the_org_scope() {
        let user = sample_user();
        let user_id = user.id;
        let org_id = user.org_id;
        let session = Session::new(user_id, Duration::hours(1), Utc::now());

        let mut mocks = Mocks::new();
        mocks
            .sessions
            .expect_find_by_token_hash()
            .return_once(move |_| Ok(session));
        mocks
            .users
            .expect_find_by_id()
            .return_once(move |_| Ok(user));

        let context = mocks.into_use_case().authenticate("token").await.unwrap();

        assert_eq!(
            context,
            AuthContext {
                user_id,
                org_id,
                is_superadmin: false,
            }
        );
    }

    #[tokio::test]
    async fn authenticate_rejects_an_expired_session() {
        let user = sample_user();
        let expired = Session::new(user.id, Duration::hours(-1), Utc::now());

        let mut mocks = Mocks::new();
        mocks
            .sessions
            .expect_find_by_token_hash()
            .return_once(move |_| Ok(expired));

        let err = mocks
            .into_use_case()
            .authenticate("token")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Unauthorized));
    }

    #[tokio::test]
    async fn authenticate_rejects_an_unknown_token() {
        let mut mocks = Mocks::new();
        mocks
            .sessions
            .expect_find_by_token_hash()
            .return_once(|_| Err(Error::not_found()));

        let err = mocks
            .into_use_case()
            .authenticate("token")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Unauthorized));
    }

    #[tokio::test]
    async fn authenticate_rejects_a_session_whose_user_is_gone() {
        // The cascade on `sessions.user_id` normally removes the session with
        // the user, so this is the race between the two reads. It is still a
        // credential that no longer identifies anyone, which is a 401 rather
        // than a missing page.
        let session = Session::new(Uuid::new_v4(), Duration::hours(1), Utc::now());

        let mut mocks = Mocks::new();
        mocks
            .sessions
            .expect_find_by_token_hash()
            .return_once(move |_| Ok(session));
        mocks
            .users
            .expect_find_by_id()
            .return_once(|_| Err(Error::not_found()));

        let err = mocks
            .into_use_case()
            .authenticate("token")
            .await
            .unwrap_err();

        assert!(matches!(err, Error::Unauthorized), "got {err:?}");
    }

    #[tokio::test]
    async fn authenticate_does_not_turn_a_database_failure_into_a_logout() {
        // A 401 makes the browser drop the session and return to the login
        // page, so a database that is merely unreachable must not report one;
        // otherwise a blip signs everybody out.
        for failing in [Failing::Sessions, Failing::Users] {
            let session = Session::new(Uuid::new_v4(), Duration::hours(1), Utc::now());
            let mut mocks = Mocks::new();
            match failing {
                Failing::Sessions => {
                    mocks.sessions.expect_find_by_token_hash().return_once(|_| {
                        Err(Error::External(anyhow::anyhow!("connection refused")))
                    });
                }
                Failing::Users => {
                    mocks
                        .sessions
                        .expect_find_by_token_hash()
                        .return_once(move |_| Ok(session));
                    mocks.users.expect_find_by_id().return_once(|_| {
                        Err(Error::External(anyhow::anyhow!("connection refused")))
                    });
                }
            }

            let err = mocks
                .into_use_case()
                .authenticate("token")
                .await
                .unwrap_err();

            assert!(
                !matches!(err, Error::Unauthorized),
                "{failing:?} failure reported as unauthorized"
            );
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum Failing {
        Sessions,
        Users,
    }
}
