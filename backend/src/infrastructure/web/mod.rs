use axum::Router;
use std::{future::Future, sync::Arc};
use tower_http::trace;

use crate::{
    application::use_cases::admin::AdminUseCaseTrait,
    application::use_cases::auth::AuthUseCaseTrait,
    application::use_cases::connections::ConnectionsUseCaseTrait,
    application::use_cases::dashboard::DashboardUseCaseTrait, config::Config,
};

mod error;
mod middleware;
mod routes;

#[derive(Clone)]
pub struct State {
    auth: Arc<dyn AuthUseCaseTrait>,
    connections: Arc<dyn ConnectionsUseCaseTrait>,
    dashboard: Arc<dyn DashboardUseCaseTrait>,
    admin: Arc<dyn AdminUseCaseTrait>,
}

impl State {
    pub fn new(
        auth: Arc<dyn AuthUseCaseTrait>,
        connections: Arc<dyn ConnectionsUseCaseTrait>,
        dashboard: Arc<dyn DashboardUseCaseTrait>,
        admin: Arc<dyn AdminUseCaseTrait>,
    ) -> Self {
        State {
            auth,
            connections,
            dashboard,
            admin,
        }
    }
}

pub async fn run(
    config: Config,
    state: State,
    shutdown_signal: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = Router::new()
        .merge(routes::health::router())
        .nest("/api/v1", routes::auth::router())
        .nest("/api/v1/connections", routes::connections::router())
        .nest("/api/v1/dashboard", routes::dashboard::router())
        .nest("/api/v1/admin", routes::admin::router())
        .with_state(state)
        .layer(config.get_cors_layer())
        .layer(
            trace::TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(tracing::Level::INFO))
                .on_request(trace::DefaultOnRequest::new().level(tracing::Level::INFO))
                .on_failure(trace::DefaultOnFailure::new().level(tracing::Level::ERROR)),
        );

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{0}", config.port)).await?;

    if let Ok(addr) = listener.local_addr() {
        tracing::info!("Listening on http://{addr}");
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    Ok(())
}

#[cfg(test)]
fn empty_mock_state() -> State {
    State {
        auth: Arc::new(crate::application::use_cases::auth::MockAuthUseCase::new()),
        connections: Arc::new(
            crate::application::use_cases::connections::MockConnectionsUseCase::new(),
        ),
        dashboard: Arc::new(crate::application::use_cases::dashboard::MockDashboardUseCase::new()),
        admin: Arc::new(crate::application::use_cases::admin::MockAdminUseCase::new()),
    }
}

#[cfg(test)]
pub fn get_mock_state_with_auth(
    auth: crate::application::use_cases::auth::MockAuthUseCase,
) -> State {
    State {
        auth: Arc::new(auth),
        ..empty_mock_state()
    }
}

#[cfg(test)]
pub fn get_mock_state_with_connections(
    connections: crate::application::use_cases::connections::MockConnectionsUseCase,
) -> State {
    State {
        connections: Arc::new(connections),
        ..empty_mock_state()
    }
}

#[cfg(test)]
pub fn get_mock_state_with_admin(
    admin: crate::application::use_cases::admin::MockAdminUseCase,
) -> State {
    State {
        admin: Arc::new(admin),
        ..empty_mock_state()
    }
}

#[cfg(test)]
pub fn get_mock_state_with_dashboard(
    dashboard: crate::application::use_cases::dashboard::MockDashboardUseCase,
) -> State {
    State {
        dashboard: Arc::new(dashboard),
        ..empty_mock_state()
    }
}
