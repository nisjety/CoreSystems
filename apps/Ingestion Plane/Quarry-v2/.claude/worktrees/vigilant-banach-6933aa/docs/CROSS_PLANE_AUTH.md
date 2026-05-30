# Cross-Plane Auth — HMAC + Idempotency

D2 + D3 / cluster #14.

## TL;DR

| Layer                          | Auth | Replay defence  | Idempotency        |
| ------------------------------ | ----- | ---------------- | ------------------- |
| **client → quarry-edge**       | JWT (auth-core JWKS, P0) | n/a (request-fresh) | not yet |
| **quarry-edge → quarry-control** | **HMAC-SHA256** (this doc) | timestamp + nonce  | `Idempotency-Key` header |
| **quarry-edge → data-plane** (HTTP / gRPC) | API key (TLS) | n/a | future cycle |
| **quarry-edge → model-plane** | Bearer | n/a | future cycle |

## Wire shape

Every edge → control request carries three headers when
`QUARRY_EDGE__INTERNAL_SECRET` is configured:

```
X-Quarry-Sig:        sig_v1=<base64(HMAC-SHA256(secret, canonical))>
X-Quarry-Sig-TS:     <unix seconds, decimal>
X-Quarry-Sig-Nonce:  <hex 128-bit random>
```

Canonical string (newline-separated, positional):

```
<METHOD>\n<path?query>\n<sha256_hex(body)>\n<timestamp>\n<nonce>
```

- `path?query` is `r.URL.Path + "?" + r.URL.RawQuery` (Go) or the
  reconstructed query string (Rust). Empty query → no `?`.
- `sha256_hex(body)` is the lowercase-hex digest of the *exact* bytes
  on the wire. Empty body → `e3b0c4...b855`.
- `timestamp` is unix seconds (decimal). Servers reject ±5 min skew.
- `nonce` is 128-bit hex. Servers dedup within the skew window.

## Reference implementations

| Code path | Location                                                      |
| --------- | ------------------------------------------------------------- |
| Rust signer | `crates/quarry-edge/src/internal_auth.rs`                     |
| Go verifier | `services/quarry-control/internal/httpx/hmac.go`             |
| Round-trip test | `..._test.go` (signs in Go test, verifies in Go middleware) |

## Rollout modes

Control plane reads two env vars:

| `QUARRY_INTERNAL_SECRET`         | `QUARRY_INTERNAL_HMAC_REQUIRED` | Behavior                                                  |
| --------------------------------- | -------------------------------- | --------------------------------------------------------- |
| _(unset)_                         | _(any)_                          | **Trust the network** — no verification. Private only.    |
| set                               | _(unset / `0`)_                  | **Rollout** — verify when present, allow when absent      |
| set                               | `1`                              | **Enforce** — reject every unsigned request 401           |

Operational rollout sequence:
1. Set the same `QUARRY_INTERNAL_SECRET` on edge + control (matched 32+ char hex).
2. Edge starts signing automatically.
3. After every edge instance is rolled, set `QUARRY_INTERNAL_HMAC_REQUIRED=1` on control.

## Threat model coverage

| Threat                                                        | Defence                       |
| ------------------------------------------------------------- | ----------------------------- |
| Spoofed edge → control (e.g. DNS poisoning inside private net) | HMAC sig — attacker lacks secret |
| Tampered body in flight                                       | body sha256 baked into sig    |
| Captured + replayed valid request                              | timestamp skew + nonce dedup  |
| `alg=none` / algorithm confusion                                | algorithm pinned via `sig_v1=` |
| Algorithm rotation                                              | `sig_v1=` prefix → future `sig_v2=` |

HMAC does NOT defeat eavesdropping — TLS does that. The two layers
are complementary.

## Idempotency keys (D3)

Every mutating call from edge → control carries:

```
Idempotency-Key: <ulid>
```

The Rust edge sets this automatically in `schedule_routes::forward_json`
for every `POST` / `DELETE` so a transient 5xx retry can't produce a
duplicate operation.

Control plane storage:

```sql
CREATE TABLE quarry_idempotency_keys (
    org_id            TEXT         NOT NULL,
    idempotency_key   TEXT         NOT NULL,
    route             TEXT         NOT NULL,
    request_hash      TEXT         NOT NULL,
    response_status   SMALLINT,
    response_body     BYTEA,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at        TIMESTAMPTZ  NOT NULL,
    PRIMARY KEY (org_id, idempotency_key, route)
);
```

- 24-hour TTL on each entry.
- `(org_id, idempotency_key, route)` is the unique key — same key
  reused on a DIFFERENT route doesn't collide.
- `request_hash` lets the control plane detect a buggy client that
  re-uses the same key with a different body → 409 Conflict.

Current status: migration shipped (`005_cycle23.sql`); enforcement
logic lands in cycle 24 alongside the Sources store.

## What's still pending

- **`QUARRY_INTERNAL_HMAC_REQUIRED=1` enforcement in production** —
  staging environments should flip this once monitoring confirms
  every edge is signing.
- **Idempotency-Key enforcement in handlers** — middleware is
  scaffolded (`resources::IdempotencyKeyHandler`) but the per-route
  dedup logic uses the migration's table only in cycle 24.
- **Live Temporal integration** — `internal/temporal/client.go`
  ships the trait and `NoopClient`; `SDKClient` integration is
  cycle 24's lift.
- **End-to-end integration tests against live Temporal** — see
  `docs/INTEGRATION_TESTS.md` (D6) for the harness pattern.
