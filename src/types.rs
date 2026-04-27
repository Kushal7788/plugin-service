use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegisterSessionRequest {
    pub(crate) client_session_id: String,
    pub(crate) max_recv_data: usize,
    pub(crate) max_sent_data: usize,
    #[serde(default)]
    pub(crate) session_data: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegisterSessionResponse {
    pub(crate) session_id: String,
    pub(crate) verifier_ws_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ErrorResponse {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum HandlerType {
    Sent,
    Recv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum HandlerPart {
    StartLine,
    Protocol,
    Method,
    RequestTarget,
    StatusCode,
    Headers,
    Body,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Handler {
    #[serde(rename = "type")]
    pub(crate) handler_type: HandlerType,
    pub(crate) part: HandlerPart,
    #[serde(default)]
    pub(crate) params: Option<HandlerParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandlerParams {
    #[serde(alias = "type")]
    pub(crate) content_type: Option<String>,
    pub(crate) path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RangeWithHandler {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) handler: Handler,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevealConfig {
    pub(crate) sent: Vec<RangeWithHandler>,
    pub(crate) recv: Vec<RangeWithHandler>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResultResponse {
    pub(crate) status: String,
    pub(crate) session_id: String,
    pub(crate) client_session_id: String,
    pub(crate) created_at_utc: String,
    pub(crate) reveal_config_received: bool,
    pub(crate) result: Option<crate::state::SessionResult>,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HostQuery {
    pub(crate) host: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactVerifyRequest {
    pub(crate) artifact: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArtifactVerifyResponse {
    pub(crate) integrity_verified: bool,
    pub(crate) semantic_verified: bool,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttestationIssueRequest {
    pub(crate) attestation_request_b64: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttestationIssueResponse {
    pub(crate) attestation_b64: String,
}
