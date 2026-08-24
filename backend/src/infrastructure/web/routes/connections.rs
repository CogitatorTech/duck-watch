use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use crate::application::use_cases::auth::AuthContext;
use crate::domain::entities::motherduck_connections::{
    CONNECTION_NAME_MAX_LEN, ConnectionDraft, TOKEN_MAX_LEN,
};
use crate::domain::entities::pricing::RegionTier;
use crate::domain::error::Error;
use crate::infrastructure::web::State as AppState;
use crate::infrastructure::web::middleware::ValidatedJson;

#[derive(Debug, Deserialize, Validate)]
struct ConnectionBody {
    #[validate(length(min = 1, max = CONNECTION_NAME_MAX_LEN))]
    name: String,
    #[validate(length(min = 1, max = TOKEN_MAX_LEN))]
    token: String,
    /// MotherDuck price tier for the account's region; defaults to tier 1.
    region: Option<String>,
}

async fn list_connections(
    State(state): State<AppState>,
    context: AuthContext,
) -> Result<impl IntoResponse, Error> {
    let connections = state.connections.list_connections(context).await?;
    Ok((StatusCode::OK, Json(connections)))
}

async fn post_connection(
    State(state): State<AppState>,
    context: AuthContext,
    ValidatedJson(payload): ValidatedJson<ConnectionBody>,
) -> Result<impl IntoResponse, Error> {
    let region_tier = match payload.region.as_deref() {
        Some(raw) => RegionTier::parse(raw)?,
        None => RegionTier::default(),
    };
    let draft = ConnectionDraft::new(&payload.name, &payload.token, region_tier)?;
    let connection = state.connections.create_connection(context, draft).await?;
    Ok((StatusCode::CREATED, Json(connection)))
}

async fn delete_connection(
    State(state): State<AppState>,
    context: AuthContext,
    Path(connection_id): Path<Uuid>,
) -> Result<impl IntoResponse, Error> {
    state
        .connections
        .delete_connection(context, connection_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_connections).post(post_connection))
        .route("/{connection_id}", axum::routing::delete(delete_connection))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::application::use_cases::connections::MockConnectionsUseCase;
    use crate::domain::entities::motherduck_connections::MotherDuckConnection;
    use crate::infrastructure::web::get_mock_state_with_connections;
    use crate::infrastructure::web::routes::extract_body_response;

    fn context() -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
        }
    }

    fn sample_connection() -> MotherDuckConnection {
        ConnectionDraft::new("prod", "tok", RegionTier::Tier1)
            .unwrap()
            .into_new_connection(Utc::now())
            .0
    }

    #[tokio::test]
    async fn post_connection_returns_201_without_the_token() {
        let context = context();
        let connection = sample_connection();

        let mut connections = MockConnectionsUseCase::new();
        connections
            .expect_create_connection()
            .return_once(move |_, _| Ok(connection));
        let state = get_mock_state_with_connections(connections);

        let payload = ConnectionBody {
            name: "prod".into(),
            token: "tok".into(),
            region: None,
        };
        let response = post_connection(State(state), context, ValidatedJson(payload))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = extract_body_response::<serde_json::Value>(response.into_body())
            .await
            .unwrap();
        assert_eq!(body["name"], "prod");
        // The serialized connection must never carry the token.
        assert!(body.get("token").is_none());
        assert!(!body.to_string().contains("tok\""));
    }

    #[tokio::test]
    async fn post_connection_maps_a_rejected_token_to_422() {
        let mut connections = MockConnectionsUseCase::new();
        connections
            .expect_create_connection()
            .return_once(|_, _| Err(Error::validation("no history access")));
        let state = get_mock_state_with_connections(connections);

        let payload = ConnectionBody {
            name: "prod".into(),
            token: "bad".into(),
            region: None,
        };
        let response = post_connection(State(state), context(), ValidatedJson(payload))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn delete_connection_returns_204() {
        let mut connections = MockConnectionsUseCase::new();
        connections
            .expect_delete_connection()
            .return_once(|_, _| Ok(()));
        let state = get_mock_state_with_connections(connections);

        let response = delete_connection(State(state), context(), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
