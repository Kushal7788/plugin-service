use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use tlsn::attestation::{signing::Secp256k1Signer, CryptoProvider};

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

pub(crate) fn public_host() -> String {
    std::env::var("PUBLIC_HOST").unwrap_or_else(|_| "localhost:7047".to_string())
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

pub(crate) fn load_trusted_notary_keys() -> Vec<String> {
    match std::env::var("TRUSTED_NOTARY_KEYS_B64") {
        Ok(value) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn build_notary_provider(signing_key: &[u8; 32]) -> Result<CryptoProvider, String> {
    let signer = Secp256k1Signer::new(signing_key)
        .map_err(|error| format!("create secp256k1 signer failed: {error}"))?;
    let mut provider = CryptoProvider::default();
    provider.signer.set_signer(Box::new(signer));
    Ok(provider)
}
