# Identifiers — Phase 0 Freeze

**Source of truth:** [`rust/crates/mp-ids/src/lib.rs`](../../rust/crates/mp-ids/src/lib.rs)
**Wire contract:** [`proto/model_plane/v1/ids.proto`](../../proto/model_plane/v1/ids.proto)

## 1. Format

All canonical Model Plane identifiers are **ULIDs** — 26-character Crockford Base32 strings (RFC draft ulid).

- Lexicographically sortable by creation time (millisecond-precision prefix).
- Case-insensitive; canonical form is uppercase.
- 128 bits total (48 bits timestamp + 80 bits randomness).
- Validation: `ulid::Ulid::from_string` — any 26-char Crockford Base32 string that round-trips.

Example: `01HZ3N4KAG8RW2V9XQY5P1B7TM`

## 2. Typed newtypes

Every ID kind is a distinct Rust newtype around `String`. Types are **not** interchangeable at
compile time even when the underlying ULID string is identical. Each type provides:

- `::generate()` — returns a freshly minted random ULID.
- `TryFrom<String>` / `TryFrom<&str>` — validates and wraps.
- `Display`, `AsRef<str>`, `as_str()`, `into_inner()`.
- `Serialize` / `Deserialize` as transparent strings.

## 3. Canonical ID catalogue

| Type | Owner service | Lifetime | Used in subjects / payloads |
|------|---------------|----------|-----------------------------|
| `AgentId` | capability-core | persistent | agent definitions, run.agent_id |
| `ThreadId` | session-core | persistent | conversation threads |
| `SessionKey` | session-core | session-scoped | `mp.v1.session.{session_key}.command` |
| `RunId` | session-core | run-scoped | `mp.v1.run.{run_id}.event` |
| `StepId` | execution-core | run-scoped | step records, span IDs |
| `CheckpointId` | session-core | persistent | resumable snapshots |
| `SandboxLeaseId` | sandbox-manager | lease-scoped | active sandbox leases |
| `BrowserLeaseId` | browser-broker | lease-scoped | active browser grants |

## 4. Non-ULID identifiers

The following identifiers are **not** minted as ULIDs and are outside the `mp-ids` typed set:

- `org_id` — tenant ID, opaque string (supplied by control plane).
- `user_id` — user ID, opaque string (supplied by auth).
- `trace_id` / `span_id` — W3C Trace Context (16 / 8 bytes hex).
- `idempotency_key` — client-supplied, arbitrary string ≤ 256 bytes.

These appear in envelopes and RPC metadata but are not validated by `mp-ids`.

## 5. Invariants

1. A given ULID string MAY be reused across different ID types; semantic distinction comes from
   the typed newtype, not from the value.
2. Services MUST validate incoming IDs via `TryFrom` before trusting them.
3. Generated IDs MUST use `::generate()` (or an equivalent ULID source) — never hand-constructed.
4. IDs are immutable once assigned; renaming or re-minting is a new resource.

## 6. Breaking-change policy

- Adding a new ID newtype: **non-breaking**.
- Changing the underlying format of an existing type (e.g. ULID → UUIDv7): **breaking**, requires
  v2 proto + coordinated migration.
- Changing ownership of an ID type across services: requires ADR; the string format stays.

## 7. Review checklist

- [x] Every identifier used on NATS, in Postgres, or on gRPC is listed above.
- [x] No service defines ad-hoc ID types outside `mp-ids`.
- [x] `ids.proto` messages use `string` fields named identically to the Rust types (snake_case).
