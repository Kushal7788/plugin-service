use std::net::SocketAddr;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::config::{load_env_files, load_notary_signing_key, load_trusted_notary_keys, port};
use crate::handlers::{
    artifact_verify_handler, health_handler, info_handler, issue_attestation_handler,
    register_session_handler, reveal_config_handler, session_result_handler, verifier_ws_handler,
};
use crate::state::AppState;

pub(crate) async fn run() {
    load_env_files();
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "mobile_tlsn_verifier_service=info,tlsn=warn".to_string()),
        )
        .with_target(true)
        .init();

    let state = AppState::new(load_trusted_notary_keys(), load_notary_signing_key());
    let app = router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    info!("mobile tlsn verifier service listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server failed");
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/info", get(info_handler))
        .route("/v1/sessions/register", post(register_session_handler))
        .route(
            "/v1/sessions/:session_id/verifier",
            get(verifier_ws_handler),
        )
        .route(
            "/v1/sessions/:session_id/reveal-config",
            post(reveal_config_handler),
        )
        .route(
            "/v1/sessions/:session_id/attestation",
            post(issue_attestation_handler),
        )
        .route(
            "/v1/sessions/:session_id/result",
            get(session_result_handler),
        )
        .route("/v1/artifacts/verify", post(artifact_verify_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
