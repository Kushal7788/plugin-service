use std::time::Duration;

use axum::extract::ws::WebSocket;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{error, info};

use crate::config::{
    session_retention_seconds, session_ttl_seconds, verifier_run_timeout_seconds,
    websocket_handoff_timeout_seconds,
};
use crate::state::{AppState, NotaryMaterial, SessionRecord, SessionResult, SessionStatus};
use crate::verifier::{run_verifier, VerifierCompletion};
use crate::ws_bridge::bridge_websocket_to_duplex;

pub(crate) async fn run_verifier_session(
    state: AppState,
    session_id: String,
    prover_socket_rx: oneshot::Receiver<WebSocket>,
) {
    let record = match mark_session_in_progress(&state, &session_id) {
        Ok(Some(record)) => {
            info!(
                "session={} started clientSessionId={} limits(sent={}, recv={}) metadataKeys={}",
                session_id,
                record.client_session_id,
                record.max_sent_data,
                record.max_recv_data,
                record.session_data.len()
            );
            record
        }
        Ok(None) => {
            error!("session={} missing before verifier start", session_id);
            return;
        }
        Err(error) => {
            error!(
                "session={} failed to persist in_progress: {}",
                session_id, error
            );
            return;
        }
    };

    let handoff_timeout = websocket_handoff_timeout_seconds();
    let socket = match timeout(Duration::from_secs(handoff_timeout), prover_socket_rx).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(_)) => {
            state.live_socket_txs.lock().await.remove(&session_id);
            set_session_failed(&state, &session_id, "verifier websocket handoff failed");
            cleanup_old_sessions(state).await;
            return;
        }
        Err(_) => {
            state.live_socket_txs.lock().await.remove(&session_id);
            set_session_failed(
                &state,
                &session_id,
                &format!("timed out waiting {handoff_timeout}s for verifier websocket"),
            );
            cleanup_old_sessions(state).await;
            return;
        }
    };

    let (bridge_socket, verifier_socket) = tokio::io::duplex(1 << 20);
    tokio::spawn(bridge_websocket_to_duplex(
        session_id.clone(),
        socket,
        bridge_socket,
    ));

    let verifier_timeout = verifier_run_timeout_seconds();
    match timeout(
        Duration::from_secs(verifier_timeout),
        run_verifier(verifier_socket, record.max_sent_data, record.max_recv_data),
    )
    .await
    {
        Ok(Ok(VerifierCompletion {
            server_name,
            transcript,
            transcript_commitments,
            connection_info,
            server_ephemeral_key,
        })) => {
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
            let material = NotaryMaterial {
                connection_info,
                server_ephemeral_key,
                transcript_commitments,
            };
            if let Err(error) = mark_session_completed(&state, &session_id, result, material) {
                error!(
                    "session={} failed to persist completion: {}",
                    session_id, error
                );
            }
            info!("session={} verifier completed", session_id);
        }
        Ok(Err(error)) => {
            error!("session={} verifier failed: {}", session_id, error);
            set_session_failed(&state, &session_id, &error.to_string());
        }
        Err(_) => {
            let message = format!("verifier run timed out after {verifier_timeout}s");
            error!("session={} {}", session_id, message);
            set_session_failed(&state, &session_id, &message);
        }
    }

    cleanup_old_sessions(state).await;
}

fn mark_session_in_progress(
    state: &AppState,
    session_id: &str,
) -> Result<Option<SessionRecord>, crate::state::SessionStoreError> {
    let Some(mut record) = state.store.get(session_id)? else {
        return Ok(None);
    };
    record.status = SessionStatus::InProgress;
    state.store.save(session_id, &record)?;
    Ok(Some(record))
}

fn mark_session_completed(
    state: &AppState,
    session_id: &str,
    result: SessionResult,
    material: NotaryMaterial,
) -> Result<(), crate::state::SessionStoreError> {
    let Some(mut record) = state.store.get(session_id)? else {
        return Ok(());
    };
    record.status = SessionStatus::Completed(result);
    record.notary_material = Some(material);
    state.store.save(session_id, &record)
}

pub(crate) fn set_session_failed(state: &AppState, session_id: &str, message: &str) {
    match state.store.get(session_id) {
        Ok(Some(mut record)) => {
            record.status = SessionStatus::Failed {
                message: message.to_string(),
            };
            if let Err(error) = state.store.save(session_id, &record) {
                error!(
                    "session={} failed to persist failure: {}",
                    session_id, error
                );
            }
        }
        Ok(None) => {
            error!("session={} missing while marking failed", session_id);
        }
        Err(error) => {
            error!(
                "session={} failed to load for failure update: {}",
                session_id, error
            );
        }
    }
}

pub(crate) async fn cleanup_old_sessions(state: AppState) {
    let now = Utc::now();
    let session_ttl = session_ttl_seconds();
    let stale_cutoff = now - chrono::Duration::seconds(session_ttl);
    let delete_cutoff = now - chrono::Duration::seconds(session_retention_seconds());
    let stale_message = format!("session exceeded {session_ttl}s active TTL");

    match state
        .store
        .mark_stale_active_failed(stale_cutoff, &stale_message)
    {
        Ok(count) if count > 0 => {
            info!("marked {count} stale active session(s) failed");
        }
        Ok(_) => {}
        Err(error) => {
            error!("stale session cleanup failed: {}", error);
        }
    }

    if let Err(error) = state.store.delete_older_than(delete_cutoff) {
        error!("old session cleanup failed: {}", error);
    }
    let mut live_sockets = state.live_socket_txs.lock().await;
    live_sockets.retain(|session_id, _| match state.store.get(session_id) {
        Ok(Some(session)) => session.created_at_utc >= stale_cutoff && session.status.is_active(),
        Ok(None) => false,
        Err(error) => {
            error!("session={} cleanup lookup failed: {}", session_id, error);
            true
        }
    });
}
