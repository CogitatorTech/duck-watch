use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::domain::error::{Error, RepositoryErrorType};

impl From<JsonRejection> for Error {
    fn from(e: JsonRejection) -> Self {
        Error::Validation(e.into())
    }
}

impl From<PathRejection> for Error {
    fn from(e: PathRejection) -> Self {
        Error::Validation(e.into())
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        self.log();

        let status_code = match self {
            Error::Repository(RepositoryErrorType::NotFound) => StatusCode::NOT_FOUND,
            Error::Repository(RepositoryErrorType::Conflict) => StatusCode::CONFLICT,
            Error::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::External(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        // Internal failures must not leak their cause to the caller.
        let message = match self {
            Error::External(_) => "internal server error".to_string(),
            _ => self.to_string(),
        };

        (status_code, Json(json!({ "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_maps_to_404() {
        let response = Error::not_found().into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn conflict_maps_to_409() {
        let response = Error::Repository(RepositoryErrorType::Conflict).into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn validation_maps_to_422() {
        let response = Error::validation("bad input").into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn unauthorized_maps_to_401() {
        let response = Error::unauthorized().into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn external_maps_to_500() {
        let response = Error::External(anyhow::anyhow!("boom")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn external_does_not_leak_its_cause() {
        let response = Error::External(anyhow::anyhow!("secret connection string")).into_response();
        let body = crate::infrastructure::web::routes::extract_body_response::<serde_json::Value>(
            response.into_body(),
        )
        .await
        .unwrap();

        assert_eq!(body, json!({ "error": "internal server error" }));
    }
}
