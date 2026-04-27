use std::collections::{HashMap, HashSet};

use crate::state::SessionRevealPolicy;
use crate::types::{HandlerPart, HandlerType, RevealConfig};

pub(crate) fn parse_reveal_policy(
    session_data: &HashMap<String, String>,
) -> Result<SessionRevealPolicy, String> {
    let allowed_request_headers = session_data
        .get("allowedRequestHeaders")
        .ok_or_else(|| "sessionData.allowedRequestHeaders is required".to_string())
        .and_then(|raw| {
            serde_json::from_str::<Vec<String>>(raw)
                .map_err(|error| format!("allowedRequestHeaders parse failed: {error}"))
        })?;
    let allowed_response_body_json_paths = session_data
        .get("allowedResponseBodyJsonPaths")
        .ok_or_else(|| "sessionData.allowedResponseBodyJsonPaths is required".to_string())
        .and_then(|raw| {
            serde_json::from_str::<Vec<String>>(raw)
                .map_err(|error| format!("allowedResponseBodyJsonPaths parse failed: {error}"))
        })?;
    if allowed_request_headers.is_empty() {
        return Err("allowedRequestHeaders must not be empty".to_string());
    }
    if allowed_response_body_json_paths.is_empty() {
        return Err("allowedResponseBodyJsonPaths must not be empty".to_string());
    }
    Ok(SessionRevealPolicy {
        allowed_request_headers,
        allowed_response_body_json_paths,
    })
}

pub(crate) fn validate_reveal_config(
    policy: &SessionRevealPolicy,
    reveal_config: &RevealConfig,
) -> Result<(), String> {
    if policy.allowed_request_headers.is_empty() {
        return Err("session reveal policy has empty allowed request header set".to_string());
    }
    if reveal_config.sent.is_empty() || reveal_config.recv.is_empty() {
        return Err("reveal config must include sent and recv ranges".to_string());
    }
    for range in &reveal_config.sent {
        if range.start >= range.end {
            return Err("sent reveal range start must be less than end".to_string());
        }
        if !matches!(range.handler.handler_type, HandlerType::Sent) {
            return Err("sent reveal entry must use SENT handler type".to_string());
        }
        if !matches!(
            range.handler.part,
            HandlerPart::Method | HandlerPart::RequestTarget
        ) {
            return Err("sent reveal entry can only disclose METHOD or REQUEST_TARGET".to_string());
        }
    }

    let allowed_paths = policy
        .allowed_response_body_json_paths
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let mut seen_paths = HashSet::new();
    for range in &reveal_config.recv {
        if range.start >= range.end {
            return Err("recv reveal range start must be less than end".to_string());
        }
        if !matches!(range.handler.handler_type, HandlerType::Recv) {
            return Err("recv reveal entry must use RECV handler type".to_string());
        }
        if !matches!(range.handler.part, HandlerPart::Body) {
            return Err("recv reveal entry can only disclose BODY".to_string());
        }
        let params = range
            .handler
            .params
            .as_ref()
            .ok_or_else(|| "recv BODY reveal entry must include params".to_string())?;
        if params.content_type.as_deref().unwrap_or_default() != "json" {
            return Err("recv BODY reveal entry must declare params.type=json".to_string());
        }
        let path = params
            .path
            .as_ref()
            .ok_or_else(|| "recv BODY reveal entry must include params.path".to_string())?;
        if !allowed_paths.contains(path) {
            return Err(format!("recv BODY reveal path `{path}` is not allowlisted"));
        }
        seen_paths.insert(path.clone());
    }

    for required_path in &policy.allowed_response_body_json_paths {
        if !seen_paths.contains(required_path) {
            return Err(format!(
                "reveal config missing required allowlisted path `{required_path}`"
            ));
        }
    }

    Ok(())
}

