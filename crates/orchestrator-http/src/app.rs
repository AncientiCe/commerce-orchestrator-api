//! Axum application builder.

use axum::middleware;
use axum::Router;
use std::net::SocketAddr;

use crate::routes;
use crate::state::AppState;

/// Build the application router (state is injected when calling serve).
pub fn app() -> Router<AppState> {
    routes::router()
}

/// Wrap a router in the UCP signing layers.
///
/// Both layers are no-ops unless the state carries the corresponding keys, so
/// this is applied unconditionally and the deployment's configuration decides
/// whether signatures are produced or required.
pub fn signed_router(router: Router<AppState>, state: AppState) -> Router<AppState> {
    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::signing::verify_request_signature,
        ))
        .layer(middleware::from_fn_with_state(
            state,
            crate::signing::sign_response,
        ))
}

/// Run the server on the given address. Injects state into the router for request handling.
pub async fn serve(
    router: Router<AppState>,
    state: AppState,
    addr: SocketAddr,
) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("orchestrator API listening on {}", addr);
    let router = signed_router(router, state.clone());
    axum::serve(listener, router.with_state(state)).await
}
