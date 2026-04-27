# Verifier Service Architecture

The verifier service is a single Rust binary built with Axum and Tokio.

## Request Flow

1. `POST /v1/sessions/register` creates an in-memory session record and spawns the TLSN verifier task.
2. The prover connects to `GET /v1/sessions/{sessionId}/verifier` as a WebSocket.
3. `ws_bridge.rs` adapts WebSocket frames into a Tokio duplex stream for the TLSN verifier runtime.
4. `verifier.rs` runs the TLSNotary verifier state machine and stores transcript commitments and connection material.
5. The plugin submits reveal config and requests `/attestation`.
6. `/v1/artifacts/verify` validates sanitized payload shape, decodes the presentation, checks the trusted notary key policy, and verifies the presentation signature.

## Modules

- `app`: router and server startup.
- `config`: env loading and notary key handling.
- `handlers`: transport-facing API handlers.
- `session`: long-running verifier task and session cleanup.
- `policy`: reveal and payload validation rules.
- `artifact`: proof envelope decoding.
- `state`: shared app/session state.
- `types`: wire DTOs.
- `ws_bridge`: WebSocket/duplex adapter.
- `verifier`: TLSN verifier integration.

## Operational Boundaries

Sessions are stored in process memory and cleaned after roughly ten minutes.
For multi-instance deployments, use sticky routing or replace the session store
with shared state before scaling horizontally.