pub(crate) fn validate_sanitized_payload_shape(payload: &serde_json::Value) -> Result<(), String> {
    let request = payload
        .get("request")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "payload.request is required".to_string())?;
    let endpoint = request
        .get("endpoint")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "payload.request.endpoint is required".to_string())?;
    for key in ["method", "host", "path"] {
        if endpoint.get(key).and_then(|value| value.as_str()).is_none() {
            return Err(format!("payload.request.endpoint.{key} must be a string"));
        }
    }

    let response = payload
        .get("response")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "payload.response is required".to_string())?;
    if response.get("headers").is_some() {
        return Err("payload.response.headers must not be present".to_string());
    }
    let status = response
        .get("status")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| "payload.response.status must be a number".to_string())?;
    if status == 0 {
        return Err("payload.response.status must be greater than zero".to_string());
    }
    response
        .get("revealedBody")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "payload.response.revealedBody must be an object".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Handler, HandlerParams, RangeWithHandler};

    #[test]
    fn reveal_config_validation_rejects_response_headers() {
        let policy = SessionRevealPolicy {
            allowed_request_headers: vec!["accept".to_string()],
            allowed_response_body_json_paths: vec!["user.name".to_string()],
        };
        let reveal_config = RevealConfig {
            sent: vec![RangeWithHandler {
                start: 0,
                end: 3,
                handler: Handler {
                    handler_type: HandlerType::Sent,
                    part: HandlerPart::Method,
                    params: None,
                },
            }],
            recv: vec![RangeWithHandler {
                start: 10,
                end: 30,
                handler: Handler {
                    handler_type: HandlerType::Recv,
                    part: HandlerPart::Headers,
                    params: Some(HandlerParams {
                        content_type: Some("json".to_string()),
                        path: Some("user.name".to_string()),
                    }),
                },
            }],
        };
        assert!(validate_reveal_config(&policy, &reveal_config).is_err());
    }

    #[test]
    fn reveal_config_validation_accepts_allowed_paths() {
        let policy = SessionRevealPolicy {
            allowed_request_headers: vec!["accept".to_string()],
            allowed_response_body_json_paths: vec!["profile.name".to_string()],
        };
        let reveal_config = RevealConfig {
            sent: vec![RangeWithHandler {
                start: 0,
                end: 3,
                handler: Handler {
                    handler_type: HandlerType::Sent,
                    part: HandlerPart::Method,
                    params: None,
                },
            }],
            recv: vec![RangeWithHandler {
                start: 10,
                end: 30,
                handler: Handler {
                    handler_type: HandlerType::Recv,
                    part: HandlerPart::Body,
                    params: Some(HandlerParams {
                        content_type: Some("json".to_string()),
                        path: Some("profile.name".to_string()),
                    }),
                },
            }],
        };
        assert!(validate_reveal_config(&policy, &reveal_config).is_ok());
    }

    #[test]
    fn sanitized_payload_validation_rejects_response_headers() {
        let payload = serde_json::json!({
            "request": {
                "endpoint": {
                    "method": "GET",
                    "host": "example.com",
                    "path": "/api/profile"
                }
            },
            "response": {
                "status": 200,
                "headers": [{"name":"set-cookie","value":"secret"}],
                "revealedBody": {"user.name": "demo"}
            }
        });
        assert!(validate_sanitized_payload_shape(&payload).is_err());
    }

    #[test]
    fn sanitized_payload_validation_accepts_minimal_shape_without_optional_sections() {
        let payload = serde_json::json!({
            "request": {
                "endpoint": {
                    "method": "POST",
                    "host": "www.kaggle.com",
                    "path": "/api/i/routing.RoutingService/GetPageDataByUrl"
                }
            },
            "response": {
                "status": 200,
                "revealedBody": {
                    "userProfile.userJoinDate": "2024-03-09T15:26:40.023Z"
                }
            },
            "proof": {
                "format": "tlsn.presentation.bincode",
                "encoding": "base64",
                "presentation": "AQID"
            },
            "session": {
                "sessionId": "sample-session",
                "verifierSessionId": "verifier-session"
            }
        });
        assert!(validate_sanitized_payload_shape(&payload).is_ok());
    }
}
