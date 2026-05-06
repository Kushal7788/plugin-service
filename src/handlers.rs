use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use tlsn::attestation::{
    request::Request as AttestationRequest, Attestation, AttestationConfig, CryptoProvider,
};
use tokio::sync::oneshot;
use tracing::{info, warn};
use uuid::Uuid;

use crate::artifact::{decode_presentation_from_envelope, decode_proof_envelope};
use crate::config::build_notary_provider;
use crate::policy::{
    parse_reveal_policy, validate_reveal_config, validate_sanitized_payload_shape,
};
use crate::session::run_verifier_session;
use crate::state::{AppState, SessionRecord, SessionStatus, SessionStoreError};
use crate::types::{
    ArtifactVerifyRequest, ArtifactVerifyResponse, AttestationIssueRequest,
    AttestationIssueResponse, ErrorResponse, RegisterSessionRequest, RegisterSessionResponse,
    RevealConfig, SessionResultResponse,
};

pub(crate) async fn health_handler() -> &'static str {
    "ok"
}

pub(crate) async fn info_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "mobile_tlsn_verifier_service",
        "apiVersion": "v1",
    }))
}

pub(crate) async fn register_session_handler(
    State(state): State<AppState>,
    Json(req): Json<RegisterSessionRequest>,
) -> Result<Json<RegisterSessionResponse>, (StatusCode, Json<ErrorResponse>)> {
    if req.client_session_id.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "clientSessionId is required",
        ));
    }
    let session_id = match req.session_id.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => Uuid::parse_str(value)
            .map(|uuid| uuid.to_string())
            .map_err(|_| {
                api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "sessionId must be a UUID when provided",
                )
            })?,
        _ => Uuid::new_v4().to_string(),
    };
    if state
        .store
        .get(&session_id)
        .map_err(store_api_error)?
        .is_some()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "session_exists",
            "sessionId already exists",
        ));
    }
    let (tx, rx) = oneshot::channel::<WebSocket>();
    let reveal_policy = parse_reveal_policy(&req.session_data)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, "invalid_reveal_policy", message))?;

    let record = SessionRecord {
        client_session_id: req.client_session_id.clone(),
        max_recv_data: req.max_recv_data,
        max_sent_data: req.max_sent_data,
        created_at_utc: Utc::now(),
        session_data: req.session_data,
        reveal_policy,
        reveal_config: None,
        notary_material: None,
        status: SessionStatus::Pending,
    };
    state
        .store
        .insert(&session_id, &record)
        .map_err(store_api_error)?;
    state
        .live_socket_txs
        .lock()
        .await
        .insert(session_id.clone(), tx);
    tokio::spawn(run_verifier_session(state.clone(), session_id.clone(), rx));

    Ok(Json(RegisterSessionResponse {
        session_id: session_id.clone(),
        verifier_ws_url: format!("/v1/sessions/{session_id}/verifier"),
    }))
}

pub(crate) async fn verifier_ws_handler(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if state
        .store
        .get(&session_id)
        .map_err(store_api_error)?
        .is_none()
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "unknown session id",
        ));
    };
    if !state.live_socket_txs.lock().await.contains_key(&session_id) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "live_session_not_available",
            "session is not active on this process",
        ));
    }
    Ok(ws.on_upgrade(move |socket| async move {
        let tx = state.live_socket_txs.lock().await.remove(&session_id);
        match tx {
            Some(tx) => {
                if tx.send(socket).is_err() {
                    warn!("session={} verifier ws dropped before handoff", session_id);
                }
            }
            None => {
                warn!("session={} verifier ws attempted after handoff", session_id);
            }
        }
    }))
}

pub(crate) async fn reveal_config_handler(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(reveal_config): Json<RevealConfig>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let Some(mut record) = state.store.get(&session_id).map_err(store_api_error)? else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "unknown session id",
        ));
    };
    validate_reveal_config(&record.reveal_policy, &reveal_config).map_err(|message| {
        api_error(StatusCode::BAD_REQUEST, "reveal_policy_violation", message)
    })?;
    record.reveal_config = Some(reveal_config);
    state
        .store
        .save(&session_id, &record)
        .map_err(store_api_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "sessionId": session_id,
    })))
}

pub(crate) async fn session_result_handler(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<SessionResultResponse>, (StatusCode, Json<ErrorResponse>)> {
    let Some(record) = state.store.get(&session_id).map_err(store_api_error)? else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "unknown session id",
        ));
    };
    let (status, result, error_message) = match &record.status {
        SessionStatus::Pending => ("pending".to_string(), None, None),
        SessionStatus::InProgress => ("in_progress".to_string(), None, None),
        SessionStatus::Completed(value) => ("completed".to_string(), Some(value.clone()), None),
        SessionStatus::Failed { message } => ("failed".to_string(), None, Some(message.clone())),
    };
    Ok(Json(SessionResultResponse {
        status,
        session_id,
        client_session_id: record.client_session_id.clone(),
        created_at_utc: record.created_at_utc.to_rfc3339(),
        reveal_config_received: record.reveal_config.is_some(),
        result,
        error_message,
    }))
}

