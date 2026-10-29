# mp-events (Python)

Canonical event envelope for the Model Plane — Python parity leg.

Mirrors the Go (`pkg/envelope`) and Rust (`crates/mp-events`) implementations.
Provides:

- `Envelope` — pydantic v2 model with 12 canonical fields.
- `derive_idempotency_hash(producer, event_type, resource_ref, idempotency_key)` —
  blake3 hex digest of `"|"`-joined UTF-8 string. Must match Go/Rust output byte-for-byte.

## Golden

Input: `("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1")`
Output: `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`
