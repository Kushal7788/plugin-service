use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;
use tlsn::attestation::presentation::Presentation;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProofEnvelope {
    pub(crate) format: String,
    #[serde(default)]
    pub(crate) encoding: Option<String>,
    #[serde(default)]
    pub(crate) presentation: serde_json::Value,
}

pub(crate) fn decode_proof_envelope(payload: &serde_json::Value) -> Result<ProofEnvelope, String> {
    let raw = payload
        .get("proof")
        .cloned()
        .unwrap_or_else(|| payload.clone());
    serde_json::from_value::<ProofEnvelope>(raw)
        .map_err(|error| format!("proof envelope parse failed: {error}"))
}

pub(crate) fn decode_presentation_from_envelope(
    envelope: &ProofEnvelope,
) -> Result<Presentation, String> {
    match envelope.format.as_str() {
        "proof.presentation" => {
            serde_json::from_value::<Presentation>(envelope.presentation.clone())
                .map_err(|error| format!("presentation json parse failed: {error}"))
        }
        "proof.presentation.bincode" => {
            if envelope.encoding.as_deref().unwrap_or("base64") != "base64" {
                return Err("unsupported presentation encoding".to_string());
            }
            let encoded = envelope
                .presentation
                .as_str()
                .ok_or_else(|| "bincode presentation must be base64 string".to_string())?;
            let bytes = BASE64_STANDARD
                .decode(encoded)
                .map_err(|error| format!("presentation base64 decode failed: {error}"))?;
            bincode::deserialize::<Presentation>(&bytes)
                .map_err(|error| format!("presentation bincode parse failed: {error}"))
        }
        other => Err(format!("unsupported proof format: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_envelope_parser_accepts_nested_shape() {
        let payload = serde_json::json!({
            "proof": {
                "format": "proof.presentation.bincode",
                "encoding": "base64",
                "presentation": "AQID"
            }
        });
        let envelope = decode_proof_envelope(&payload).expect("envelope should parse");
        assert_eq!(envelope.format, "proof.presentation.bincode");
        assert_eq!(envelope.encoding.as_deref(), Some("base64"));
    }

    #[test]
    fn bincode_envelope_requires_valid_base64() {
        let envelope = ProofEnvelope {
            format: "proof.presentation.bincode".to_string(),
            encoding: Some("base64".to_string()),
            presentation: serde_json::json!("not-base64!"),
        };
        assert!(decode_presentation_from_envelope(&envelope).is_err());
    }

    #[test]
    fn json_envelope_requires_valid_presentation_object() {
        let envelope = ProofEnvelope {
            format: "proof.presentation".to_string(),
            encoding: None,
            presentation: serde_json::json!({"unexpected": "shape"}),
        };
        assert!(decode_presentation_from_envelope(&envelope).is_err());
    }
}
