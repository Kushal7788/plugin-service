# Architecture

The service is one Rust binary built on axum and Tokio. It plays two TLSNotary roles at once: the **verifier**, which co-operates in an MPC-TLS session with a mobile prover, and the **notary**, which signs an attestation over what the verifier observed. A Cloudflare Worker can sit in front of it to give each session a dedicated container.

## 1. Module components

```mermaid
flowchart TB
    main["main.rs"] --> app["app.rs - startup and router"]
    app --> config["config.rs - env vars and notary key"]
    app --> handlers["handlers.rs - HTTP and WebSocket handlers"]
    app --> stateMod["state.rs - AppState and SessionStore"]

    handlers --> types["types.rs - DTOs"]
    handlers --> policy["policy.rs - reveal and payload rules"]
    handlers --> artifact["artifact.rs - envelope and presentation decode"]
    handlers --> session["session.rs - verifier task and cleanup"]
    handlers --> config
    handlers --> stateMod

    session --> bridge["ws_bridge.rs - WebSocket to duplex"]
    session --> verifier["verifier.rs - TLSN MPC verifier"]
    session --> stateMod
    session --> config

    stateMod --> mem["MemorySessionStore"]
    stateMod --> sqlite["SqliteSessionStore"]

    verifier --> tlsn[["tlsn crate v0.1.0-alpha.14"]]
    artifact --> tlsn
    config --> tlsn
    handlers --> tlsn
```

*Figure 1. Rust modules and their dependencies.*

`handlers.rs` is the only module that knows about HTTP. It delegates validation to `policy.rs`, proof decoding to `artifact.rs`, and long-running work to `session.rs`. `session.rs` owns the session lifecycle: it bridges the socket (`ws_bridge.rs`), drives the protocol (`verifier.rs`) and records status transitions in the store. `state.rs` defines `AppState`, which is cloned into every handler and holds:

| Field | Type | Role |
| --- | --- | --- |
| `store` | `Arc<dyn SessionStore>` | Persistent session records (SQLite or memory) |
| `live_socket_txs` | `Arc<Mutex<HashMap<String, oneshot::Sender<WebSocket>>>>` | Process-local handoff slots for prover sockets |
| `trusted_notary_keys` | `Arc<Vec<String>>` | Base64 public key derived from the signing key |
| `notary_signing_key` | `Arc<[u8; 32]>` | secp256k1 private key for attestations |

## 2. Deployment topology

```mermaid
flowchart LR
    Client["Mobile prover"] --> Worker["Cloudflare Worker - worker/index.ts"]
    Worker -- "/openapi.json, /docs" --> Static["Served by the Worker"]
    Worker -- "POST /v1/sessions/register" --> Mint["Mint UUID and inject sessionId"]
    Mint --> CS["Container named by sessionId"]
    Worker -- "/v1/sessions/{id}/..." --> CS
    Worker -- "/health, /v1/info, /v1/artifacts/verify" --> CC["Container named control"]
    CS --> Img["Rust service image (Dockerfile)"]
    CC --> Img
```

*Figure 2. Request routing when deployed on Cloudflare Workers + Containers.*

The live WebSocket handoff map and the running verifier task exist only in the memory of the process that handled registration. A load balancer that spreads one session's requests across instances would break it: the WebSocket upgrade would return `409 live_session_not_available`. The Worker avoids this by choosing the container name before the request reaches Rust: it mints a UUID, injects it as `sessionId` in the registration body, and routes every later `/v1/sessions/{id}/...` request (including the WebSocket upgrade) to `getContainer(env.VERIFIER_CONTAINER, id)`. Routes without a session ID go to a shared `control` container. All containers receive the same `NOTARY_SIGNING_KEY_B64`, so the `control` instance can verify artifacts whose attestations were signed by any session container. `wrangler.jsonc` caps the deployment at 20 container instances, and `sleepAfter = "15m"` keeps a container alive past the default 10-minute session TTL.

## 3. Request lifecycle

