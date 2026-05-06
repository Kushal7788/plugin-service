# Verifier Service Architecture

The verifier service is a single Rust binary built with Axum and Tokio.

## Request Flow

1. `POST /v1/sessions/register` stores a session record and spawns the TLSN verifier task.
2. The prover connects to `GET /v1/sessions/{sessionId}/verifier` as a WebSocket.
3. `ws_bridge.rs` adapts WebSocket frames into a Tokio duplex stream for the TLSN verifier runtime.
4. `verifier.rs` runs the TLSNotary verifier state machine and stores transcript commitments and connection material.
5. The plugin submits reveal config and requests `/attestation`.
6. `/v1/artifacts/verify` validates sanitized payload shape, decodes the presentation, checks the trusted notary key policy, and verifies the presentation signature.

For Cloudflare Container deployments, `worker/index.ts` mints or preserves the
registration `sessionId` and routes every `/v1/sessions/{sessionId}/...` request
to `getContainer(env.VERIFIER_CONTAINER, sessionId)`. This gives active verifier
sessions a stable named container instance instead of relying on load balancing.

## Modules

- `app`: router and server startup.
- `config`: env loading and notary key handling.
- `handlers`: transport-facing API handlers.
- `session`: long-running verifier task and session cleanup.
- `policy`: reveal and payload validation rules.
- `artifact`: proof envelope decoding.
- `state`: shared app state, live WebSocket handoff map, and session store backends.
- `types`: wire DTOs.
- `ws_bridge`: WebSocket/duplex adapter.
- `verifier`: TLSN verifier integration.

## Operational Boundaries

By default, session records are stored in SQLite. Set `SESSION_STORE=memory`
for local development when you do not want a DB file.

Active sessions are bounded by `SESSION_TTL_SECONDS` and stale `pending` or
`in_progress` sessions are marked `failed`. Records remain queryable until
`SESSION_RETENTION_SECONDS`, then cleanup deletes them. Cleanup runs at startup
and after verifier tasks exit, so a restarted container marks old active SQLite
records failed instead of leaving them dangling indefinitely.

SQLite removes process memory as the source of truth for persisted session
records, but it does not make active verifier tasks portable. The live WebSocket
handoff and TLSN verifier runtime remain process-local, so multi-instance
deployments still need sticky routing for all requests belonging to an in-flight
session. `VERIFIER_RUN_TIMEOUT_SECONDS` adds a hard cap after WebSocket handoff;
`WEBSOCKET_HANDOFF_TIMEOUT_SECONDS` caps the registration-to-WebSocket wait.

`NOTARY_SIGNING_KEY_B64` is the only notary key deployment secret. The service
derives the matching compressed secp256k1 public verifying key at startup and
uses that for self-issued artifact verification.
