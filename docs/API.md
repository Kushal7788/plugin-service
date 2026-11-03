# API Reference

All routes are defined in `src/app.rs` and implemented in `src/handlers.rs`; wire types live in `src/types.rs`. JSON field names are camelCase.

- Local base URL: `http://localhost:7047` (see `PORT`).
- CORS: fully permissive (`tower_http::cors::CorsLayer::permissive()`).
- Authentication: none.

## Endpoint summary

| Method | Path | Served by | Purpose |
| --- | --- | --- | --- |
| `GET` | `/health` | Rust service | Liveness check |
| `GET` | `/v1/info` | Rust service | Service name and API version |
| `POST` | `/v1/sessions/register` | Rust service (Worker mints `sessionId`) | Create a verifier session and start its verifier task |
| `GET` (WebSocket) | `/v1/sessions/{sessionId}/verifier` | Rust service | Prover connection for the MPC protocol |
| `POST` | `/v1/sessions/{sessionId}/reveal-config` | Rust service | Submit and validate reveal ranges |
| `POST` | `/v1/sessions/{sessionId}/attestation` | Rust service | Issue a signed attestation |
| `GET` | `/v1/sessions/{sessionId}/result` | Rust service | Poll session status and transcript digests |
| `POST` | `/v1/artifacts/verify` | Rust service | Verify an exported proof artifact |
| `GET`, `HEAD` | `/openapi.json` | Cloudflare Worker only | OpenAPI 3.0.3 schema (`worker/openapi-schema.ts`) |
| `GET`, `HEAD` | `/docs` | Cloudflare Worker only | Swagger UI backed by `/openapi.json` |

## Errors

Handler-level errors use a single JSON shape (`ErrorResponse`):

```json
{ "code": "session_not_found", "message": "unknown session id" }
```

Every handler that reads the session store can also return `500` with code `session_store_error`.

Requests rejected by axum extractors before a handler runs (for example a missing `Content-Type: application/json` header, malformed JSON, missing required fields, or a non-WebSocket request to the verifier route) receive axum's default **plain-text** rejection with a 4xx status, not the JSON shape above.

---

## `GET /health`

Returns the plain-text body `ok` with status `200`.

## `GET /v1/info`

```json
{ "service": "mobile_tlsn_verifier_service", "apiVersion": "v1" }
```

---

## `POST /v1/sessions/register`

Creates a session record with status `pending`, registers a one-time WebSocket handoff slot, and spawns the verifier task (which immediately marks the session `in_progress` and waits up to `WEBSOCKET_HANDOFF_TIMEOUT_SECONDS` for the prover).

### Request (`RegisterSessionRequest`)

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `sessionId` | string (UUID) | no | If present and non-blank, must parse as a UUID and must not already exist. Otherwise the service creates a v4 UUID. The Worker always fills this in. |
| `clientSessionId` | string | yes | Caller-side identifier; must not be blank. Echoed back by `/result`. |
| `maxRecvData` | integer | yes | Upper bound on the prover's `max_recv_data`; the verifier fails the session if the prover asks for more. |
| `maxSentData` | integer | yes | Upper bound on the prover's `max_sent_data`. |
| `sessionData` | object of string to string | no (defaults to `{}`) | Must contain the reveal policy below, so it is effectively required. |

Reveal policy keys inside `sessionData` (both are JSON-encoded string arrays and must be non-empty):

| Key | Example value | Meaning |
| --- | --- | --- |
| `allowedRequestHeaders` | `"[\"accept\"]"` | Parsed and required to be non-empty. |
| `allowedResponseBodyJsonPaths` | `"[\"user.name\",\"transactions[0].date\"]"` | Exact set of JSON paths the reveal config must disclose. |

```json
{
  "clientSessionId": "debug-session-001",
  "maxRecvData": 16384,
  "maxSentData": 4096,
  "sessionData": {
    "allowedRequestHeaders": "[\"accept\"]",
    "allowedResponseBodyJsonPaths": "[\"user.name\"]"
  }
}
```

### Response `200` (`RegisterSessionResponse`)

```json
{
  "sessionId": "3f1c2c4e-2d7b-4c55-9a43-1f0f7f3a9b11",
  "verifierWsUrl": "/v1/sessions/3f1c2c4e-2d7b-4c55-9a43-1f0f7f3a9b11/verifier"
}
```

`verifierWsUrl` is a relative path. Resolve it against the origin used for registration, using `ws://` for `http://` and `wss://` for `https://`.

