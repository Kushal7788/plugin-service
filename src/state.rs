use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::WebSocket;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tlsn::{connection::ConnectionInfo, transcript::TranscriptCommitment};
use tokio::sync::{oneshot, Mutex};

use crate::types::RevealConfig;

pub(crate) type SessionMap = Arc<Mutex<HashMap<String, Arc<Mutex<SessionRecord>>>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) sessions: SessionMap,
    pub(crate) trusted_notary_keys: Arc<Vec<String>>,
    pub(crate) notary_signing_key: Arc<[u8; 32]>,
}

impl AppState {
    pub(crate) fn new(trusted_notary_keys: Vec<String>, notary_signing_key: [u8; 32]) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            trusted_notary_keys: Arc::new(trusted_notary_keys),
            notary_signing_key: Arc::new(notary_signing_key),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionRecord {
    pub(crate) client_session_id: String,
    pub(crate) max_recv_data: usize,
    pub(crate) max_sent_data: usize,
    pub(crate) created_at_utc: DateTime<Utc>,
    pub(crate) session_data: HashMap<String, String>,
    pub(crate) reveal_policy: SessionRevealPolicy,
    pub(crate) reveal_config: Option<RevealConfig>,
    pub(crate) notary_material: Option<NotaryMaterial>,
    pub(crate) status: SessionStatus,
    pub(crate) prover_socket_tx: Option<oneshot::Sender<WebSocket>>,
}

#[derive(Debug, Clone)]
pub(crate) struct NotaryMaterial {
    pub(crate) connection_info: ConnectionInfo,
    pub(crate) server_ephemeral_key: tlsn::connection::ServerEphemKey,
    pub(crate) transcript_commitments: Vec<TranscriptCommitment>,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionStatus {
    Pending,
    InProgress,
    Completed(SessionResult),
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResult {
    pub(crate) server_name: String,
    pub(crate) transcript_sent_len: usize,
    pub(crate) transcript_recv_len: usize,
    pub(crate) transcript_sent_digest_base64: String,
    pub(crate) transcript_recv_digest_base64: String,
    pub(crate) verified_at_utc: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionRevealPolicy {
    pub(crate) allowed_request_headers: Vec<String>,
    pub(crate) allowed_response_body_json_paths: Vec<String>,
}
