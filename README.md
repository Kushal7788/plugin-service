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
cd mpc-verifier
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
- `worker/index.ts`: Cloudflare Worker that routes session traffic to named
  Cloudflare Container instances.
- `worker/wrangler.jsonc`: Cloudflare Worker, Container, and Durable Object
  config.
- `docs/`: local run, API, testing, architecture, and deployment guides.

## API Surface

- `GET /openapi.json` from the Cloudflare Worker deployment
- `GET /docs` from the Cloudflare Worker deployment
- `GET /health`
- `GET /v1/info`
- `POST /v1/sessions/register`
- `GET /v1/sessions/{sessionId}/verifier` as a WebSocket upgrade
- `POST /v1/sessions/{sessionId}/reveal-config`
- `POST /v1/sessions/{sessionId}/attestation`
- `GET /v1/sessions/{sessionId}/result`
- `POST /v1/artifacts/verify`

## Environment

Runtime config is loaded from the repository `.env` and then the current
working directory `.env` if present.

- `PORT`: HTTP port, default `7047`.
- `RUST_LOG`: tracing filter, default `mobile_tlsn_verifier_service=info,tlsn=warn`.
- `SESSION_STORE`: session record backend, `sqlite` by default. Set to `memory`
  for local development when you do not want a DB file. SQLite persists session
  metadata, reveal config, verifier results, and attestation material outside
  process memory.
- `SESSION_STORE_SQLITE_PATH`: SQLite database path when `SESSION_STORE=sqlite`,
  default `/tmp/mobile_tlsn_verifier_sessions.sqlite3`.
- `SESSION_TTL_SECONDS`: active session TTL before stale `pending` or
  `in_progress` sessions are marked `failed`, default `600`.
- `SESSION_RETENTION_SECONDS`: session record retention before cleanup deletes
  old rows, default `3600` and never less than `SESSION_TTL_SECONDS`.
- `VERIFIER_RUN_TIMEOUT_SECONDS`: hard cap for the TLSN verifier runtime after
  WebSocket handoff, default `SESSION_TTL_SECONDS`.
- `WEBSOCKET_HANDOFF_TIMEOUT_SECONDS`: hard cap for waiting on the prover
  WebSocket after registration, default `60`.
- `NOTARY_SIGNING_KEY_B64`: base64-encoded 32-byte secp256k1 private key. A deterministic development fallback is used when unset; production must set this. The service derives the matching public verifying key at startup for self-issued artifact verification.

SQLite storage does not make an active verifier WebSocket portable across
processes. The live WebSocket handoff and TLSN verifier task remain local to
the process that accepted registration, so container deployments still need
session-affinity routing for in-flight sessions.

On startup and after each verifier task exits, cleanup marks stale active
sessions as `failed` before deleting records older than
`SESSION_RETENTION_SECONDS`.

Artifact verification expects proof envelopes in `proof.presentation.bincode`
(standard) or `proof.presentation`.

## Plugin Integration

Set `TransportConfig.verifierUrl` in the plugin to this service. The plugin
will register a session, connect to the verifier WebSocket, submit reveal
ranges, request an attestation, poll the result, and call artifact verification.

See `docs/api_reference.md`, `docs/local_run.md`, `docs/testing_guide.md`, and
`docs/production_deployment.md` for detailed workflows.

## Cloudflare Worker + Container

Current deployment:

- Worker URL: `https://mpc-verifier.burnt.com`
- workers.dev URL: enabled in `worker/wrangler.jsonc`
- Preview URLs: enabled in `worker/wrangler.jsonc`
- Cloudflare account: `burnt` (`7b5971b399673fe613f69d1f6d9a0940`)
- Container application: `mpc-verifier-verifiercontainer`
- Container application ID: `<container-app-id>`

The custom domain was added with `routes[].custom_domain = true`. After deploy,
public resolvers returned Cloudflare A/AAAA records, but the first immediate
`curl` from this machine failed to resolve the hostname. Allow for DNS
propagation before using it as the mobile verifier endpoint.

Container runtime defaults live in the Rust service and Docker image, not in
`worker/wrangler.jsonc`. The Worker passes only the production
`NOTARY_SIGNING_KEY_B64` secret into the container.

Install Worker tooling and type-check the routing layer:

```bash
cd worker
npm install
npm run check
```

`worker/index.ts` serves `/openapi.json` and `/docs` directly from the
Worker. It also mints a UUID for `POST /v1/sessions/register`, injects it as
`sessionId`, and routes the request to `getContainer(env.VERIFIER_CONTAINER,
sessionId)`. Later `/v1/sessions/{sessionId}/...` HTTP and WebSocket requests
route by the same session ID.

GitHub Actions deploy previews on pull requests with `wrangler versions upload`
and deploy production on pushes to `main`.

Before production deploy, set the signing key as a Worker secret:

```bash
cd worker
op read "op://Engineering/<item-id>/NOTARY_SIGNING_KEY_B64" | npx wrangler secret put NOTARY_SIGNING_KEY_B64
```

Then validate or deploy:

```bash
cd worker
npx wrangler deploy --dry-run
npm run deploy
```

## License

This repository is licensed under the MIT License. See `LICENSE`.

### TLSNotary dependency licensing

This service depends on `tlsn` from `https://github.com/tlsnotary/tlsn` tag
`v0.1.0-alpha.14`, which is dual-licensed as `MIT OR Apache-2.0`.

For this repository, TLSNotary is consumed under the MIT option. See
`THIRD_PARTY_LICENSES.md` for third-party attribution and license text.
