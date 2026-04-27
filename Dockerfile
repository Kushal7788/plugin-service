FROM rust:1.84 AS builder

WORKDIR /workspace
COPY mobile_tlsn_verifier_service ./mobile_tlsn_verifier_service
COPY code/tlsn ./code/tlsn

WORKDIR /workspace/mobile_tlsn_verifier_service
RUN cargo build --release --bin mobile-tlsn-verifier-service

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /workspace/mobile_tlsn_verifier_service/target/release/mobile-tlsn-verifier-service /app/mobile-tlsn-verifier-service

EXPOSE 7047
CMD ["/app/mobile-tlsn-verifier-service"]
