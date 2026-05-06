# Local Run Guide

## Prerequisites

- Rust toolchain (`rustup`, stable channel).
- Build dependencies for the pinned TLSN Git dependency.

## Start Service

```bash
cd mobile_tlsn_verifier_service
RUST_LOG=mobile_tlsn_verifier_service=info,tlsn=warn cargo run
```

Default bind: `0.0.0.0:7047`.

## Sanity Checks

```bash
curl http://localhost:7047/health
curl http://localhost:7047/v1/info
```

## Plugin Configuration (Local)

Use the service URL in app/plugin transport config:

- Android emulator: `http://10.0.2.2:7047`
- iOS simulator: `http://127.0.0.1:7047`
- Physical devices: `http://<your-lan-ip>:7047`

## Manual Session Flow Test

```bash
curl -X POST "http://localhost:7047/v1/sessions/register" \
  -H "Content-Type: application/json" \
  -d '{
    "clientSessionId":"debug-session-001",
    "maxRecvData":16384,
    "maxSentData":4096,
    "sessionData":{
      "allowedRequestHeaders":"[\"accept\"]",
      "allowedResponseBodyJsonPaths":"[\"user.name\"]"
    }
  }'
```

Use `verifierWsUrl` from response for the prover WebSocket side, then query:

```bash
curl "http://localhost:7047/v1/sessions/<session-id>/result"
```
