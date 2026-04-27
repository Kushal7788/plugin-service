# Verifier Service Testing Guide

## Local Checks

```bash
cd mobile_tlsn_verifier_service
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

## What Tests Cover

- Proof envelope parsing and presentation decode errors.
- Reveal config policy enforcement.
- Sanitized payload shape validation.
- Module-level validation logic without requiring a full TLSN MPC session.

## Manual Integration Test

1. Start the service:

```bash
cargo run
```

2. Confirm health:

```bash
curl http://localhost:7047/health
```

3. Run the Flutter sample app and set `TLSN_VERIFIER_URL` to the service URL.

Full TLSN verifier coverage requires the mobile plugin prover flow because the
WebSocket endpoint expects TLSN protocol frames rather than ordinary JSON.