pub(crate) async fn issue_attestation_handler(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<AttestationIssueRequest>,
) -> Result<Json<AttestationIssueResponse>, (StatusCode, Json<ErrorResponse>)> {
    let Some(record) = state.store.get(&session_id).map_err(store_api_error)? else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "unknown session id",
        ));
    };
    let material = record.notary_material.clone().ok_or_else(|| {
        api_error(
            StatusCode::CONFLICT,
            "session_not_ready",
            "session not ready for attestation",
        )
    })?;
    let request_bytes = BASE64_STANDARD
        .decode(req.attestation_request_b64)
        .map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_attestation_request",
                format!("request base64 decode failed: {error}"),
            )
        })?;
    let attestation_request: AttestationRequest =
        bincode::deserialize(&request_bytes).map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_attestation_request",
                format!("request decode failed: {error}"),
            )
        })?;
    let provider = build_notary_provider(&state.notary_signing_key).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "notary_provider_error",
            error,
        )
    })?;
    let mut config_builder = AttestationConfig::builder();
    config_builder.supported_signature_algs(Vec::from_iter(provider.signer.supported_algs()));
    let config = config_builder.build().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attestation_config_error",
            format!("attestation config failed: {error}"),
        )
    })?;
    let mut builder = Attestation::builder(&config)
        .accept_request(attestation_request)
        .map_err(|error| {
            api_error(
                StatusCode::BAD_REQUEST,
                "attestation_request_rejected",
                format!("attestation request rejected: {error}"),
            )
        })?;
    builder
        .connection_info(material.connection_info)
        .server_ephemeral_key(material.server_ephemeral_key)
        .transcript_commitments(material.transcript_commitments);
    let attestation = builder.build(&provider).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "attestation_build_failed",
            format!("attestation build failed: {error}"),
        )
    })?;
    let attestation_b64 =
        BASE64_STANDARD.encode(bincode::serialize(&attestation).map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "attestation_encode_failed",
                format!("attestation encode failed: {error}"),
            )
        })?);
    Ok(Json(AttestationIssueResponse { attestation_b64 }))
}

pub(crate) async fn artifact_verify_handler(
    State(state): State<AppState>,
    Json(req): Json<ArtifactVerifyRequest>,
) -> Result<Json<ArtifactVerifyResponse>, (StatusCode, Json<ErrorResponse>)> {
    let payload = req
        .artifact
        .get("payload")
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_artifact",
                "artifact payload missing",
            )
        })?
        .clone();
    validate_sanitized_payload_shape(&payload)
        .map_err(|message| api_error(StatusCode::BAD_REQUEST, "invalid_payload_shape", message))?;
    let envelope = decode_proof_envelope(&payload)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, "invalid_proof_envelope", error))?;
    info!(
        "artifact verify requested format={} encoding={}",
        envelope.format,
        envelope.encoding.as_deref().unwrap_or("-")
    );
    let presentation = decode_presentation_from_envelope(&envelope)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, "invalid_presentation", error))?;
    let key_b64 = BASE64_STANDARD.encode(&presentation.verifying_key().data);
    if !state.trusted_notary_keys.is_empty()
        && !state
            .trusted_notary_keys
            .iter()
            .any(|value| value == &key_b64)
    {
        warn!(
            "artifact verify rejected: untrusted key key_prefix={} trusted_count={}",
            key_b64.chars().take(24).collect::<String>(),
            state.trusted_notary_keys.len()
        );
        return Ok(Json(ArtifactVerifyResponse {
            integrity_verified: false,
            semantic_verified: false,
            reason: Some("presentation signed by untrusted notary key".to_string()),
        }));
    }
    let provider = CryptoProvider::default();
    match presentation.verify(&provider) {
        Ok(_) => {
            info!("artifact verify succeeded");
            Ok(Json(ArtifactVerifyResponse {
                integrity_verified: true,
                semantic_verified: true,
                reason: None,
            }))
        }
        Err(error) => {
            warn!("artifact verify failed: {}", error);
            Ok(Json(ArtifactVerifyResponse {
                integrity_verified: false,
                semantic_verified: false,
                reason: Some(format!("presentation verification failed: {error}")),
            }))
        }
    }
}

fn api_error(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            code: code.into(),
            message: message.into(),
        }),
    )
}

fn store_api_error(error: SessionStoreError) -> (StatusCode, Json<ErrorResponse>) {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "session_store_error",
        error.to_string(),
    )
}
