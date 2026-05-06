# Production Deployment Guide

## Deployment Model

- Deploy `mobile_tlsn_verifier_service` as an independent service.
- Expose HTTPS/WSS via reverse proxy or load balancer.
- Keep plugin and verifier separately scalable.

## Build Release Binary

```bash
cargo build --release
```

Binary:

- `target/release/mobile-tlsn-verifier-service`

## Environment Variables

- `PORT` — listen port (default `7047`).
- `RUST_LOG` — log filter (recommended `info` in production).
- `SESSION_STORE` — session record backend; defaults to `sqlite`. Set to `memory` only for local development when you do not want a DB file.
- `SESSION_STORE_SQLITE_PATH` — SQLite path when `SESSION_STORE=sqlite`; defaults to `/tmp/mobile_tlsn_verifier_sessions.sqlite3`.
- `SESSION_TTL_SECONDS` — active session TTL before stale `pending` or `in_progress` sessions are marked `failed` (default `600`).
- `SESSION_RETENTION_SECONDS` — session record retention before deletion (default `3600`; clamped to at least `SESSION_TTL_SECONDS`).
- `VERIFIER_RUN_TIMEOUT_SECONDS` — hard cap for TLSN verifier runtime after WebSocket handoff (default `SESSION_TTL_SECONDS`).
- `WEBSOCKET_HANDOFF_TIMEOUT_SECONDS` — hard cap for waiting on the prover WebSocket after registration (default `60`).
- `NOTARY_SIGNING_KEY_B64` — base64-encoded 32-byte secp256k1 private key. The service derives the matching public verifying key at startup.

## Container Deployment

```bash
docker build --platform linux/amd64 -t mobile-tlsn-verifier-service .
docker run --rm -p 7047:7047 \
  -e PORT=7047 \
  mobile-tlsn-verifier-service
```

The `PORT` override is shown only to make the exposed local port explicit; the
Docker image already defaults to `7047`.

For Cloudflare Containers, SQLite avoids depending on Rust process memory for
session metadata/results, but container disk is still instance-local. Keep
session-affinity routing for active sessions because the verifier WebSocket and
TLSN task cannot be resumed in another process.

The Docker image sets the default container environment for port, logging,
SQLite session storage, TTLs, and verifier timeouts. These values can still be
overridden by the runtime. `worker/wrangler.jsonc` intentionally does not
duplicate those non-secret defaults; the Worker only injects
`NOTARY_SIGNING_KEY_B64`.

### Cloudflare Containers

This repo includes basic Worker wiring:

- `worker/wrangler.jsonc` configures the Worker, `VerifierContainer`, Durable
  Object binding, SQLite-backed Durable Object migration, rollout grace, and the
  `burnt` account ID.
- `worker/index.ts` routes registration, HTTP session routes, and WebSocket
  upgrades to named container instances.
- `worker/package.json` provides `npm run check`, `npm run types`,
  `npm run dev`, and `npm run deploy`.

Current deployed resources:

- Worker: `mpc-verifier`
- Worker URL: `https://mpc-verifier.burnt.com`
- workers.dev URL: enabled in `worker/wrangler.jsonc`
- Preview URLs: enabled in `worker/wrangler.jsonc`
- Cloudflare account: `burnt` (`7b5971b399673fe613f69d1f6d9a0940`)
- Current version ID: `<version-id>`
- Container application: `mpc-verifier-verifiercontainer`
- Container application ID: `<container-app-id>`
- 1Password signing key item: `op://Engineering/<item-id>/NOTARY_SIGNING_KEY_B64`

Route every request for a logical verifier session to the same named container
instance with `getContainer(env.VERIFIER_CONTAINER, sessionId)`. The Worker
should extract the `sessionId` from `/v1/sessions/{sessionId}/...` paths and
mint a UUID for registration requests that do not already include one. The
service accepts an optional registration `sessionId`, so the Worker can choose
the named container before forwarding the request.

Keep the container awake for longer than the service's active-session TTL. For
the default 600 second TTL, `sleepAfter = "15m"` leaves a buffer for WebSocket
setup and result polling. Set `rollout_active_grace_period` to at least the max
active session duration so rolling deploys do not eagerly replace a verifier
container that is still running an in-flight WebSocket session.

Install and verify the Worker layer:

```bash
cd worker
npm install
npm run types
npm run check
npx wrangler deploy --dry-run
```

Set the production signing key with Wrangler before deployment. The public
verifying key is derived by the Rust service; do not configure it separately.

```bash
cd worker
op read "op://Engineering/<item-id>/NOTARY_SIGNING_KEY_B64" | npx wrangler secret put NOTARY_SIGNING_KEY_B64
```

Deploy:

```bash
cd worker
npm run deploy
```

GitHub Actions are configured for the deployment path:

- `.github/workflows/preview.yml` runs on pull requests to `main`, type-checks
  the Worker, runs Rust tests, and uploads a preview Worker version with a
  `pr-<number>` preview alias.
- `.github/workflows/deploy.yml` runs on pushes to `main`, repeats the checks,
  and deploys production to `https://mpc-verifier.burnt.com`.

The workflows use `BURNT_CLOUDFLARE_API_TOKEN` and
`BURNT_CLOUDFLARE_ACCOUNT_ID`, matching the existing Burnt GitHub Actions
secret naming convention.

Cloudflare's current Preview URL docs note a limitation for Workers that
implement Durable Objects. The PR workflow still uploads a version with a
`pr-<number>` preview alias and records a preview URL when Wrangler returns one.

The custom domain uses `routes[].custom_domain = true`, so Cloudflare manages
the DNS record and certificate. After the route deploy, public resolvers
returned Cloudflare A/AAAA records, but the first immediate `curl` from this
machine failed to resolve the hostname. Allow for DNS propagation before using
this URL as a mobile verifier endpoint.

Do not route WebSocket upgrade requests by load balancing alone.

## Reverse Proxy Requirements

- Route HTTP endpoints and WebSocket upgrades to backend service.
- Enable long read timeout for MPC sessions.
- Preserve `Upgrade` and `Connection` headers.

## Security Recommendations

- Terminate TLS at gateway/proxy (`https` + `wss` externally).
- Restrict CORS to approved mobile app origins where relevant.
- Run service in private network, expose only gateway endpoint.
- Centralize logs and monitor for failed verifier sessions.

## Readiness Checklist

- Health endpoint integrated into orchestration probes.
- Horizontal autoscaling/load testing completed.
- Alerting for `failed` session status spikes.
- Plugin endpoint configuration tested for Android + iOS production builds.
