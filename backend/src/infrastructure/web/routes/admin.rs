use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};

use crate::application::use_cases::auth::AuthContext;
use crate::domain::error::Error;
use crate::infrastructure::web::State as AppState;

async fn list_organizations(
    State(state): State<AppState>,
    context: AuthContext,
) -> Result<impl IntoResponse, Error> {
    let overviews = state.admin.list_organizations(context).await?;
    Ok((StatusCode::OK, Json(overviews)))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/organizations", get(list_organizations))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::application::use_cases::admin::MockAdminUseCase;
    use crate::infrastructure::web::get_mock_state_with_admin;

    fn context(is_superadmin: bool) -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            is_superadmin,
        }
    }

    #[tokio::test]
    async fn a_regular_user_gets_403() {
        let mut admin = MockAdminUseCase::new();
        admin
            .expect_list_organizations()
            .return_once(|_| Err(Error::forbidden()));
        let state = get_mock_state_with_admin(admin);

        let response = list_organizations(State(state), context(false))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_superadmin_gets_the_list() {
        let mut admin = MockAdminUseCase::new();
        admin
            .expect_list_organizations()
            .return_once(|_| Ok(vec![]));
        let state = get_mock_state_with_admin(admin);

        let response = list_organizations(State(state), context(true))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
