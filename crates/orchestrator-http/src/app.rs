//! Axum application builder.

use axum::Router;
use std::net::SocketAddr;

use crate::routes;
use crate::state::AppState;

/// Build the application router (state is injected when calling serve).
pub fn app() -> Router<AppState> {
    routes::router()
}

/// Run the server on the given address. Injects state into the router for request handling.
pub async fn serve(
    router: Router<AppState>,
    state: AppState,
    addr: SocketAddr,
) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("orchestrator API listening on {}", addr);
    axum::serve(listener, router.with_state(state)).await
}
