use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::WebSocket;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;
use tracing::{error, info};

use crate::state::{AppState, NotaryMaterial, SessionRecord, SessionResult, SessionStatus};
use crate::verifier::{run_verifier, VerifierCompletion};
use crate::ws_bridge::bridge_websocket_to_duplex;

pub(crate) async fn run_verifier_session(
    state: AppState,
    session_id: String,
    record: Arc<Mutex<SessionRecord>>,
    prover_socket_rx: oneshot::Receiver<WebSocket>,
) {
    {
        let mut locked = record.lock().await;
        locked.status = SessionStatus::InProgress;
        info!(
            "session={} started clientSessionId={} limits(sent={}, recv={}) metadataKeys={}",
            session_id,
            locked.client_session_id,
            locked.max_sent_data,
            locked.max_recv_data,
            locked.session_data.len()
        );
    }

    let socket = match timeout(Duration::from_secs(60), prover_socket_rx).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(_)) => {
            set_session_failed(&record, "verifier websocket handoff failed").await;
            return;
        }
        Err(_) => {
            set_session_failed(&record, "timed out waiting for verifier websocket").await;
            return;
        }
    };

    let (max_sent_data, max_recv_data) = {
        let locked = record.lock().await;
        (locked.max_sent_data, locked.max_recv_data)
    };
    let (bridge_socket, verifier_socket) = tokio::io::duplex(1 << 20);
    tokio::spawn(bridge_websocket_to_duplex(
        session_id.clone(),
        socket,
        bridge_socket,
    ));

    match run_verifier(verifier_socket, max_sent_data, max_recv_data).await {
        Ok(VerifierCompletion {
            server_name,
            transcript,
            transcript_commitments,
            connection_info,
            server_ephemeral_key,
        }) => {
            let sent = transcript.sent_unsafe().to_vec();
            let recv = transcript.received_unsafe().to_vec();
            let result = SessionResult {
                server_name: server_name.to_string(),
                transcript_sent_len: sent.len(),
                transcript_recv_len: recv.len(),
                transcript_sent_digest_base64: BASE64_STANDARD.encode(Sha256::digest(&sent)),
                transcript_recv_digest_base64: BASE64_STANDARD.encode(Sha256::digest(&recv)),
                verified_at_utc: Utc::now().to_rfc3339(),
            };
            let mut locked = record.lock().await;
            locked.status = SessionStatus::Completed(result);
            locked.notary_material = Some(NotaryMaterial {
                connection_info,
                server_ephemeral_key,
                transcript_commitments,
            });
            info!("session={} verifier completed", session_id);
        }
        Err(error) => {
            error!("session={} verifier failed: {}", session_id, error);
            set_session_failed(&record, &error.to_string()).await;
        }
    }

    cleanup_old_sessions(state).await;
}

pub(crate) async fn set_session_failed(record: &Arc<Mutex<SessionRecord>>, message: &str) {
    let mut locked = record.lock().await;
    locked.status = SessionStatus::Failed {
        message: message.to_string(),
    };
}

pub(crate) async fn cleanup_old_sessions(state: AppState) {
    let now = Utc::now();
    let mut guard = state.sessions.lock().await;
    guard.retain(|_, session| {
        if let Ok(locked) = session.try_lock() {
            let age_secs = (now - locked.created_at_utc).num_seconds();
            age_secs < 600
        } else {
            true
        }
    });
}