```mermaid
sequenceDiagram
    autonumber
    participant P as Mobile prover
    participant H as handlers.rs
    participant St as SessionStore
    participant T as session task
    participant B as ws_bridge
    participant V as TLSN verifier
    participant S as Target server

    P->>H: POST /v1/sessions/register
    H->>H: parse_reveal_policy(sessionData)
    H->>St: insert(record, status Pending)
    H->>H: store oneshot sender in live_socket_txs
    H-)T: spawn run_verifier_session(receiver)
    H-->>P: sessionId, verifierWsUrl
    T->>St: save(status InProgress)

    P->>H: GET /v1/sessions/{id}/verifier (Upgrade)
    H->>H: remove sender, send upgraded socket
    H-->>T: WebSocket via oneshot
    T->>B: bridge socket to 1 MiB duplex
    T->>V: run_verifier(duplex, limits)
    P->>S: TLS handshake and HTTP request
    P->>V: MPC protocol messages over WebSocket
    V->>V: commit, check MPC and limits, accept, run, verify
    V-->>T: server name, transcript, commitments, connection info
    T->>St: save(status Completed, NotaryMaterial)
    T->>St: cleanup stale and expired sessions

    P->>H: POST /v1/sessions/{id}/reveal-config
    H->>H: validate_reveal_config(policy, config)
    H->>St: save(reveal_config)
    H-->>P: ok

    P->>H: POST /v1/sessions/{id}/attestation
    H->>St: get(NotaryMaterial)
    H->>H: accept request, build and sign attestation
    H-->>P: attestationB64

    P->>H: GET /v1/sessions/{id}/result
    H-->>P: status completed, digests

    P->>H: POST /v1/artifacts/verify
    H->>H: payload shape, envelope decode, key check, Presentation verify
    H-->>P: integrityVerified, semanticVerified
```

*Figure 3. The main flow for one proof, from registration to artifact verification.*

Registration does not block on the MPC work. It records the session, creates a `oneshot` channel, and spawns a Tokio task that immediately moves the session to `in_progress` and waits up to `WEBSOCKET_HANDOFF_TIMEOUT_SECONDS` for the socket. The WebSocket handler's only job is to pass the upgraded socket through that channel. The task then runs the verifier under `VERIFIER_RUN_TIMEOUT_SECONDS`. After the session completes, the prover builds its attestation request locally, has the notary sign it, and later creates a presentation that a relying party can submit to `/v1/artifacts/verify`. The plugin also posts its reveal config, which is validated against the policy fixed at registration. The ordering of reveal-config, attestation and result calls shown here is the typical client order; the server only requires that attestation follows verifier completion.

### Verifier phases

`verifier.rs` drives the `tlsn` verifier state machine:

| Phase | What the service does |
| --- | --- |
| Setup | `Session::new(socket.compat())`, spawn the session driver, build `VerifierConfig` with the cached Mozilla root store |
| `commit()` | Receive the prover's request; reject anything other than `TlsCommitProtocolConfig::Mpc`; reject if `max_sent_data` or `max_recv_data` exceed the registered limits |
| `accept()` then `run()` | Take part in the MPC-TLS connection to the target server |
| `verify()` then `accept()` | Obtain `VerifierOutput`: server name, partial transcript, transcript commitments |
| `close()` | Close the verifier and session handle, join the driver task |
| Post-processing | Require server name and transcript; build `ConnectionInfo` using the sum of `ApplicationData` ciphertext lengths; extract the server ephemeral key |

## 4. Session state machine

```mermaid
stateDiagram-v2
    [*] --> Pending: register
    Pending --> InProgress: verifier task starts
    InProgress --> Completed: verifier finished
    InProgress --> Failed: handoff channel dropped
    InProgress --> Failed: handoff timeout
    InProgress --> Failed: verifier error
    InProgress --> Failed: run timeout
    Pending --> Failed: TTL cleanup
    InProgress --> Failed: TTL cleanup
    Completed --> [*]: retention cleanup
    Failed --> [*]: retention cleanup
```

*Figure 4. `SessionStatus` transitions (`src/state.rs`, `src/session.rs`).*

`Pending` is usually short-lived, because the spawned task marks the session `InProgress` before it starts waiting for the WebSocket. Failures record a human-readable message that `/result` returns as `errorMessage`. `cleanup_old_sessions` runs at startup and at the end of every verifier task. It:

1. marks `Pending`/`InProgress` sessions older than `SESSION_TTL_SECONDS` as `Failed` ("session exceeded Ns active TTL"),
2. deletes all records older than `SESSION_RETENTION_SECONDS` (never less than the TTL), and
3. prunes handoff slots whose session is gone, no longer active, or past the TTL.

Running cleanup at startup means a restarted container marks sessions it can no longer serve as failed instead of reporting them as in progress indefinitely.

## 5. Data model

