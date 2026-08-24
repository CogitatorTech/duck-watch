use axum::{
    extract::{FromRequest, FromRequestParts, Json, rejection::JsonRejection},
    http::{Request, header, request::Parts},
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::application::use_cases::auth::AuthContext;
use crate::domain::error::Error;
use crate::infrastructure::web::State as AppState;

/// Reads the bearer token from the `Authorization` header. Handlers that need
/// the raw token (logout) use this; everything else goes through `AuthContext`.
pub fn bearer_token(parts: &Parts) -> Result<String, Error> {
    parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or(Error::Unauthorized)
}

/// Extractor that resolves the bearer token into the authenticated caller, so
/// protected handlers just take `context: AuthContext` as an argument.
impl FromRequestParts<AppState> for AuthContext {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        state.auth.authenticate(&token).await
    }
}

/// A `Json` extractor that runs `validator` on the decoded body, so handlers
/// only ever see a payload that passed its field rules.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = Error;

    async fn from_request(
        req: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await?;
        value
            .validate()
            .map_err(|err| Error::validation(err.to_string()))?;
        Ok(ValidatedJson(value))
    }
}
