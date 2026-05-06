use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use tlsn::attestation::{
    signing::{Secp256k1Signer, Signer},
    CryptoProvider,
};

pub(crate) fn load_env_files() {
    let manifest_env = format!("{}/.env", env!("CARGO_MANIFEST_DIR"));
    let _ = dotenvy::from_filename(&manifest_env);
    let _ = dotenvy::dotenv();
}

pub(crate) fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(7047)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionStoreConfig {
    Memory,
    Sqlite { path: String },
}

pub(crate) fn session_store_config() -> SessionStoreConfig {
    match std::env::var("SESSION_STORE") {
        Ok(value) if value.eq_ignore_ascii_case("memory") => SessionStoreConfig::Memory,
        _ => SessionStoreConfig::Sqlite {
            path: std::env::var("SESSION_STORE_SQLITE_PATH")
                .unwrap_or_else(|_| "/tmp/mobile_tlsn_verifier_sessions.sqlite3".to_string()),
        },
    }
}

pub(crate) fn session_ttl_seconds() -> i64 {
    env_i64("SESSION_TTL_SECONDS", 600).max(1)
}

pub(crate) fn session_retention_seconds() -> i64 {
    env_i64("SESSION_RETENTION_SECONDS", 3600).max(session_ttl_seconds())
}

pub(crate) fn verifier_run_timeout_seconds() -> u64 {
    env_u64(
        "VERIFIER_RUN_TIMEOUT_SECONDS",
        session_ttl_seconds().try_into().unwrap_or(600),
    )
    .max(1)
}

pub(crate) fn websocket_handoff_timeout_seconds() -> u64 {
    env_u64("WEBSOCKET_HANDOFF_TIMEOUT_SECONDS", 60).max(1)
}

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

pub(crate) fn load_notary_signing_key() -> [u8; 32] {
    let fallback = [1_u8; 32];
    let raw = match std::env::var("NOTARY_SIGNING_KEY_B64") {
        Ok(value) => value,
        Err(_) => return fallback,
    };
    let decoded = match BASE64_STANDARD.decode(raw) {
        Ok(value) => value,
        Err(_) => return fallback,
    };
    if decoded.len() != 32 {
        return fallback;
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(&decoded);
    out
}

pub(crate) fn derive_trusted_notary_keys(signing_key: &[u8; 32]) -> Result<Vec<String>, String> {
    let signer = Secp256k1Signer::new(signing_key)
        .map_err(|error| format!("create secp256k1 signer failed: {error}"))?;
    Ok(vec![BASE64_STANDARD.encode(signer.verifying_key().data)])
}

pub(crate) fn build_notary_provider(signing_key: &[u8; 32]) -> Result<CryptoProvider, String> {
    let signer = Secp256k1Signer::new(signing_key)
        .map_err(|error| format!("create secp256k1 signer failed: {error}"))?;
    let mut provider = CryptoProvider::default();
    provider.signer.set_signer(Box::new(signer));
    Ok(provider)
}