### Errors

| Status | `code` | When |
| --- | --- | --- |
| 400 | `invalid_request` | `clientSessionId` is blank, or `sessionId` is not a UUID |
| 400 | `invalid_reveal_policy` | A policy key is missing, is not a JSON string array, or is empty |
| 409 | `session_exists` | `sessionId` already exists in the store |
| 500 | `session_store_error` | Store read/write failed |

When called through the Worker, a non-`POST` method returns `405 method_not_allowed`, an invalid JSON body returns `400 invalid_json`, and a non-object body returns `400 invalid_request`.

---

## `GET /v1/sessions/{sessionId}/verifier` (WebSocket)

The prover connects here with a WebSocket upgrade. After the upgrade, the socket is handed to the session's verifier task and bridged into the TLSNotary MPC runtime: binary and text frames are forwarded as raw bytes, and verifier output is sent back as binary frames. The payload is TLSNotary protocol traffic, not JSON.

| Status | `code` | When |
| --- | --- | --- |
| 101 | none | Upgrade accepted |
| 404 | `session_not_found` | Unknown session ID |
| 409 | `live_session_not_available` | No pending handoff on this process: the socket was already handed off, the handoff expired, or the request reached a different process than the one that registered the session |
| 500 | `session_store_error` | Store read failed |

---

## `POST /v1/sessions/{sessionId}/reveal-config`

Validates the prover's reveal ranges against the session's reveal policy and stores them on the session record. It can be called in any session state; a later valid call overwrites the stored config.

### Request (`RevealConfig`)

```json
{
  "sent": [
    { "start": 0, "end": 3, "handler": { "type": "SENT", "part": "METHOD" } }
  ],
  "recv": [
    {
      "start": 120,
      "end": 140,
      "handler": {
        "type": "RECV",
        "part": "BODY",
        "params": { "type": "json", "path": "user.name" }
      }
    }
  ]
}
```

| Type | Fields |
| --- | --- |
| `RangeWithHandler` | `start` (integer), `end` (integer), `handler` (`Handler`) |
| `Handler` | `type`: `SENT` or `RECV`; `part`: one of `START_LINE`, `PROTOCOL`, `METHOD`, `REQUEST_TARGET`, `STATUS_CODE`, `HEADERS`, `BODY`, `ALL`; `params` (optional `HandlerParams`) |
| `HandlerParams` | `contentType` (alias `type`), `path`; both optional strings |

Validation rules (`policy::validate_reveal_config`):

1. `sent` and `recv` must both be non-empty.
2. Every range must have `start < end`.
3. `sent` entries must use `type: SENT` and `part` `METHOD` or `REQUEST_TARGET`.
4. `recv` entries must use `type: RECV`, `part: BODY`, and `params` with `type`/`contentType` equal to `json` and a `path`.
5. Each `path` must be in `allowedResponseBodyJsonPaths`, must not repeat, and every allowlisted path must appear.

### Response `200`

```json
{ "ok": true, "sessionId": "3f1c2c4e-2d7b-4c55-9a43-1f0f7f3a9b11" }
```

### Errors

| Status | `code` | When |
| --- | --- | --- |
| 400 | `reveal_policy_violation` | Any rule above fails; `message` names the rule |
| 404 | `session_not_found` | Unknown session ID |
| 500 | `session_store_error` | Store read/write failed |

---

## `POST /v1/sessions/{sessionId}/attestation`

Issues a TLSNotary attestation for a completed session, signed with the notary key from `NOTARY_SIGNING_KEY_B64`. Connection info, server ephemeral key and transcript commitments come from the verifier's stored session material. The endpoint does not limit how many times it is called for a session.

### Request (`AttestationIssueRequest`)

```json
{ "attestationRequestB64": "<base64 of a bincode-serialized tlsn attestation Request>" }
```

### Response `200` (`AttestationIssueResponse`)

```json
{ "attestationB64": "<base64 of a bincode-serialized tlsn Attestation>" }
```

### Errors

| Status | `code` | When |
| --- | --- | --- |
| 400 | `invalid_attestation_request` | Base64 or bincode decoding failed |
| 400 | `attestation_request_rejected` | The TLSN builder rejected the request |
| 404 | `session_not_found` | Unknown session ID |
| 409 | `session_not_ready` | The verifier has not completed, so no notary material exists |
| 500 | `notary_provider_error`, `attestation_config_error`, `attestation_build_failed`, `attestation_encode_failed` | Signing or encoding failure |
| 500 | `session_store_error` | Store read failed |

