# S3.2 Close-out Design: Signed Space Capability Profile + Backend-Pinned Lease + Scratch/Snapshot Lifecycle

Status: design complete, implementation not started. Companion to
`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md` §S3.2 and
`SPACE_PAGE_AUDIT_2026-09-06.md` §14/§1059-1065.

## Why this doc exists

The adoption plan sequences the "durable Space computer" (each agent in a
Space getting a persistent, controllable sandbox — the product goal
compared against Grok's agent, OpenBot, and Warmwind's "autonomous cloud
workers") behind four milestones. The third, S3.2, has one unchecked
execution-evidence item blocking everything downstream (S3.3 durable
workspace → S4.2 process registry → S4.3 watches):

> Bind this substrate truth to a signed Space capability profile and
> backend-pinned lease; add the scratch/snapshot lifecycle before S3.2 is
> complete.

Already done (do not redo):
- Execution Core rejects a `ReadOnly`/`WorkspaceWrite` tool request when
  Bubblewrap cannot provide the requested local isolation
  (`execution-core`, 377 tests pass).
- Execution Core publishes a measured, **unsigned** `/capability-profile`:
  backend/isolation availability, `ephemeral` persistence,
  `bounded_oneshot` processes, no backup, egress-disabled-by-default,
  credential-free execution.
- Sandbox Manager refuses normal startup unless
  `SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT=true` is explicitly set,
  since its lease/snapshot stores are in-memory only.

This doc closes the remaining item. Verification bar (from the adoption
plan): backend loss/downgrade, concurrent provision, restart,
suspend/destroy, expired lease, credential scan, egress policy.

## 1. Signed Space capability profile

**Signer:** Control Plane, extending the existing generic
`spaces.Decision` / `spaces.SignDecision` envelope in
`apps/Control Plane/user-core/internal/spaces/decision.go` — the same
envelope `model.cron.create`/`model.cron.fire` already reuse (as opposed
to `RunActionDecision`/`OwnerGrantDecision`, which needed extra fields and
so duplicated the sign/verify machinery wholesale). `Decision`'s existing
fields (OrgID, SpaceRef, SubjectID, ServiceAudience, ActionID,
ActionSchemaHash, PayloadDigest, Permissions, IssuedAt/ExpiresAt,
ZeroDataRetention, Nonce) are generic enough to carry this new action
without a fork.

New file `apps/Control Plane/user-core/internal/spaces/space_capability_decision.go`,
mirroring `schedule_fire_decision.go`:

```go
const (
    spaceCapabilityAction   = "model.space.capability_profile"
    spaceCapabilityAudience = "model-plane-sandbox-manager"
    spaceCapabilitySchema   = "sha256:space-capability-profile-v1"
)

type SpaceCapabilityIntent struct {
    OrgID          string
    SpaceRef       string
    SubjectID      string // execution-core/sandbox-manager service identity
    BackendID      string // e.g. "bubblewrap-host-a1", "external:<node-id>"
    ProfileDigest  string // sha256 hex of the canonical measured SandboxCapabilityProfile JSON
    Persistence    string
    Processes      string
    Backup         bool
    Egress         string
    CredentialMode string
    IdempotencyKey string
}

func (i SpaceCapabilityIntent) Validate() error { /* same non-blank pattern as ScheduleFireIntent.Validate */ }

func expectedCapabilityPayloadDigest(i SpaceCapabilityIntent) string {
    // length-prefixed SHA-256 over every claim field, domain-separated by
    // "model.space.capability_profile\x00v1\x00" — same construction as
    // expectedFirePayloadDigest in the Model-side authorizer.
}

func IssueSpaceCapabilityDecision(key SigningKey, intent SpaceCapabilityIntent, now, expiresAt time.Time) (string, error) {
    if err := intent.Validate(); err != nil { return "", err }
    d := Decision{
        DecisionRef: newDecisionRef(), OrgID: intent.OrgID, SpaceRef: intent.SpaceRef,
        SubjectID: intent.SubjectID, ServiceAudience: spaceCapabilityAudience,
        ActionID: spaceCapabilityAction, ActionSchemaHash: spaceCapabilitySchema,
        PayloadDigest: expectedCapabilityPayloadDigest(intent),
        IdempotencyKey: intent.IdempotencyKey,
        Permissions: []string{"space:sandbox:use"}, // + "space:egress" only if intent.Egress != "disabled_by_default"
        ZeroDataRetention: false,
        IssuedAt: now, ExpiresAt: expiresAt, Nonce: newNonce(),
    }
    return SignDecision(key, d)
}
```

`BackendID` and the profile fields are not literal `Decision` fields —
they fold into `PayloadDigest`'s pre-image via `SpaceCapabilityIntent`,
the same way `validateFireDecision` recomputes
`expectedFirePayloadDigest`. This means the wire response must carry the
claims in the clear alongside the signature (a `SpaceCapabilityClaims`
JSON sidecar: `BackendID`, `ProfileDigest`, `Persistence`, `Processes`,
`Backup`, `Egress`, `CredentialMode`) so the verifier has something to
recompute the digest from — tampering with the plaintext claims
invalidates the digest check even though the claims aren't signed
directly.

**HTTP wiring:** `apps/Control Plane/user-core/internal/http/spaces.go` —
new handler `issueSpaceCapabilityDecision`, new route
`POST /v1/spaces/{space_id}/capability-decisions`. Request body:
`{backend_id, measured_profile: {...}, idempotency_key}`. The handler
validates `measured_profile` against a policy allowlist *before* signing
(e.g. a local bwrap backend must report exactly `persistence:"ephemeral"`,
`backup:false`, `credential_mode:"credential_free"`; any deviation, or
`egress` other than `"disabled_by_default"` without an org entitlement, is
rejected pre-signature) — Control decides what a claimed measurement is
*authorized to be used for*, it does not attest hardware truth. Response:
typed envelope `{data: {decision: "<token>", claims: {...}}}`.

New test `space_capability_decision_test.go`, following
`schedule_fire_decision_test.go`: valid round trip, forged `BackendID`,
forged `ProfileDigest`, wrong signing key, expired `ExpiresAt`, missing
required field.

## 2. Backend-pinned lease

**Proto** (`apps/Model Plane/proto/model_plane/v1/sandboxes.proto`):

```protobuf
message AcquireLeaseRequest {
  string scope_id = 1;
  string scope_type = 2;
  google.protobuf.Duration ttl = 3;
  string org_id = 4;
  string space_id = 5;               // NEW
  string capability_decision = 6;    // NEW — signed token from §1
  string capability_claims_json = 7; // NEW — the unsigned SpaceCapabilityClaims body
}

message AcquireLeaseResponse {
  string lease_id = 1;
  string endpoint = 2;
  google.protobuf.Timestamp expires_at = 3;
  string backend_id = 4;             // NEW — this instance's own pinned backend id
  SandboxLifecycleState state = 5;   // NEW, see §3
}

message ReleaseLeaseRequest {
  string lease_id = 1;
  string backend_id = 2;             // NEW — caller re-asserts the backend it believes it's talking to
}

message SnapshotRequest {
  string lease_id = 1;
  string label = 2;
  string backend_id = 3;             // NEW
}

enum SandboxLifecycleState { // NEW, see §3
  LIFECYCLE_UNSPECIFIED = 0;
  SCRATCH = 1;
  ACTIVE = 2;
  SNAPSHOTTING = 3;
  DESTROYED = 4;
}
```

**Go lease type** (`apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go`):

```go
type Lease struct {
    ID, ScopeID, ScopeType, OrgID, OwnerID, Endpoint string
    SpaceID   string                // NEW
    BackendID string                // NEW — this process's own configured backend id, stamped at Create
    State     SandboxLifecycleState // NEW
    ExpiresAt, CreatedAt time.Time
}
```

`Store.Create` grows to
`Create(scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*Lease, error)`,
initializing `State: StateScratch`.

**Fail-closed check:** new sentinel `ErrLeaseBackendMismatch`.
`GetScoped`/`ReleaseScoped` (currently comparing `OrgID`/`OwnerID`) gain
the same comparison for `BackendID` — any `ReleaseLease`/`SnapshotSandbox`
call whose `backend_id` doesn't match the stored lease's `BackendID`
returns `ErrLeaseBackendMismatch` before any other logic runs, mapped to
`codes.FailedPrecondition`.

New file `apps/Model Plane/go/services/sandbox-manager/internal/authz/capability_verifier.go`:

```go
type SpaceCapabilityVerifier struct {
    keyID  string
    public ed25519.PublicKey
}
func LoadSpaceCapabilityVerifierFromEnv(getenv func(string) string) (*SpaceCapabilityVerifier, error)
func (v *SpaceCapabilityVerifier) Verify(token, claimsJSON string, expect CapabilityExpectation) (SpaceCapabilityClaims, error)
```

Mirrors `ControlDecisionVerifier.verify`
(`capability-core/internal/cron/control_authorizer.go:275-297`): split
`v2.<keyID>.<payload>.<sig>`, check key ID, verify with the Ed25519 stdlib, unmarshal
`Decision`, recompute `expectedCapabilityPayloadDigest` from
`claimsJSON` and compare to `Decision.PayloadDigest`, check
`ActionID == "model.space.capability_profile"`, `OrgID/SpaceRef/SubjectID`
match the request, `now.Before(ExpiresAt)`.

`AcquireLease`'s handler: extract `space_id`/`capability_decision`/
`capability_claims_json`, verify, then **reject if the verified
`claims.BackendID` != this instance's own `SANDBOX_MANAGER_BACKEND_ID`**
(new env var, read once at startup). This is the literal "fail closed on
an incompatible route" check — a lease request that lands on a different
sandbox-manager instance than the one the capability decision was pinned
to is refused, not silently served by whichever instance answered. Only
on match does `Store.Create(..., spaceID, thisInstanceBackendID, ttl)`
run.

**Open design fork (not resolved by research, flagged for the `spaces`
package owner):** carrying `BackendID`/profile facts as an
unsigned-but-digest-bound sidecar (chosen here, cheaper, same digest
strength as `model.cron.fire`) vs. a bespoke `SpaceCapabilityDecision`
struct duplicating sign/verify machinery like `RunActionDecision` does
(heavier, matches the "extra fields" precedent). This plan picks
envelope-reuse; revisit if the `spaces` package owner disagrees.

## 3. Scratch/snapshot lifecycle

States: `SCRATCH → ACTIVE → SNAPSHOTTING → DESTROYED`, with
`SNAPSHOTTING` returning to `ACTIVE` on success.

- **`SCRATCH`** (initial state on `AcquireLease`): credential-free by
  construction — this is execution-core's existing
  `SandboxEnv::Only(allowlist)` + `--clearenv`/`--setenv` mechanism
  (`sandbox.rs:74-87`, `172-181`, already implemented and tested), which
  today is unconditional; it becomes the *enforced* behavior while the
  lease is in `SCRATCH`. `SnapshotSandbox` on a `SCRATCH` lease returns
  `codes.FailedPrecondition` — nothing durable exists yet by definition.
- **`ACTIVE`**: new RPC `ActivateLease(lease_id, backend_id) returns (state)`.
  Handler checks the same backend-pin as `AcquireLease`, then flips
  `Lease.State`. Transition happens the first time a caller needs more
  than the credential-free scratch allowlist (e.g. real backend
  network/egress permission per `Decision.Permissions`).
- **`SNAPSHOTTING`**: `SnapshotSandbox` sets state to `SNAPSHOTTING`
  before reading/writing, back to `ACTIVE` after (or on failure — never
  left stuck).
- **`DESTROYED`**: `ReleaseLease` sets `State = DESTROYED` before removal
  (kept briefly for idempotent double-release detection). Any further
  `SnapshotSandbox`/`ActivateLease` against a `DESTROYED` or absent lease
  fails closed via the existing `ErrLeaseNotFound`/`ErrLeaseExpired`.

**Snapshot exclusion** — new file
`apps/Model Plane/go/services/sandbox-manager/internal/snapshot/exclude.go`:

```go
func ExcludeCredentials(files map[string][]byte) map[string][]byte
```

Two exclusions, ported from execution-core's already-proven patterns
rather than invented:

1. **Path exclusion** — any file whose key has a scratch-only prefix
   (`scratch/`, `/tmp/`) is dropped before it's added to the snapshot
   payload.
2. **Content redaction** — for every remaining file, run the same regex
   family as `execution-core/src/scrub.rs:164-233` (Bearer/JWT, Slack
   `xoxb-`, Google `AIzaSy`, DSN passwords, generic API-key shapes),
   ported to Go, replacing matches in-place before writing to MinIO.

`SnapshotSandbox`'s backing code calls `ExcludeCredentials` immediately
before the MinIO `PutObject` call — never after.

## 4. File-by-file change list

**Control Plane** (`apps/Control Plane/user-core/`):
- NEW `internal/spaces/space_capability_decision.go`
- NEW `internal/spaces/space_capability_decision_test.go`
- MODIFY `internal/http/spaces.go` — handler + route registration

**Model Plane — proto:**
- MODIFY `apps/Model Plane/proto/model_plane/v1/sandboxes.proto`
- Regenerate Go stubs via the existing protoc/buf generation step

**Model Plane — execution-core (Rust):**
- MODIFY `src/sandbox.rs` — add `capability_profile_digest(&SandboxCapabilityProfile) -> String`
- NEW `src/capability_client.rs` — HTTP client to Control's new endpoint
- MODIFY `src/http_health.rs` — `/capability-profile` wraps response as
  `{profile, backend_id, decision: Option<String>}`; `decision` is `None`
  whenever no Control client is configured or backend is `"unavailable"`
  (never fabricate a signature)
- MODIFY existing test at `http_health.rs:105-112` + add
  `decision_is_none_without_control_client`

**Model Plane — sandbox-manager (Go):**
- MODIFY `internal/lease/lease.go`
- NEW `internal/authz/capability_verifier.go`
- MODIFY `internal/server/server.go` — `AcquireLease` verification +
  pinning; `ReleaseLease`/`SnapshotSandbox` backend check; new
  `ActivateLease` handler
- NEW `internal/snapshot/exclude.go`
- MODIFY `internal/snapshot/snapshot.go`
- MODIFY `cmd/main.go` — read `SANDBOX_MANAGER_BACKEND_ID` + Control
  public key env vars at startup; fail closed if a capability verifier
  can't be constructed and ephemeral-dev isn't explicitly opted in

## 5. Test plan (the 7 verification scenarios)

| Scenario | File | Assertion |
|---|---|---|
| Backend loss/downgrade | extend `sandbox.rs` near line 372 + new `capability_profile_reports_unavailable_when_bwrap_probe_fails`; new `server_test.go` `TestAcquireLeaseFailsClosedWhenClaimedBackendMismatchesInstance` | `backend=="unavailable"` never accompanied by a `decision` token |
| Concurrent provision | `lease_test.go` new `TestStoreConcurrentCreateIsRaceFree` (`go test -race`) | unique lease IDs, no data race, no lost `ExpiresAt` |
| Restart | `lease_test.go` new `TestFreshStoreRejectsPreRestartLeaseID` | stale lease ID from a discarded `Store` returns `ErrLeaseNotFound` from a new `Store` (fail-closed, **not** durability — see §6) |
| Suspend/destroy | new `internal/server/lifecycle_test.go`: `TestReleaseLeaseTransitionsStateToDestroyed`, `TestSnapshotAfterDestroyIsRejected` | destroyed lease rejects further snapshot/activate |
| Expired lease | extend `lease_test.go:52 TestStoreReportsExpiredAndIDGenerationErrors` | expiry check runs before the backend check |
| Credential scan | new `internal/snapshot/exclude_test.go` | ported fixtures from `scrub.rs:164-233`; scratch-prefixed keys absent entirely |
| Egress policy | new `server_test.go` `TestAcquireLeaseRejectsEgressCapableClaimsWithoutEntitlement` | claimed egress beyond default without `"space:egress"` permission is rejected, mirroring `hasPermission(...,"cron:fire")` |

Convention: table-driven `t.Run` subtests, hand-built fakes (an injectable
`capabilityVerify func(...)` field on `Server`, mirroring the existing
`principal func(...)` in `server_test.go:62-66`), `bufconn` for gRPC
round-trips — no mocking library.

## 6. Sequencing

1. **Proto/schema first** — fields + enum + `ActivateLease` RPC,
   regenerate stubs. No behavior change; existing handlers ignore new
   fields; existing tests pass unchanged.
2. **Control Plane signing** — new decision file + test + HTTP endpoint.
   Fully testable in isolation (no consumer yet); mirrors
   `schedule_fire_decision.go` closely, so review cost is low.
3. **execution-core measurement→signing plumbing** — digest fn, HTTP
   client, `/capability-profile` wrapper, tested against a fake Control
   endpoint (same isolation pattern as `control_authorizer_test.go`).
4. **sandbox-manager verification + backend pinning** — depends on 1
   (proto) and 2 (token format/public key); testable with a
   locally-generated ed25519 keypair, no live Control Plane needed.
5. **Lifecycle + snapshot exclusion** — depends on 4 for `Lease.State`/
   backend plumbing to exist.
6. **End-to-end wiring + the 7 verification-bar tests** — point
   execution-core's real startup at Control's real endpoint, land the
   cross-cutting tests last since several exercise the full chain.

## Open questions (flagged, not resolved)

- Control's signature attests "this measurement is authorized for this
  Space," not independent hardware truth — it trusts execution-core's
  self-report over an authenticated channel. True remote attestation
  doesn't exist in this repo; this plan treats that as an accepted trust
  boundary.
- "Restart" survivability here is fail-closed only (stale lease IDs are
  cleanly rejected), not durable. Real durability needs a Postgres-backed
  store — `capability-core/internal/registry/scope_store.go` is the
  template to copy (same shape: org/agent-scoped rows, `pgxpool.Pool` via
  a narrow interface). `sandbox-manager/cmd/main.go` already flags this
  gap; this plan does not close it. **This is the natural next slice
  after S3.2, and the actual prerequisite for the "durable Space
  computer" work the user wants to build next.**
- Envelope-reuse (`Decision` + sidecar claims) vs. a bespoke signed struct
  for `BackendID`/profile facts is a real design fork the research
  surfaced but didn't resolve — see the open question in §2.

## Relationship to the "own computer per agent" feature

This closes S3.2 only. S3.3 (durable workspace with CAS-based run
overlays, org read-only + Space writable + per-run overlay, "never
last-writer-wins", output promoted through Data APIs) is the next
milestone and is where the actual persistent workspace a human or agent
can return to gets built — informed by the openbot architecture review
(disk-level persistence via named volumes, not VM-memory snapshotting;
ARIA-ref-based browser control; a decoupled resolve→decide→audit→execute
pipeline this repo already has in execution-core). That comparison and
the concrete new-build/adapt-existing breakdown for the durable-computer
feature itself lives in the conversation that produced this doc — write
it up as a follow-on design doc once S3.2 lands, since S3.3 depends on
S3.2's backend-pinned lease and the Postgres-backed durable store noted
above.