```mermaid
classDiagram
    class SessionRecord {
        String client_session_id
        usize max_recv_data
        usize max_sent_data
        DateTime created_at_utc
        HashMap session_data
        SessionRevealPolicy reveal_policy
        Option~RevealConfig~ reveal_config
        Option~NotaryMaterial~ notary_material
        SessionStatus status
    }
    class SessionRevealPolicy {
        Vec~String~ allowed_request_headers
        Vec~String~ allowed_response_body_json_paths
    }
    class RevealConfig {
        Vec~RangeWithHandler~ sent
        Vec~RangeWithHandler~ recv
    }
    class RangeWithHandler {
        usize start
        usize end
        Handler handler
    }
    class Handler {
        HandlerType type
        HandlerPart part
        Option~HandlerParams~ params
    }
    class NotaryMaterial {
        ConnectionInfo connection_info
        ServerEphemKey server_ephemeral_key
        Vec~TranscriptCommitment~ transcript_commitments
    }
    class SessionStatus {
        <<enumeration>>
        Pending
        InProgress
        Completed
        Failed
    }
    class SessionResult {
        String server_name
        usize transcript_sent_len
        usize transcript_recv_len
        String transcript_sent_digest_base64
        String transcript_recv_digest_base64
        String verified_at_utc
    }
    SessionRecord --> SessionRevealPolicy
    SessionRecord --> RevealConfig
    SessionRecord --> NotaryMaterial
    SessionRecord --> SessionStatus
    SessionStatus --> SessionResult : Completed carries
    RevealConfig --> RangeWithHandler
    RangeWithHandler --> Handler
```

*Figure 5. The session record persisted for each session.*

The whole `SessionRecord`, including the TLSN `NotaryMaterial` needed to issue an attestation later, is serialized with serde_json. The SQLite backend stores it in one table:

```sql
CREATE TABLE IF NOT EXISTS sessions (
    session_id     TEXT PRIMARY KEY NOT NULL,
    created_at_utc TEXT NOT NULL,   -- RFC 3339, used for TTL and retention cutoffs
    updated_at_utc TEXT NOT NULL,
    record_json    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_created_at_utc ON sessions(created_at_utc);
```

Both stores implement the same `SessionStore` trait (`insert`, `get`, `save`, `mark_stale_active_failed`, `delete_older_than`). Each store serializes access with a `std::sync::Mutex`: the SQLite store holds a single `rusqlite::Connection`, and the memory store holds a `HashMap`. Status, reveal policy and timestamps live inside the JSON blob, so the TTL sweep loads candidate rows by `created_at_utc` and checks their status in Rust.

## 6. Data flow and trust boundaries

```mermaid
flowchart LR
    subgraph Prover["Prover side (untrusted)"]
        Req["sessionData policy"]
        RC["Reveal config"]
        AR["AttestationRequest"]
        Pres["Presentation artifact"]
    end
    subgraph Service["Verifier and notary"]
        Pol["SessionRevealPolicy"]
        NM["NotaryMaterial from MPC"]
        Key["NOTARY_SIGNING_KEY_B64"]
        PubKey["Derived public key"]
        Att["Signed Attestation"]
    end
    Req --> Pol
    RC -- "validated against" --> Pol
    AR --> Att
    NM --> Att
    Key --> Att
    Key --> PubKey
    Pres -- "key must match" --> PubKey
```

*Figure 6. Which inputs come from the prover and which come from the verifier's own observations.*

The attestation's connection info, server ephemeral key and transcript commitments come from `NotaryMaterial`, which is produced by the verifier during MPC, so the prover cannot substitute them. The signing key never leaves the process, and the trusted verification key is derived from it at startup rather than configured separately. Inputs supplied by the prover (the policy strings, the reveal config, the attestation request and the presentation) are all parsed and validated before use.

## 7. Operational boundaries

- **Persistence vs. liveness.** SQLite makes session records survive restarts, but the WebSocket and verifier task do not. Container disk is instance-local, so on Cloudflare the database is per container.
- **Timeouts.** Handoff (default 60 s), verifier run (default equal to the TTL), TTL (default 600 s) and retention (default 3600 s) bound how long any session can hold resources.
- **Startup is fail-fast.** A missing or malformed `NOTARY_SIGNING_KEY_B64`, or a SQLite file that cannot be opened, panics before the listener binds.
- **Security posture.** No authentication, permissive CORS, and no rate limiting. These are expected to be handled by a gateway in front of the service.
