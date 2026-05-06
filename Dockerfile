FROM rust:1.90 AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin mobile-tlsn-verifier-service

FROM debian:bookworm-slim

ENV PORT=7047 \
    RUST_LOG=mobile_tlsn_verifier_service=info,tlsn=warn \
    SESSION_STORE=sqlite \
    SESSION_STORE_SQLITE_PATH=/tmp/mobile_tlsn_verifier_sessions.sqlite3 \
    SESSION_TTL_SECONDS=600 \
    SESSION_RETENTION_SECONDS=3600 \
    VERIFIER_RUN_TIMEOUT_SECONDS=600 \
    WEBSOCKET_HANDOFF_TIMEOUT_SECONDS=60

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /workspace/target/release/mobile-tlsn-verifier-service /app/mobile-tlsn-verifier-service

EXPOSE 7047
CMD ["/app/mobile-tlsn-verifier-service"]
