# Deployment Guide

The service is a single stateless-at-boot binary that needs one secret (`NOTARY_SIGNING_KEY_B64`) and listens on `PORT` (default `7047`). This guide covers a plain binary, Docker, and the included Cloudflare Workers + Containers setup. All environment variables are listed in the [README](../README.md#configure) and in `.env.example`.

## Requirement for every deployment: session affinity

A session's prover WebSocket and its TLSN verifier task live in the memory of the process that handled `POST /v1/sessions/register`. Every later request for that session (`/verifier`, `/reveal-config`, `/attestation`, `/result`) must reach the **same process**. SQLite persistence does not change this. Do not load-balance WebSocket upgrades without session affinity.

## Release binary

```bash
cargo build --release
NOTARY_SIGNING_KEY_B64=... ./target/release/mobile-tlsn-verifier-service
```

## Docker

The `Dockerfile` is a two-stage build: `rust:1.90` compiles `mobile-tlsn-verifier-service` in release mode, and `debian:bookworm-slim` (plus `ca-certificates`) runs it. The image sets defaults for `PORT`, `RUST_LOG`, the SQLite store and the timeouts, but **not** the signing key.

```bash
docker build --platform linux/amd64 -t mobile-tlsn-verifier-service .
docker run --rm -p 7047:7047 \
  -e NOTARY_SIGNING_KEY_B64="$NOTARY_SIGNING_KEY_B64" \
  mobile-tlsn-verifier-service
```

Or with Compose, which reads `NOTARY_SIGNING_KEY_B64` from the shell or from `.env`:

```bash
docker compose up --build
```

The SQLite file defaults to `/tmp/mobile_tlsn_verifier_sessions.sqlite3` inside the container. Mount a volume and set `SESSION_STORE_SQLITE_PATH` if records should survive container replacement.

## Reverse proxy

- Route both HTTP endpoints and WebSocket upgrades to the service, preserving the `Upgrade` and `Connection` headers.
- Use long read timeouts on the verifier WebSocket; an MPC session can run up to `VERIFIER_RUN_TIMEOUT_SECONDS` (default 600 s).
- Terminate TLS at the proxy so clients use `https://` and `wss://`.
- Add authentication, rate limiting and a CORS allowlist at this layer. The service itself allows all origins and has no authentication.

## Cloudflare Workers + Containers

The `worker/` directory deploys the Rust image as a Cloudflare Container behind a Worker that provides session affinity.

| File | Role |
| --- | --- |
| `worker/index.ts` | Serves `/openapi.json` and `/docs`; mints a UUID for registration and routes it to a container named by that ID; routes `/v1/sessions/{id}/...` to the same named container; routes everything else to a `control` container |
| `worker/wrangler.jsonc` | Worker `mpc-verifier`, `VerifierContainer` class built from `../Dockerfile`, `max_instances: 20`, `rollout_active_grace_period: 900`, Durable Object binding `VERIFIER_CONTAINER`, required secret `NOTARY_SIGNING_KEY_B64`, custom-domain route |
| `worker/package.json` | `npm run check` (tsc), `types` (wrangler types), `dev`, `deploy` |

`VerifierContainer` sets `defaultPort = 7047` and `sleepAfter = "15m"` and passes only `NOTARY_SIGNING_KEY_B64` into the container. Other runtime settings come from the image defaults. Keep `sleepAfter` and `rollout_active_grace_period` longer than the longest expected session, so idle shutdowns and rolling deploys do not kill in-flight MPC sessions.

`wrangler.jsonc` is configured for a specific Cloudflare account (`account_id`) and custom domain (`routes`). Change both before deploying to a different account.

### Deploy steps

```bash
cd worker
npm install
npm run types
npm run check
npx wrangler secret put NOTARY_SIGNING_KEY_B64   # paste the base64 key when prompted
npx wrangler deploy --dry-run
npm run deploy
```

Do not configure the public verifying key separately: the service derives it from the signing key at startup.

### CI/CD

| Workflow | Trigger | Steps |
| --- | --- | --- |
| `.github/workflows/preview.yml` | Pull requests to `main` | `npm ci`, `npm run types`, `npm run check`, `cargo test --locked`, then `wrangler versions upload --preview-alias pr-<number>` and a PR comment with the preview URL |
| `.github/workflows/deploy.yml` | Push to `main`, manual dispatch | Same checks, then `wrangler deploy` |

Both workflows expect the repository secrets `BURNT_CLOUDFLARE_API_TOKEN` and `BURNT_CLOUDFLARE_ACCOUNT_ID`. Cloudflare preview URLs have documented limitations for Workers that use Durable Objects, so a preview upload may not produce a working URL.

## Readiness checklist

- `GET /health` wired into liveness probes.
- Alerting on spikes in `failed` sessions (visible in logs as `session=<id> verifier failed` and in `/result`).
- `NOTARY_SIGNING_KEY_B64` stored in a secret manager, and rotation planned with care: rotating it invalidates verification of artifacts signed with the old key.
- Mobile plugin endpoints tested with production `https://` / `wss://` URLs on Android and iOS.
