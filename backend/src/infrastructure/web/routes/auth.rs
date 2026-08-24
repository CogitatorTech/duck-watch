use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, request::Parts},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use validator::Validate;

use crate::application::use_cases::auth::AuthContext;
use crate::domain::entities::users::{EMAIL_MAX_LEN, PASSWORD_MAX_LEN, PASSWORD_MIN_LEN};
use crate::domain::error::Error;
use crate::infrastructure::web::State as AppState;
use crate::infrastructure::web::middleware::{ValidatedJson, bearer_token};

#[derive(Debug, Deserialize, Validate)]
struct SetupBody {
    #[validate(length(min = 3, max = EMAIL_MAX_LEN))]
    email: String,
    #[validate(length(min = PASSWORD_MIN_LEN, max = PASSWORD_MAX_LEN))]
    password: String,
}

#[derive(Debug, Deserialize, Validate)]
struct LoginBody {
    #[validate(length(min = 3, max = EMAIL_MAX_LEN))]
    email: String,
    #[validate(length(min = 1, max = PASSWORD_MAX_LEN))]
    password: String,
}

/// Whether the instance still needs its account. The setup page asks this
/// before offering the form, and it is the one route that answers before
/// anyone has signed in.
async fn get_setup(State(state): State<AppState>) -> Result<impl IntoResponse, Error> {
    let needed = state.auth.needs_setup().await?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "needed": needed })),
    ))
}

/// Creates the one account. It reports a conflict once the instance has been
/// claimed, so it cannot be used to add a second account later.
async fn post_setup(
    State(state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<SetupBody>,
) -> Result<impl IntoResponse, Error> {
    let response = state
        .auth
        .create_account(&payload.email, &payload.password)
        .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn post_login(
    State(state): State<AppState>,
    ValidatedJson(payload): ValidatedJson<LoginBody>,
) -> Result<impl IntoResponse, Error> {
    let response = state.auth.login(&payload.email, &payload.password).await?;
    Ok((StatusCode::OK, Json(response)))
}

async fn post_logout(
    State(state): State<AppState>,
    parts: Parts,
) -> Result<impl IntoResponse, Error> {
    let token = bearer_token(&parts)?;
    state.auth.logout(&token).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_me(
    State(state): State<AppState>,
    context: AuthContext,
) -> Result<impl IntoResponse, Error> {
    let account = state.auth.get_account(context).await?;
    Ok((StatusCode::OK, Json(account)))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/setup", get(get_setup).post(post_setup))
        .route("/auth/login", post(post_login))
        .route("/auth/logout", post(post_logout))
        .route("/me", get(get_me))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::application::use_cases::auth::{AuthResponse, MockAuthUseCase};
    use crate::domain::entities::users::{Email, User};
    use crate::infrastructure::web::routes::extract_body_response;

    fn sample_response() -> AuthResponse {
        AuthResponse {
            user: User::new(Email::new("owner@example.com").unwrap(), Utc::now()),
            token: "token".to_string(),
        }
    }

    #[tokio::test]
    async fn post_setup_returns_201_with_a_token() {
        let mut auth = MockAuthUseCase::new();
        auth.expect_create_account()
            .return_once(|_, _| Ok(sample_response()));
        let state = crate::infrastructure::web::get_mock_state_with_auth(auth);

        let payload = SetupBody {
            email: "owner@example.com".into(),
            password: "password1".into(),
        };
        let response = post_setup(State(state), ValidatedJson(payload))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = extract_body_response::<serde_json::Value>(response.into_body())
            .await
            .unwrap();
        assert_eq!(body["token"], "token");
    }

    #[tokio::test]
    async fn post_login_maps_bad_credentials_to_401() {
        let mut auth = MockAuthUseCase::new();
        auth.expect_login()
            .return_once(|_, _| Err(Error::unauthorized()));
        let state = crate::infrastructure::web::get_mock_state_with_auth(auth);

        let payload = LoginBody {
            email: "owner@example.com".into(),
            password: "wrong-password".into(),
        };
        let response = post_login(State(state), ValidatedJson(payload))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
