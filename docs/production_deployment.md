# Production Deployment Guide

## Deployment Model

- Deploy `mobile_tlsn_verifier_service` as an independent service.
- Expose HTTPS/WSS via reverse proxy or load balancer.
- Keep plugin and verifier separately scalable.

## Build Release Binary

```bash
cd mobile_tlsn_verifier_service
cargo build --release
```

Binary:

- `target/release/mobile-tlsn-verifier-service`

## Environment Variables

- `PORT` — listen port (default `7047`).
- `RUST_LOG` — log filter (recommended `info` in production).
- `PUBLIC_HOST` — host used in `verifierWsUrl` registration responses (for example `verifier.example.com`).

## Container Deployment

```bash
docker build -t mobile-tlsn-verifier-service .
docker run --rm -p 7047:7047 \
  -e PORT=7047 \
  -e PUBLIC_HOST=verifier.example.com \
  -e RUST_LOG=mobile_tlsn_verifier_service=info,tlsn=warn \
  mobile-tlsn-verifier-service
```

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
