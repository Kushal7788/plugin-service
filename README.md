# Mobile TLSN Verifier Service

Standalone Axum service used by `mobile_tlsn_plugin` to run the TLSNotary
verifier side, issue attestations, and verify exported proof artifacts.

TLSN is pinned through Cargo to `https://github.com/tlsnotary/tlsn.git` tag
`v0.1.0-alpha.14`; a local `code/tlsn` checkout is not required.

## Requirements

- Rust stable with `rustfmt` and `clippy`.
- Network access to fetch the pinned TLSN Git dependency.
- Optional Docker for container builds.

## Quick Start

```bash
cd mobile_tlsn_verifier_service
cp .env.example .env
cargo run
```

The service listens on `http://0.0.0.0:7047` by default.

## Project Layout

- `src/main.rs`: thin executable bootstrap.
- `src/app.rs`: router construction and process startup.
- `src/config.rs`: environment loading, public host, notary keys, and crypto provider setup.
- `src/handlers.rs`: HTTP and WebSocket route handlers.
- `src/session.rs`: verifier session lifecycle and cleanup.
- `src/state.rs`: shared application/session state.
- `src/types.rs`: request and response DTOs.
- `src/policy.rs`: reveal policy and sanitized payload validation.
- `src/artifact.rs`: proof envelope and presentation decoding.
- `src/ws_bridge.rs`: WebSocket to duplex IO adapter.
- `src/verifier.rs`: TLSNotary verifier runtime.
- `docs/`: local run, API, testing, architecture, and deployment guides.

## API Surface

- `GET /health`
- `GET /v1/info`
- `POST /v1/sessions/register`
- `GET /v1/sessions/{sessionId}/verifier` as a WebSocket upgrade
- `POST /v1/sessions/{sessionId}/reveal-config`
- `POST /v1/sessions/{sessionId}/attestation`
- `GET /v1/sessions/{sessionId}/result`
- `POST /v1/artifacts/verify`

## Environment

Runtime config is loaded from `mobile_tlsn_verifier_service/.env` and then the
current working directory `.env` if present.

- `PORT`: HTTP port, default `7047`.
- `PUBLIC_HOST`: host returned in `verifierWsUrl`, default `localhost:7047`.
- `RUST_LOG`: tracing filter, default `mobile_tlsn_verifier_service=info,tlsn=warn`.
- `NOTARY_SIGNING_KEY_B64`: base64-encoded 32-byte secp256k1 private key. A deterministic development fallback is used when unset; production must set this.
- `TRUSTED_NOTARY_KEYS_B64`: comma-separated base64 public keys accepted by artifact verification.

## Plugin Integration

Set `TransportConfig.verifierUrl` in the plugin to this service. The plugin
will register a session, connect to the verifier WebSocket, submit reveal
ranges, request an attestation, poll the result, and call artifact verification.

See `docs/api_reference.md`, `docs/local_run.md`, `docs/testing_guide.md`, and
`docs/production_deployment.md` for detailed workflows.
