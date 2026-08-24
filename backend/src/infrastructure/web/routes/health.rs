use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde_json::json;

use crate::infrastructure::web::State as AppState;

/// Liveness probe. It reports that the process is serving traffic and does not
/// touch the database, so an unhealthy dependency cannot restart a working pod.
async fn get_health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") })),
    )
}

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(get_health))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::web::routes::extract_body_response;

    #[tokio::test]
    async fn get_health_reports_ok() {
        let response = super::get_health().await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            extract_body_response::<serde_json::Value>(response.into_body())
                .await
                .unwrap(),
            json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") })
        );
    }
}