---

## `GET /v1/sessions/{sessionId}/result`

### Response `200` (`SessionResultResponse`)

```json
{
  "status": "completed",
  "sessionId": "3f1c2c4e-2d7b-4c55-9a43-1f0f7f3a9b11",
  "clientSessionId": "debug-session-001",
  "createdAtUtc": "2026-01-01T12:00:00.000000+00:00",
  "revealConfigReceived": true,
  "result": {
    "serverName": "api.example.com",
    "transcriptSentLen": 412,
    "transcriptRecvLen": 3150,
    "transcriptSentDigestBase64": "<base64 sha256 of sent transcript>",
    "transcriptRecvDigestBase64": "<base64 sha256 of received transcript>",
    "verifiedAtUtc": "2026-01-01T12:00:09.000000+00:00"
  },
  "errorMessage": null
}
```

The values above are illustrative.

| Field | Notes |
| --- | --- |
| `status` | `pending`, `in_progress`, `completed` or `failed` |
| `createdAtUtc` | RFC 3339 timestamp |
| `revealConfigReceived` | Whether a valid reveal config has been stored |
| `result` | Present only when `completed`. Lengths and SHA-256 digests are computed over the verifier's partial transcript buffers (`sent_unsafe()` / `received_unsafe()`). |
| `errorMessage` | Present only when `failed` (handoff failure or timeout, verifier error, run timeout, or TTL expiry) |

Records are deleted after `SESSION_RETENTION_SECONDS`, after which this endpoint returns `404`.

### Errors

| Status | `code` | When |
| --- | --- | --- |
| 404 | `session_not_found` | Unknown or expired session ID |
| 500 | `session_store_error` | Store read failed |

---

## `POST /v1/artifacts/verify`

Verifies a proof artifact exported by the plugin. This route is not tied to a session.

### Request (`ArtifactVerifyRequest`)

`artifact.payload` must have this shape (`policy::validate_sanitized_payload_shape`). The proof envelope is read from `payload.proof`, or from `payload` itself if `proof` is absent.

```json
{
  "artifact": {
    "payload": {
      "request": {
        "endpoint": { "method": "GET", "host": "api.example.com", "path": "/v1/profile" }
      },
      "response": {
        "status": 200,
        "revealedBody": { "user.name": "demo" }
      },
      "proof": {
        "format": "proof.presentation.bincode",
        "encoding": "base64",
        "presentation": "<base64 bincode Presentation>"
      }
    }
  }
}
```

| Rule | Detail |
| --- | --- |
| `request.endpoint.method`, `.host`, `.path` | Must be strings |
| `response.status` | Must be a number greater than zero |
| `response.revealedBody` | Must be an object |
| `response.headers` | Must be absent |
| `proof.format` | `proof.presentation.bincode` (base64 string; `encoding` must be `base64` or omitted) or `proof.presentation` (JSON-serialized `Presentation` object) |

Other fields in the payload (for example a `session` object) are ignored.

### Response `200` (`ArtifactVerifyResponse`)

```json
{ "integrityVerified": true, "semanticVerified": true, "reason": null }
```

Verification failures return `200` with both flags `false` and a `reason`:

- `presentation signed by untrusted notary key`: the presentation's verifying key is not the public key derived from `NOTARY_SIGNING_KEY_B64`.
- `presentation verification failed: ...`: `Presentation::verify` failed.

`semanticVerified` currently always equals `integrityVerified`.

### Errors

| Status | `code` | When |
| --- | --- | --- |
| 400 | `invalid_artifact` | `artifact.payload` missing |
| 400 | `invalid_payload_shape` | A shape rule above failed |
| 400 | `invalid_proof_envelope` | Envelope JSON could not be parsed |
| 400 | `invalid_presentation` | Unsupported format or encoding, or decode failure |

---

## Worker-only routes

These are served by `worker/index.ts` without starting a container, and are not available when running the Rust binary directly.

| Route | Response |
| --- | --- |
| `GET /openapi.json` | `application/vnd.oai.openapi+json` OpenAPI 3.0.3 document with `access-control-allow-origin: *` |
| `GET /docs` | Swagger UI HTML page (loads `swagger-ui-dist@5` from unpkg) with a restrictive Content-Security-Policy |

Other methods on these paths return `405 method_not_allowed`.
