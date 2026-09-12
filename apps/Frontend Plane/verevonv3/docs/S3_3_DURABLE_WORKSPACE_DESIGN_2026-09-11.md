# S3.3 Durable Workspace Design: CAS-Based Run Overlays

## Why this doc exists

`S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md` closed the security-critical
prerequisite (signed Space capability + backend-pinned lease + scratch/snapshot
lifecycle) and named its own successor in one sentence:

> "S3.3 (durable workspace with CAS-based run overlays, org read-only + Space
> writable + per-run overlay, "never last-writer-wins", output promoted
> through Data APIs) is the next milestone and is where the actual persistent
> workspace a human or agent can return to gets built ... That comparison and
> the concrete new-build/adapt-existing breakdown for the durable-computer
> feature itself lives in the conversation that produced this doc — write it
> up as a follow-on design doc once S3.2 lands."

This is that follow-on doc. S3.2 landed (`a7b4add5`..`00f17a1a`, 2026-09-11):
Control Plane signs a Space capability decision, execution-core requests and
reports it, sandbox-manager verifies and pins a lease to one backend, and a
lease's SCRATCH → ACTIVE → SNAPSHOTTING → DESTROYED lifecycle is enforced. What
S3.2 explicitly did **not** build — flagged in its own Open Questions — is
durability: both `lease.Store` and `snapshot.Store` are `map + sync.RWMutex`,
`SnapshotSandbox` synthesizes a `snapshots/<lease>/<id>` object key and uploads
nothing, and no MinIO/S3 client exists anywhere in this codebase despite the
env vars being wired in `docker-compose.yml`. That's the gap this doc closes.

**A correction on provenance, made honestly rather than silently carried
forward.** The S3.2 doc's citation of "the openbot architecture review" had no
written artifact anywhere in this repo — not in `docs/external-ideas-harvest.md`
(which never mentions OpenBot at all, in the license matrix or anywhere else),
not in `docs/core-research/`. It existed only in an unlogged prior
conversation. Before writing this doc I re-verified the claims directly
against `github.com/CopilotKit/OpenBot` (MIT license, confirmed via the GitHub
API on 2026-09-11; README fetched and quoted below) rather than propagate an
unlogged, unverifiable citation:

- **Disk-level persistence via named volumes** — confirmed. OpenBot's own
  docs: *"Nothing is deleted: the database, the Bots' files and their browser
  profiles are volumes"* and *"the supervisor gives each Bot its own
  container, its own `/workspace` volume and its own browser profile."*
- **ARIA-ref-based browser control** — confirmed, via a semantic
  `element.*` policy namespace (*"rules can inspect `tool.name`, `intent`,
  `bot.id`, `actor.id`, `page.url`, `page.host`, `element.*`"*), not raw pixel
  coordinates.
- **A resolve→decide→audit→execute pipeline** — confirmed, and it's their
  own phrase for it: *"The gateway is the only way in: it resolves the target
  from a server-held snapshot, evaluates the policy, writes the audit row, and
  only then calls the computer."*

**What is NOT from OpenBot**, confirmed by the same re-check: OpenBot has no
content-addressable storage, no hash-based dedup, no "last-writer-wins"
handling, and no overlay/layering concept. Its model is one dedicated volume
per bot instance — simpler than what this doc proposes, because OpenBot has no
multi-tenant-org-sharing-a-Space requirement to solve. The CAS/overlay design
below is this repo's own, motivated by a problem OpenBot doesn't have:
multiple runs, in the same Space, needing isolated concurrent writes against a
workspace other runs and a human may also be viewing or editing.
**Action item, not yet done**: add OpenBot to `docs/external-ideas-harvest.md`'s
license matrix (§1) as MIT — porting the resolve/decide/audit/execute framing
and the "everything is a volume, nothing is deleted" philosophy is licensed
freely once that entry exists; it should exist before anyone treats the
citation above as settled, per this repo's own adoption-gating rule.

**Two more things this research surfaced that shape scope below:**

- `docs/decisions/ledger.md:208-236` (2026-08-22) **rejected** a generic
  `SandboxBackend` trait "for now... a registry/interface for one
  implementation," with an explicit revisit condition: *"a second real
  sandbox backend (remote/SDK-only) is actually being built."* The same
  reasoning applies here: this doc adds exactly one object-storage backend
  (MinIO). It must not introduce a `StorageBackend`/`ObjectStore` trait ahead
  of a second real backend — see Open Questions.
- `docs/capability-ownership-matrix.md:65` lists sandbox-manager's durable
  target as **"in-mem + Redis target"**, dated from its 2026-06-03 §5 ruling —
  older and less specific than S3.2's explicit naming of
  `capability-core/internal/registry/scope_store.go` (Postgres via `pgxpool`)
  as "the template to copy." This doc follows the newer, more specific
  instruction and flags the inconsistency for the matrix's own owner to
  reconcile — see Open Questions. Two matrix rows are directly relevant:
  "Sandbox lease lifecycle | sandbox-manager (Go) | ... ✅ owner" and the
  2026-06-03 §5 ruling itself, which said full provisioning is
  "YAGNI-deferred... build provisioning only when a concrete
  per-session-workspace consumer drives it" — S3.2 is now exactly that
  concrete consumer, which is why proceeding now (not before) is justified.

## 1. Content-addressable object store

**New crate/module**: `apps/Model Plane/rust/services/execution-core/src/workspace_cas.rs`
(Rust — this is on the hot path, where a run reads/writes its own overlay) and
a thin Go counterpart `apps/Model Plane/go/services/sandbox-manager/internal/cas/blob_store.go`
(Go — sandbox-manager only ever needs to read a manifest and hand out
pre-signed URLs or proxy a byte range for snapshot inspection; it does not sit
on the per-tool-call hot path).

Every blob is content-addressed: key = `sha256:<hex>` of its raw bytes,
matching the digest convention already used everywhere in this codebase for
payload/profile digests (`spaceCapabilityPayloadDigest`,
`capability_profile_digest`, `scrub.rs`'s redaction, etc. — one hashing
convention, not a second one invented here). Stored in MinIO under
`cas/<sha256-hex>` in the bucket already provisioned but unused
(`OBJECT_STORAGE_BUCKET:-model-plane-artifacts`, `docker-compose.yml:777`).
Content-addressing gives two properties for free that a path-keyed store
would not: automatic deduplication (two runs writing the same file content
share one blob), and the compare-and-swap merge check in §3 (a hash IS the
version marker — no separate revision counter needed).

```rust
pub struct CasClient { http: Client, endpoint: Url, bucket: String, access_key: String, secret_key: String }

impl CasClient {
    pub fn from_env() -> Result<Option<Self>, String>; // None when unconfigured — same convention as CapabilityClient::from_env
    pub async fn put(&self, content: &[u8]) -> Result<String, String>; // returns "sha256:<hex>"; no-ops if the key already exists (dedup)
    pub async fn get(&self, digest: &str) -> Result<Vec<u8>, String>; // errors if the digest doesn't parse as sha256:<64 hex chars> — never trusts a caller-supplied path
}
```

This is the **first real object-storage client in Model Plane** — confirmed
by dedicated research: zero MinIO/S3 client code exists in `rust/` or `go/`
today (`execution-core/src/artifact/mod.rs` has only a comment and a pure
`build_artifact_key` formatter never called outside one test;
`sandbox-manager/internal/snapshot/snapshot.go`'s `ObjectKey` is a string
field nothing writes to). This is also independently, already-documented
debt, not news: `docs/gap-model.md:314`, `docs/core-research/sandbox-manager.md:39,43`,
`docs/gap-analysis.md:194` all say so. No S3-capable crate/package is already
a transitive dependency anywhere (`Cargo.lock`/`go.sum` checked directly) —
this is a genuinely new dependency, not a low-friction reuse. Pick an
MIT/Apache-licensed minimal HTTP-based S3-compatible client for Rust
(`rust-s3` or hand-rolled `reqwest` + AWS SigV4, since MinIO's default API is
S3-compatible) rather than the full `aws-sdk-s3` (heavier, more surface area
than this needs — put4/get2 operations only).

## 2. Durable manifest store (Postgres, sandbox-manager)

**New migration** `apps/Model Plane/go/services/sandbox-manager/migrations/0001_workspace_manifest.up.sql`
(sandbox-manager has no `migrations/` directory today — this is its first):

```sql
CREATE TABLE IF NOT EXISTS workspace_files (
    org_id        TEXT NOT NULL,
    space_id      TEXT NOT NULL,
    run_id        TEXT,              -- NULL = the Space's merged, durable state; non-NULL = one run's not-yet-merged overlay
    path          TEXT NOT NULL,
    content_hash  TEXT NOT NULL,     -- "sha256:<hex>", the CAS key
    base_hash     TEXT,              -- for a run row: the Space-level hash this path had when the run started (NULL = path didn't exist yet)
    size_bytes    BIGINT NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- PRIMARY KEY cannot contain an expression (COALESCE below) -- only an
-- index can -- so identity here is a UNIQUE INDEX, the same expression-index
-- shape as capability-core's capability_scopes_active_grant_uq.
CREATE UNIQUE INDEX workspace_files_identity_uq ON workspace_files (org_id, space_id, COALESCE(run_id, ''), path);
CREATE INDEX workspace_files_space_idx ON workspace_files (org_id, space_id) WHERE run_id IS NULL;
CREATE INDEX workspace_files_run_idx ON workspace_files (org_id, space_id, run_id) WHERE run_id IS NOT NULL;
```

(Corrected 2026-09-11 during Step 1 implementation: a live Postgres smoke test
of this exact migration surfaced the `PRIMARY KEY` expression error above
before it ever reached a real deployment — the fix is reflected here and in
the actual migration file.)

Mirrors `capability-core/internal/registry/scope_store.go` field-for-field in
spirit: soft-state via an explicit column (`run_id IS NULL` meaning "merged",
the same shape as that store's `revoked_at IS NULL` meaning "active"),
authorization/consistency pushed into SQL rather than app-level locking, a
narrow `scopeDatabase`-style interface (`Exec`/`Query`/`QueryRow`) wrapping
`*pgxpool.Pool` so unit tests substitute a stub without a real database, and a
`//go:build integration` suite using `testcontainers-go` that applies the real
`.up.sql` file — exactly `scope_store_test.go` /
`scope_store_integration_test.go`'s dual structure. `cmd/main.go` gains the
same fail-closed wiring already used there: `DATABASE_URL` required,
`os.Exit(1)` on missing config or pool-construction failure — no in-memory
fallback for a durable store that's supposed to survive a restart.

This is also where `sandbox-manager`'s own `internal/lease/lease.go` and
`internal/snapshot/snapshot.go` — currently `map + sync.RWMutex`, explicitly
flagged as the "restart is fail-closed, not durable" gap in S3.2's Open
Questions — get ported onto the same Postgres store, following the identical
template. That port is mechanical once `workspace_files` exists, since both
stores already have the narrow, injectable-clock-and-randomness shape that
made the in-memory version easy to test; the SQL swap doesn't change any
handler-level behavior in `internal/server/server.go`.

## 3. The three-layer workspace model

Per run: **org read-only** (an org's shared knowledge every run can read but
never write) → **Space** (`workspace_files` rows with `run_id IS NULL` — the
Space's own durable, cross-run state) → **run overlay** (`workspace_files`
rows with `run_id = <lease id>` — this run's own in-flight changes, isolated
from every other concurrent run in the same Space).

Critically, **this needs no new sandbox mechanism.** execution-core's
`sandbox.rs` already has exactly the primitive this design needs:
`MpSandboxPolicy::WorkspaceWrite { writable_roots: Vec<PathBuf>, network }`,
whose `writable_roots` are literal host directories bind-mounted writable
*over* a `--ro-bind / /` baseline ("later binds win in bwrap" —
`assemble_argv`, `sandbox.rs:183-189`). So the integration is: **hydrate, run,
diff** — real files land on local disk, exactly like every other tool call
today, and the bwrap plumbing is untouched:

1. **Hydrate** (before `ActivateLease` promotes SCRATCH → ACTIVE, or lazily on
   first workspace access): for the org layer and the Space layer, read each
   path's `content_hash` from `workspace_files`, fetch the blob from CAS, and
   write it to a per-run scratch directory
   (`/var/lib/execution-core/spaces/<space_id>/runs/<lease_id>/workspace/`).
   Record each hydrated path's hash as that row's `base_hash` for the merge
   check in step 3.
2. **Run**: pass that directory as one of `WorkspaceWrite`'s `writable_roots` —
   the *only* change needed in `sandbox.rs` is extending `LaunchOptions` with
   an optional list of additional read-only bind pairs
   (`extra_ro_binds: &[(PathBuf, PathBuf)]`) so the org layer can be mounted
   read-only at a distinct path from the run's writable workspace, rather than
   only ever getting the whole-filesystem `--ro-bind / /` baseline. Everything
   the run writes lands in its own directory; nothing is shared with any
   other concurrent run.
3. **Diff & upload** (on `SnapshotSandbox`, already state-gated to `ACTIVE`
   lease only by S3.2's `BeginSnapshot`/`EndSnapshot`): walk the run's
   workspace directory, hash each file, `CasClient::put` anything whose hash
   isn't already known, and upsert `workspace_files` rows with
   `run_id = <lease id>` for every path that's new or changed relative to what
   was hydrated. **`ExcludeCredentials`** (already built, S3.2 step 5 —
   `internal/snapshot/exclude.go`) runs here, on the diffed content, before
   anything reaches CAS — closing the loop the design doc that built it
   already anticipated ("ready to call once \[a real write path\] lands").

**Amendment, 2026-09-11, during step 3 implementation — a gap this
section's original wording didn't account for.** Implementing §3 surfaced a
real, previously undocumented gap: **no service anywhere in this repo
currently calls sandbox-manager's lease RPCs as a client.**
`capability-ownership-matrix.md`'s "execution-core requests lease" claim
does not match the code, confirmed by direct search: execution-core has no
`SandboxManagerClient` at all. `model-gateway` constructs one
(`state.rs:122`, `state.rs:510-514`, env vars `SANDBOX_MANAGER_URL`/
`SANDBOX_MANAGER_ADDR`) but never calls a single RPC on it anywhere in this
repo. And execution-core's only real "workspace directory" concept —
`code_interpreter.rs`'s `Workspace` (`code_interpreter.rs:303-346`) — is
fully ephemeral: created per tool call and deleted on `Drop` covering every
return path by design (its own doc comment: *"Cleanup MUST NOT hang off the
happy path"*), with no `lease_id`/`space_id` anywhere in its construction.
The `shell` tool path (`executor::execute_sandboxed`) has no workspace
concept at all.

This means the hydrate/diff mechanism above has no real trigger today, for
the same reason the CAS client (step 1) and sandbox-manager's own lease RPCs
(S3.2) shipped with no consumer: the caller-side wiring doesn't exist yet,
and building it means redesigning `code_interpreter.rs`'s carefully-built
ephemeral lifecycle to become lease/Space-aware — a distinct, larger change
deserving its own design pass, not something to fold into "add a CAS client
call."

**What step 3 actually delivers, scoped to this reality:**
- `src/workspace_hydrate.rs` (new) — `hydrate`/`diff_and_upload`, fully
  tested against a mocked CAS backend (`wiremock`), including a UTF-8-safety
  issue this exact port needed to get right that the Go original doesn't
  have: `ExcludeCredentials` (`internal/snapshot/exclude.go`) works safely on
  arbitrary bytes in Go because Go's `string` type is just an opaque byte
  sequence — `string(content)`/`[]byte(scrubbed)` round-trips losslessly
  even for invalid UTF-8. Rust's `String`/`&str` must be valid UTF-8, so a
  naive port using `String::from_utf8_lossy` would silently corrupt any
  genuinely binary file (an image, a compiled artifact) by replacing invalid
  byte sequences with U+FFFD. `workspace_hydrate.rs`'s `redact` only scrubs
  content that decodes as valid UTF-8, passing binary content through
  byte-for-byte unchanged otherwise — verified by a dedicated test using
  non-UTF-8 bytes and an exact-byte-match mock assertion, not just an "it
  didn't panic" check.
- `src/sandbox.rs`'s `extra_ro_binds` extension, exactly as originally
  planned above.

## 3.5. Wiring a real caller

Designed 2026-09-11, grounded by tracing every real call site of
`ExecuteStep`/`RunAgent` (execution-core's two entry points) before writing
anything, since the size of this step depends entirely on how far upstream a
Space's identity is actually already known.

**The core finding: `space_id` is genuinely first-class on a *thread*
record, but nowhere on the two live per-run/per-step RPCs.**
`sessions.proto`'s `ThreadSummary` carries `space_id` (field 8) and multiple
thread-scoped messages thread it through as caller-supplied context. But
`ExecuteStepRequest` and `RunAgentRequest` (`execution.proto`) have **no**
`space_id` field — confirmed against the freshest generated bindings, no
proto/codegen drift. Only the disabled `ExecuteScheduledStepRequest` lane
already has it. `RunDetail` (the run's own durable record, `runs.proto`) has
neither `space_id` nor even `org_id` — a run's identity is thinner than
expected.

**Real call-site trace (three real, non-test call sites of `ExecuteStep`;
one of `RunAgent`; none in Go or the Frontend Plane):**

- `model-gateway/src/sse.rs:5969-6032` (`spawn_run_dispatch`) builds
  `RunAgentRequest`. The enclosing SSE-invoke handler already has
  `req.space_context: Option<ThreadSpaceContext>` in scope a few dozen lines
  away (`sse.rs:~649-668`, client/BFF-supplied — "The V3 BFF injects this
  after calling Control," `http_routes.rs:6108`) — genuinely a small,
  local plumbing job to thread it into `SessionRun` and onto the request.
  **But** `RunAgent`'s own proto comment says it's still "MVP no-tool
  slice — a single InferenceCore.Infer round," so plumbing `space_id` onto
  it alone would not yet make any real tool call lease-aware.
- `model-gateway/src/tools.rs:140-166` (`handle_code_interpreter`) and
  `model-gateway/src/browser_run.rs:419-436` (`browser_run_start`) both
  build `ExecuteStepRequest` — this is the actual path real side-effecting
  tool calls take (per this repo's own architecture note: model-gateway's
  `dispatch_tool` "refuses side effects" and delegates to execution-core's
  `execute_step_inner` for anything real). **Neither has any Space context
  in scope today.** `browser_run.rs` explicitly passes `None, None` for
  `ThreadSpaceContext`; `tool_loop.rs::dispatch_tool` (the caller of
  `handle_code_interpreter`, `tool_loop.rs:1741-1773`) has `thread_id` as a
  parameter but no space-context parameter at all, and nothing upstream of
  it in this trace was checked further (a real "how far up does this chain
  need to go" question, not yet answered — see Open Questions below).
- Orchestrator-core (Go) builds bare `ExecuteStepRequest{RunId, OrgId,
  UserId}` (`activities.go:576-582, 655`) from Temporal workflow inputs that
  don't carry `ThreadID` either on the newer `StepInput` path
  (`activities.go:71-76`). Never calls `RunAgent` at all.
- No `GetThread`/single-thread-lookup RPC exists anywhere in `proto/` —
  only owner-scoped `ListThreads`. execution-core already holds a
  `SessionCoreClient` (`grpc.rs:161-162`, used today for run-ownership
  checks, skills, and terminal auth) but nothing on it reads `space_id`; a
  per-step lookup through it would also mean adding the RPC session-core
  doesn't yet expose.

**Phased plan, matching this whole design's own "smallest next slice"
discipline rather than one large change:**

1. **Phase A — proto + data plumbing only, no lease logic yet.** Add
   `space_id` to `ExecuteStepRequest` (mirroring
   `ExecuteScheduledStepRequest`'s already-established field). Traced one
   hop further than the table above to answer the "how far up" open
   question: `tool_loop.rs::dispatch_tool`'s real caller chain is
   `dispatch_audited_tool` (`tool_loop.rs:2890`) ← `run_tool_rounds`
   (`tool_loop.rs:3720`) ← `sse.rs:1554`, all inside `invoke_stream_sse`
   (`sse.rs:310`) — the SAME function that already reads
   `req.space_context: Option<ThreadSpaceContext>` earlier in its own body
   (`sse.rs:1277`, well before the `run_tool_rounds` call at 1554).
   `ThreadSpaceContext` (`session_flow.rs:118-133`) is actually a full
   Space-thread-append decision bundle (`space_decision_token`,
   `payload_digest`, `idempotency_key`, ...) — this phase uses only its
   plain `space_id` field as an identifier, not the rest of that bundle,
   which stays scoped to its own existing purpose. So this phase is
   confirmed tractable: thread `space_id: &str` (empty when
   `req.space_context` is `None`) as one new parameter through
   `run_tool_rounds` → `dispatch_audited_tool` → `dispatch_tool` →
   `handle_code_interpreter`/`browser_run_start` → the new
   `ExecuteStepRequest.space_id`. Matches this file's own existing style
   (`org_id`/`user_id`/`thread_id` are already threaded the same way; the
   already-present `#[allow(clippy::too_many_arguments)]` on these
   functions is a pre-existing condition this phase adds one more argument
   to, not one it introduces). Changes no runtime behavior for a non-Space
   call — `space_id` stays empty, exactly like `org_id`/`user_id` already do
   for an unauthenticated legacy caller — it only makes the value *reachable*
   at execution-core's own request boundary, where it cannot exist as a
   concept at all today.
2. **Phase B — the sandbox-manager client + per-run lease lifecycle. Split
   into B.1 (done) and B.2 (blocked on a real, newly-confirmed gap) after
   actually reading sandbox-manager's and Control's verification code
   rather than assuming the client alone would be enough — see below.**

   **Phase B.1 — the client itself, DONE (`execution-core/src/sandbox_manager_client.rs`).**
   A real, tested `SandboxManagerClient` wrapping all four RPCs
   (`AcquireLease`/`ActivateLease`/`SnapshotSandbox`/`ReleaseLease`) plus
   `Health`, built the same way `capability_policy.rs`'s
   `GrpcCapabilityPolicy` wraps `CapabilityCoreClient` — lazy channel from
   `SANDBOX_MANAGER_URL`/`SANDBOX_MANAGER_ADDR` (mirroring
   `grpc.rs::serve`'s session/inference/browser resolution, not
   model-gateway's still-dead `state.rs:510-514` field), a `RPC_TIMEOUT`
   wrapper, and fail-closed validation before any wire call (blank
   `scope_id`/`org_id`/`lease_id`, zero `ttl`, a Space-scoped request with no
   `capability_decision` — all rejected as `invalid_argument` locally, per
   `server.go`'s own validation order). One deliberate design choice: it
   takes `bearer: &str` per call rather than minting its own service token
   the way `capability_policy.rs`'s `ServiceTokenProvider` does — see B.2
   below for exactly why that would have been wrong, not just a style
   preference.

   **Phase B.2 — actually acquiring a Space-scoped lease from a real run —
   DECIDED and fully IMPLEMENTED 2026-09-11 (same day as B.1; see §8's own
   item 3.5.B.2 for the slice-by-slice implementation record).** First the
   finding that forced a decision, then the decision itself.

   *The finding.* This is the open question two bullets below
   ("does Phase B need to independently confirm [space_context's] trust
   level") coming back with a concrete, code-verified answer, sharper than
   when this doc first raised it: **yes, and worse — the identity binding
   sandbox-manager itself enforces cannot currently be satisfied by
   execution-core at all.**
   `internal/authz/capability_verifier.go`'s `Verify` requires
   `decision.SubjectID == expect.SubjectID`, and `internal/server/server.go`'s
   `AcquireLease` sets `expect.SubjectID = principal.ActorID` — the identity
   sandbox-manager's own auth interceptor extracted from the CALLER's bearer,
   not anything the caller merely asserts in the request body. Control's
   decision-issuance endpoint
   (`user-core/internal/http/spaces.go`'s sandbox-capability-decision
   handler) will only ever sign a decision whose `SubjectID` is an actual
   current Space member — `ResolvePersonalThreadDecisionEvidence` returns
   `ErrNoCurrentMembership` (mapped to 403) for anyone else, a service
   account included. So the caller presenting the decision to `AcquireLease`
   must authenticate AS that same member, not as execution-core's own
   service identity. That rules out reusing `capability_policy.rs`'s
   `ServiceTokenProvider` pattern (a service mints its own org-scoped token
   from its own deployment credential) — Auth Core already serves
   `POST /api/sandbox-manager/internal-token` generically
   (`plane-token.controller.ts`'s `:audience/internal-token` route, `convex-token.service.ts`'s
   `PlaneAudience` already lists `sandbox-manager`, no Auth Core change
   needed), but a token minted that way authenticates execution-core itself,
   never the acting user. What Phase B.2 actually needs is a **delegated,
   user-bound sandbox-manager bearer** — the same shape data-plane / session
   / inference / browser-broker bearers already arrive as on `ExecuteStepRequest`
   today (see `auth.rs`'s `authenticate_delegated_*` family) — and no such
   bearer exists on the wire: Phase A's `space_id` is a plain string, not a
   credential. Minting one means deciding, likely with Auth Core and
   whichever service resolves `ThreadSpaceContext` in the first place, how a
   user-bound sandbox-manager-audience token gets issued and threaded down
   to execution-core alongside `space_id` — a cross-service auth design
   question in its own right, not a mechanical continuation of B.1.

   *The decision: a ninth delegated user-bound bearer,
   `x-sandbox-authorization`, minted by the V3 BFF from the user's session
   and forwarded unchanged — presented to `AcquireLease` only; every other
   lease RPC uses execution-core's own service token.* Grounded in a full
   trace of how the existing eight delegated bearers are minted and
   forwarded today (2026-09-11), so nothing here is a new mechanism:

   - **Minting — Auth Core, no change.** `GET /api/sandbox-manager/token`
     already works: `convex-token.service.ts`'s `PlaneAudience` lists
     `sandbox-manager` (`:104`; config `:360-369`; TTL
     `PLANE_TOKEN_TTL_SANDBOX_MANAGER_SECONDS`, default 300 s), the runtime
     gate `isInteractivePlaneAudience` (`:732-734`) admits it, and
     `plane-token-scopes.ts:49-59` gives it an empty scope list — which is
     exactly what sandbox-manager wants: `authz.go:59-61` returns early for
     `principal_type == "user"` before any scope check. The minted JWT
     (`issuePlaneToken`, `convex-token.service.ts:567-586`) carries
     `sub == user_id`, `org_id`, `principal_type: "user"`, no `service_id`,
     and the org's `zdr` — every field `authctx.go:160-166` demands.
   - **Forwarding — V3 BFF (`apps/gateway`, Rust), small change.**
     `audience_tokens.rs`'s closed `ModelServiceAudience` enum already has
     `SandboxManager` (`:27`, claim `"sandbox-manager"`; the enum's own
     `#[allow(dead_code)]` note says the closed contract "deliberately
     includes audiences whose routes are not yet invoked"). Add a
     `sandbox_token` helper beside `chat/shared.rs:145-252`'s family and
     one more header, `x-sandbox-authorization`, in the two proxy fan-ins
     (`upstream.rs::proxy_sse_stream_with_data_plane:1665-1721`,
     `chat/shared.rs::proxy_model_json_with_delegations:466-524`). Mint it
     **only when the turn is Space-scoped**: the same
     `streams.rs::stream_chat` handler (`:18-116`) that mints the other
     bearers (`:24-42`) also calls
     `spaces.rs::inject_personal_thread_context` (`:76-85`), so it knows
     whether a `space_context`/`space_append_context` was produced. A
     non-Space turn — the overwhelming majority — pays no extra Auth Core
     round-trip and forwards no header. Unlike the `required_*` family this
     is therefore `Option`al at the BFF and fail-closed further down.
   - **Verification + forwarding — model-gateway, small change.** A
     `VerifiedSandboxBearer` newtype + extractor and a ~10-line
     `verify_delegated_sandbox_bearer` over the existing generic
     `verify_delegated_user_bearer` (`auth.rs:883-903`: same `sub`, same
     `user_id`, same `org_id`, `principal_type=user`, RS256 / JWKS `kid` /
     issuer / `exp` / `nbf`), audience `"sandbox-manager"`.
     `tools.rs::handle_code_interpreter` forwards it as gRPC metadata
     `x-sandbox-authorization` beside the existing four inserts
     (`:169-198`) — only when present.
   - **Verification — execution-core, small change, conditional.** A
     hand-written `authenticate_delegated_sandbox_manager` (no shared helper
     exists here — `auth.rs:393-462` is three ~16-line copies) reading
     `x-sandbox-authorization`, audience `"sandbox-manager"`, and checking
     `org_id` / `user_id` / **`zdr`** equality with the primary execution
     bearer exactly as `authenticate_delegated_data_plane` does
     (`:446-462`; the retention-posture equality is the easy one to miss).
     Demanded in `grpc.rs::execute_step` **only when
     `tool_name == "code_interpreter"` AND `req.space_id` is non-empty**,
     mirroring how the browser bearer is demanded only for browser tools
     (`:778-789`) — so no existing `ExecuteStep` caller breaks, and a
     Space-scoped sandbox step arriving without the bearer fails closed
     with `permission_denied` rather than silently downgrading to the
     ephemeral path.
   - **Use — `AcquireLease` only.** Immediately before acquiring, request
     the capability decision through `capability_client.rs` (already
     service-authenticated with `X-Service-Token`, which user-core's handler
     accepts — it is the *subject*, not the *requester*, that must be a
     member) with `subject_id = req.user_id`, `space_ref = req.space_id`,
     `org_id = req.org_id`, this instance's `backend_id`, and
     `idempotency_key = run_id`. The decision lives **2 minutes**
     (`personal_thread_decision.go:19`), so it is never minted ahead of
     time or cached. Extend `capability_client.rs`'s `DecisionResponseData`
     to capture the `claims` sidecar user-core already returns
     (`spaces.go:1229`: `{"data":{"decision":…,"claims":…}}`) — dropped
     today, and it is verbatim `AcquireLeaseRequest.capability_claims_json`.
     Then `SandboxManagerClient::acquire_lease(user_bearer, …)` with
     `scope_type = "agent"`, `scope_id = run_id` (one lease per run is this
     doc's per-run-overlay model, §3), `space_id`, decision, claims.
     sandbox-manager records `owner_id = user_id`.
   - **Lifecycle after acquire — service token, decoupled from the user
     bearer's 300 s TTL.** `ActivateLease` / `SnapshotSandbox` /
     `ReleaseLease` never touch the capability verifier and owner-filter
     only user principals — `lease.go:148,247`: `($3 = '' OR owner_id =
     $3)`, and `authz.go::OwnerFilter` returns `""` for a service. So
     execution-core runs the rest of the lifecycle with its OWN
     `principal_type=service` token from
     `POST /api/sandbox-manager/internal-token` carrying `sandbox:write`
     (a `SandboxManagerTokenProvider` mirroring `capability_policy.rs`'s
     `ServiceTokenProvider`, `:577-735`), at any point in the run — a
     release at run end does not depend on a user credential that expired
     minutes earlier. Precondition to verify at implementation time: Auth
     Core's service-principal registry (`plane-service-principal.ts`)
     grants execution-core's service id `sandbox:write` for that audience.
   - **Per-run lease state.** A `leases: DashMap<String, SandboxLeaseHandle>`
     on `state.rs`'s `StateStore`, keyed by `run_id`, following its
     `owners: DashMap<String, RunOwner>` (`:60-64`; insert-once via
     `entry`, read via `.get`). Released on `CancelRun` and on run
     completion with the service token; the lease `ttl` is the backstop if
     the process dies first.
   - **Two loose ends that belong in the same slice.** (1) Phase A reads
     only `req.space_context`, which the BFF sets for the FIRST message of a
     scoped thread; appended messages carry `req.space_append_context`
     instead (`http_routes.rs:6104-6112`), so `space_id` is empty on every
     later turn today — `sse.rs` must read either. (2) `RunAgentRequest` has
     no `space_id` at all, and its `run_agent` handler (`grpc.rs:1087`)
     demands only the data-plane and inference bearers, yet the governed
     agent loop can dispatch `code_interpreter` too. Add `space_id` there
     with the same conditional bearer demand, or explicitly leave `RunAgent`
     on the ephemeral path — decide, don't drift.

   *Rejected, and why — also recorded in
   `apps/Model Plane/docs/decisions/ledger.md` (2026-09-11) so none is
   re-proposed:*
   - **execution-core mints a service token and uses it for `AcquireLease`.**
     Fails `SubjectID == ActorID` deterministically (the finding above). Not
     a style preference — PermissionDenied on every real request.
   - **Token exchange / on-behalf-of at execution-core** (swap the incoming
     user-bound execution bearer for a user-bound sandbox-manager one). No
     such mechanism exists anywhere in the Model Plane — zero hits for
     `on_behalf` / `obo` / `token_exchange` across Rust and Go — and the
     ledger's 2026-08-26 dev-bypass CORRECTION already settled the rule it
     would break: every delegated bearer is a user credential minted at the
     BFF from the session, and no downstream service synthesises one from
     another token ("it does not propagate trust across planes"). OBO would
     be a second identity path Auth Core does not own.
   - **model-gateway mints it.** It holds no session cookie, only forwarded
     bearers, so it cannot call `GET /api/:audience/token` — and the same
     ledger rule forbids the gateway minting delegated bearers.
   - **Relax sandbox-manager's subject binding** (accept a service principal
     plus a request-body `subject_id`, or an `on_behalf_of` claim). Undoes
     S3.2 step 4's whole point — verification against the caller's
     *verified* identity, never a body assertion (`authctx.go:31-32`).
   - **Mint the sandbox bearer on every chat turn.** Simpler plumbing, but
     one more sequential Auth Core round-trip per turn for a credential
     almost no turn uses, when the BFF already knows whether the turn is
     Space-scoped before it would mint.

   Until B.2 lands, `sandbox_manager_client.rs` remains real, tested, and
   uninvoked — the same honestly-documented "no caller yet" position Step 3
   was in this morning, now with the caller's exact shape decided.
3. **Phase C — `code_interpreter.rs` uses the lease's hydrated workspace.**
   When a Space-scoped lease is `ACTIVE`, hydrate its current
   `workspace_files` manifest (needs the `GetWorkspaceManifest`-style RPC
   this doc's §6 file list already flagged as not-yet-built) into a
   *persistent-for-the-run* directory instead of `Workspace::create`'s
   per-call ephemeral one, run the tool call against it, then
   `diff_and_upload` back on `SnapshotSandbox`. The `shell` tool path (no
   workspace concept at all today) is explicitly out of scope for this
   phase — extending it is its own follow-up, not silently bundled in.

**Open questions this design pass surfaced, not resolved:**
- ~~How far above `dispatch_tool` does the trace need to go~~ — resolved
  above: `invoke_stream_sse` itself, confirmed by reading the code, not
  inferred.
- ~~`req.space_context` is client/BFF-declared... is that trust level
  acceptable for pinning a sandbox lease to a Space?~~ — **answered, and
  reframed, by B.2's research above: this is moot until a delegated
  user-bound sandbox-manager bearer exists at all**, because
  `AcquireLease`'s own server-side identity check (`expect.SubjectID ==
  principal.ActorID`, from the CALLER's verified bearer) is strictly
  stronger than anything `space_id` alone could assert either way — Control's
  decision-issuance already independently re-verifies current Space
  membership (`ResolvePersonalThreadDecisionEvidence`) for whoever that
  bearer identifies. The follow-on question — who mints that bearer and how
  it reaches execution-core — is **decided the same day, see B.2 above**: the
  V3 BFF mints `GET /api/sandbox-manager/token` from the user's session only
  for a Space-scoped turn, forwards it as `x-sandbox-authorization`, and
  execution-core presents it to `AcquireLease` alone.
- ~~Per-run lease-state storage~~ — decided with B.2: a new `leases` field
  on `state.rs`'s `StateStore`, following its `owners: DashMap<String,
  RunOwner>` (insert-once per `run_id`, read via `.get(run_id)`). Not yet
  added; it lands with B.2's caller.
- Whether Phase A's `ExecuteStepRequest.space_id` should be trusted as-is
  (matching how `org_id`/`user_id` are already trusted, sourced from
  verified bearer claims) or needs its own verification step, given a
  Space capability decision is a strictly more consequential grant (compute
  + potential egress) than the read/knowledge-search scoping `org_id`
  already gates.

## 4. Merge: run overlay → Space (this is "never last-writer-wins")

A run's overlay is never written directly into the Space's `run_id IS NULL`
rows as a side effect of the run itself — that would BE last-writer-wins,
just deferred to snapshot time instead of write time. Merging is its own
explicit step (a new RPC, `PromoteWorkspace(lease_id, backend_id)`, mirroring
`ActivateLease`'s shape), and each path is a **compare-and-swap on
`base_hash`**:

```sql
-- one path, inside a transaction, per row in the run's overlay:
UPDATE workspace_files
SET content_hash = $new_hash, size_bytes = $new_size, updated_at = now()
WHERE org_id = $org_id AND space_id = $space_id AND run_id IS NULL AND path = $path
  AND content_hash = $base_hash;  -- the hash this run saw when it hydrated
-- 0 rows affected + the row exists with a DIFFERENT hash now = CONFLICT
-- 0 rows affected + the row doesn't exist and base_hash was NULL = fine, INSERT instead
```

A conflicting path is never silently resolved by whichever run's promotion
happened to run second — both versions remain fully recoverable (content is
immutable and hash-addressed in CAS; nothing is ever overwritten or deleted
there), and the conflict is surfaced rather than auto-picked. **What exactly
"surfaced" means — return an error listing conflicting paths for the caller
to retry/reconcile, or something richer — is an open product question, not
resolved here; see Open Questions.** Non-conflicting paths in the same
promotion always merge, independent of any conflicting path elsewhere in the
same run's overlay (per-path CAS, not one all-or-nothing check across the
whole overlay).

## 5. Output promotion through Data APIs

Distinct from the merge in §4, and only for content that should become
durable, RAG-searchable **knowledge** — not every workspace file (a build's
intermediate artifact belongs in the Space's CAS-backed workspace so the next
run in that Space can see it; it does not belong in Data Plane v2 as a
document). This is an explicit, separate action a run or its caller takes for
specific paths, not an automatic side effect of `PromoteWorkspace`.

**Already-shipped precedent to follow directly**, found by this research, not
invented here: `apps/Model Plane/rust/services/model-gateway/src/dataplane.rs`'s
`create_document`/`bulk_ingest` (lines 415-446, 484-531) already call
`documents-api-go`'s gRPC `DocumentService.CreateDocument`/`BulkIngest`
(`apps/Data Plane v2/proto/documents_v2.proto:122-130`) directly — gated by
`reject_zdr_durable_mutation` and a `VerifiedDataPlaneBearer`. Confirmed
against `apps/CODEBASE_INFORMATION_SYSTEM.md:109` ("No direct database
crossing. Model, Frontend, Application, and Ingestion consume Data Plane APIs
only") that Model Plane calling Data Plane v2's API directly is the
sanctioned path — Ingestion Plane is not a required intermediary for a run's
own output, only for *externally-fetched evidence* (that's what
`execution-core/src/promote_on_use.rs::promote`'s Quarry-v2 route is for, a
different case). **execution-core should replicate `dataplane.rs`'s client
pattern** (same `CreateDocumentRequest{org_id, source, type, title, content,
metadata, zdr_classification}` shape, same ZDR gate, same bearer
verification) rather than inventing a new one — this is a "second consumer of
an existing pattern," not a second implementation of the same capability.

## 6. File-by-file change list

**Model Plane — execution-core (Rust):**
- NEW `src/workspace_cas.rs` — `CasClient`, `put`/`get`, `from_env`
- MODIFY `src/sandbox.rs` — `LaunchOptions` gains `extra_ro_binds`;
  `assemble_argv` emits an extra `--ro-bind <host> <mount>` pair per entry,
  placed after the writable-root binds (later binds still win where they
  overlap, matching the existing ordering comment)
- NEW `src/workspace_promote.rs` — the `documents-api-go` client, modeled on
  `model-gateway/src/dataplane.rs`'s `create_document`/`bulk_ingest`
- MODIFY `src/lib.rs` — register the two new modules

**Model Plane — sandbox-manager (Go):**
- NEW `migrations/0001_workspace_manifest.up.sql`
- NEW `internal/cas/blob_store.go` — read-only MinIO client (manifest lookup +
  pre-signed URL issuance; sandbox-manager never writes CAS content itself,
  execution-core does)
- NEW `internal/workspace/store.go` — `workspace_files` CRUD, mirroring
  `scope_store.go`'s narrow-interface/fail-closed shape
- MODIFY `internal/lease/lease.go`, `internal/snapshot/snapshot.go` — ported
  onto Postgres per §2; `Store` constructors now take a `*pgxpool.Pool`
- MODIFY `internal/server/server.go` — new `PromoteWorkspace` RPC handler
  (§4); `SnapshotSandbox` calls the diff-and-upload step (§3.3)
- MODIFY `cmd/main.go` — `DATABASE_URL`-required fail-closed wiring (§2)

**Model Plane — proto:**
- MODIFY `proto/model_plane/v1/sandboxes.proto` — `PromoteWorkspace` RPC +
  request/response messages (lease_id, backend_id, conflicting_paths in the
  response)

**Docs (housekeeping, not code):**
- MODIFY `docs/external-ideas-harvest.md` — add OpenBot (MIT) to §1's license
  matrix, per the "action item, not yet done" above
- MODIFY `docs/capability-ownership-matrix.md` — reconcile the "in-mem +
  Redis target" row with the Postgres-via-pgxpool store this doc (and S3.2
  before it) actually specifies

## 7. Test plan

| Area | File | Assertion |
|---|---|---|
| CAS dedup | `workspace_cas.rs` tests | putting identical content twice returns the same digest and does not re-upload (mock transport call-count assertion) |
| CAS digest validation | `workspace_cas.rs` tests | `get` rejects a malformed digest before making any request — never trusts a caller-supplied path into the bucket |
| Manifest store, unit | `internal/workspace/store_test.go` | hand-rolled `pgx.Rows`/`pgconn.CommandTag` stub, same style as `scope_store_test.go` — exact SQL fragment + bound-arg assertions |
| Manifest store, integration | `internal/workspace/store_integration_test.go` (`//go:build integration`) | real Postgres via testcontainers, applies `0001_workspace_manifest.up.sql`, exercises hydrate/diff/merge end to end |
| Merge conflict | `server_test.go` new `TestPromoteWorkspaceReportsConflictWithoutOverwriting` | two runs hydrate the same path, one promotes first, the second's promotion reports that path as conflicting and neither Space-level row nor CAS content is lost |
| Merge non-conflict | `server_test.go` new `TestPromoteWorkspaceMergesNonConflictingPathsIndependently` | one conflicting path in an overlay does not block the other paths in the same promotion |
| Hydrate/mount integration | `sandbox.rs` new test | `extra_ro_binds` entries appear in `assemble_argv`'s output as `--ro-bind <host> <mount>` pairs, ordered after writable-root binds |
| Credential exclusion still enforced | extend `exclude_test.go` | `ExcludeCredentials` runs on diffed content before any `CasClient::put` call (mock CAS client call assertion) |
| Promotion to Data Plane | `workspace_promote.rs` tests | ZDR-flagged content is rejected before any `CreateDocument` call, mirroring `dataplane.rs`'s existing `reject_zdr_durable_mutation` test coverage |

## 8. Sequencing

1. **CAS client + manifest schema** (this doc's §1-2) — no consumer yet,
   fully testable in isolation, same "no behavior change" character as S3.2's
   own step 1.
2. **Postgres port of `lease.Store`/`snapshot.Store`** (§2) — closes S3.2's
   own named "restart" gap; existing `server_test.go` behavior must not
   change, only the storage backend underneath it.
3. **Hydrate/diff mechanism in execution-core** (§3) — depends on 1 (CAS)
   and 2 (manifest rows to hydrate from). Shipped as tested, standalone
   `workspace_hydrate.rs` + `sandbox.rs`'s `extra_ro_binds`; per this
   section's own 2026-09-11 amendment, has no real caller yet — see step 3.5
   below, designed the same day after implementing this one surfaced the gap.
3.5. **Wire a real caller** (§3.5, newly designed, not in the original
   sequencing), itself phased:
   - **3.5.A — DONE** (`aabb865d`) — plumb `space_id` onto
     `ExecuteStepRequest` and its two real call sites (`tools.rs`,
     `browser_run.rs`); pure data reachability, no lease logic, no runtime
     behavior change for a non-Space call.
   - **3.5.B.1 — DONE** (`execution-core/src/sandbox_manager_client.rs`,
     same day) — the sandbox-manager gRPC client itself (all four RPCs +
     `Health`), tested, uninvoked.
   - **3.5.B.2 — DONE.** Per-run lease
     acquire/activate lifecycle, chaining S3.2 step 3's `capability_client.rs`
     and step 4's verification together with B.1's client for the first time.
     The blocker found while designing it — sandbox-manager's `AcquireLease`
     binds to the CALLER's verified identity, which must be the Space member
     the decision names — is resolved by a ninth delegated user-bound bearer,
     `x-sandbox-authorization`: minted by the V3 BFF from the user's session
     via the already-working `GET /api/sandbox-manager/token`, only for
     Space-scoped turns, verified at model-gateway and execution-core like
     the other eight, presented to `AcquireLease` only; the rest of the
     lifecycle uses execution-core's own `sandbox:write` service token. Full
     mechanism, rejected alternatives, and evidence in §3.5's B.2 writeup and
     `apps/Model Plane/docs/decisions/ledger.md` (2026-09-11). Implementation
     slices, in order: (a) BFF mint + header — **DONE** (V3 gateway:
     `chat::shared::sandbox_token` mints `aud=sandbox-manager` only after
     `inject_personal_thread_context` produced a `space_context` /
     `space_append_context`, forwarded as `x-sandbox-authorization` on the
     SSE, JSON, and AG-UI invoke paths; nothing minted or sent for a
     non-Space turn); (b) model-gateway verify + forward — **DONE**
     (`auth::VerifiedSandboxBearer` + `verify_delegated_sandbox_bearer` over
     the shared `verify_delegated_user_bearer`, on both `require_auth` paths
     and in the service-principal header refusal list; threaded
     `invoke_stream_sse` → `run_tool_rounds` → `dispatch_audited_tool` →
     `dispatch_tool` → `handle_code_interpreter`, which forwards it as gRPC
     metadata `x-sandbox-authorization` only when present; `dispatch_tool`
     refuses a Space-scoped `code_interpreter` call that arrived without it
     rather than round-tripping to a refusal — execution-core remains the
     enforcement point. Loose end (e)(1) closed in the same slice: `sse.rs`
     now reads `space_id` from `space_context` OR `space_append_context`);
     (c) execution-core verify + `capability_client.rs` claims capture +
     `AcquireLease` on the first Space-scoped step, cached in `StateStore` —
     **DONE**. What actually landed, and where it differs from the original
     sketch:
     - **`auth::authenticate_delegated_sandbox_manager`** — the ninth
       delegated-bearer verifier, mirroring `authenticate_delegated_data_plane`
       exactly (org/user/zdr equality against the caller). Demanded in
       `grpc.rs::execute_step` only when `tool_name == "code_interpreter" &&
       !space_id.is_empty()` (mirrors the browser-bearer conditional); demanded
       in `grpc.rs::run_agent` whenever `space_id` is non-empty AT ALL,
       unconditional on tool name — a governed run decides tool-by-tool only
       once the loop is already running, so there is no earlier point to gate
       per-tool the way `execute_step` can.
     - **`capability_client.rs`** now returns `(decision, SpaceCapabilityClaims)`
       from `request_decision` — a new pub struct mirroring user-core's
       `sandboxCapabilityClaims`/sandbox-manager's `SpaceCapabilityClaims` Go
       structs field-for-field (JSON key order doesn't need to match; field
       names and types do, since sandbox-manager independently re-derives the
       payload digest from whatever claims JSON it receives — it never trusts
       byte-identical reproduction). `http_health.rs`'s existing caller
       discards the claims half; nothing else needed to change.
     - **`sandbox_lease.rs`** (new module) — `SandboxLeaseContext` (space_id +
       bearer + the two clients + backend_id, one bundle so only ONE new
       parameter threads through the dispatch chain) and `ensure_sandbox_lease`
       (cache hit on `StateStore.sandbox_lease(run_id)`, else request a
       decision + `AcquireLease`, cache the result). `ActivateLease` is
       deliberately never called: `code_interpreter` stays `SCRATCH` because it
       is hermetic (`egress: "disabled_by_default"`) — no capability here needs
       more than the credential-free allowlist `AcquireLease` already grants.
     - **The real surprise, found only by tracing the actual call graph, not by
       inspecting doc comments**: `RunAgent`'s own governed multi-tool loop
       (`runtime_loop::agent::run_agent_with_tools` → `run_rounds`) dispatches
       through the EXACT SAME `execute_step_inner` core `ExecuteStep` does —
       both `grpc.rs::run_agent` and `RunAgent`'s stale "MVP no-tool slice" doc
       comment (both the proto's own field-8 comment and `grpc.rs`'s own, now
       fixed) were simply wrong about the code's current behavior. This meant
       threading `SandboxLeaseContext` through `execute_step_inner` ONCE (via
       `execute_step_with_browser_grant`/`execute_step_with_subagent`, its two
       wrapper entry points) covers BOTH `ExecuteStep` and `RunAgent`'s
       `code_interpreter` calls with one implementation, not two — closing
       loose end (e)(2) as a side effect of doing (c) correctly, rather than as
       separate follow-up work. `RunAgentRequest` gained `space_id` (proto
       field 14) and a `LoopContext` field carrying the bearer + clients,
       inherited verbatim by delegated subagents exactly like every other
       run-identity field already is.
     - **A precondition the design flagged but had not verified turned out to
       be UNMET, and needed its own fix**: `apps/Control Plane/config/
       plane-service-principals.json`'s `execution-core` entry listed
       `capability-core`/`session-core`/`quarry` as its only audiences — no
       `sandbox-manager`, no `sandbox:read`/`sandbox:write` scope anywhere.
       Without this, `POST /api/sandbox-manager/internal-token` (slice (d)'s
       own service-token lifecycle) would 403 even after all the code above
       shipped. Added `sandbox-manager` to `audiences`/`scopesByAudience` with
       `sandbox:read`+`sandbox:write` and `retentionByAudience: "persistent"`
       (lease/snapshot metadata is durable compute state, the same rationale
       as execution-core's existing `session-core` entry).
       `scripts/run-control-plane.sh` reuses the already-minted credential —
       nothing in the running fleet needs to rotate.
     - Also fixed the model-gateway side that RunAgent threading exposed:
       `sse.rs::spawn_run_dispatch` builds the `RunAgentRequest` sent to
       execution-core, and had never read `req.space_context`/
       `space_append_context` at all — the new proto field would otherwise
       have gone out empty on literally every agentic-run call, making all of
       the above execution-core work unreachable in practice. Threaded
       `space_id` and the (already-verified-by-slice-(b)) `sandbox_bearer`
       through `agentic_run_stream` → `spawn_run_dispatch` →
       `authenticated_run_agent_request`'s conditional
       `x-sandbox-authorization` header, the same shape
       `tools.rs::handle_code_interpreter` already uses for `ExecuteStep`.
     Tests: `cargo test -p execution-core --all-targets` and
     `cargo test -p model-gateway --lib` both green (the 2 pre-existing
     execution-core failures — `executor.rs`'s Windows path-format assertion,
     `grpc.rs`'s timing-sensitive HITL test — reproduce identically in
     isolation and are unrelated, per the same check done for phase B.1).
     `go build`/`go vet` clean for sandbox-manager. Full workspace `cargo
     check` surfaces only the pre-existing Windows `tokio::signal::unix`
     failure in `inference-core`'s bin target and a pre-existing
     `SessionMessage` fixture gap in `e2e_invoke_chain_test.rs` (tracked since
     `97b25766`), neither related to this change.
     Both (e)(1) (`space_append_context`, slice (b)) and (e)(2)
     (`RunAgentRequest.space_id`, this slice) are now closed — no Phase A
     loose end remains.

     (d) `StateStore.leases`' release-on-cancel/complete lifecycle, plus the
     `SandboxManagerTokenProvider` that mints execution-core's own
     `sandbox:write` service token for it — **DONE**, same day. New
     `StateStore::take_sandbox_lease` (remove-and-return, so a retried release
     is a no-op rather than a second attempt) backs a new
     `sandbox_lease::release_sandbox_lease_if_any` (best-effort: a release
     failure never fails the RPC it rides with — `LEASE_TTL` is the backstop
     either way). `SandboxManagerTokenProvider` is a deliberate independent
     copy of `capability_policy.rs`'s `ServiceTokenProvider` shape (same
     `EXECUTION_CORE_SERVICE_ID`/`EXECUTION_CORE_SERVICE_API_KEY`, different
     audience/scope) rather than a generalized one — matches this codebase's
     own established convention of one token provider per audience (six
     independent copies already exist on the model-gateway side alone).
     Wired at the two points execution-core can actually observe a run's end,
     confirmed by tracing the call graph rather than assumed:
     `grpc.rs::cancel_run` (any run, however its lease was acquired) and
     `RunAgent`'s own `finalize()` (after it makes the run terminal — never
     before, since `finalize` returns `Err` without mutating `StateStore`
     when the durable receipt itself fails). One release per `RunAgent`
     invocation covers every delegated subagent too: a subagent's
     `LoopContext.req` is `parent.req` verbatim, so it never has a `run_id`
     of its own to key a second lease under. The inline `ExecuteStep` chat
     path (`code_interpreter` dispatched directly, not through `RunAgent`)
     has no observable "run ended" signal at all — confirmed, not assumed —
     so a lease acquired there relies on `LEASE_TTL` alone unless the same
     run is explicitly cancelled; a known, bounded gap (delayed cleanup, not
     an unbounded leak), stated in `sandbox_lease.rs`'s own module doc rather
     than left implicit.
     Tests: `cargo test -p execution-core` across all 8 targets — 561 passed
     (the same pre-existing failures as slice (c), none touching the files
     this slice changed). fmt/clippy clean on every file actually touched.

     With (a)-(d) done, phase B.2 is fully implemented: B.1's client (built,
     tested, uninvoked at the time) is exercised end to end by a real caller
     for `AcquireLease`/`ReleaseLease`; only `ActivateLease`/`SnapshotSandbox`
     stay uninvoked, deliberately, since no capability here needs more than
     the credential-free `SCRATCH` allowlist yet.
   - **3.5.C — designed 2026-09-12, not yet implemented.**
     `code_interpreter.rs` uses the lease's hydrated workspace instead of
     its own ephemeral one when Space-scoped; `shell` explicitly out of
     scope for this phase. Depends on B.2 (done) for `AcquireLease`/lease
     caching and on step 3's already-shipped `workspace_hydrate.rs` (§3) for
     `hydrate`/`diff_and_upload`.

     **A confirmed pre-existing bug this research surfaced, not previously
     load-bearing because nothing called it this way**:
     `sandbox-manager/internal/authz/authz.go`'s `Authorize()` switch has
     explicit cases only for `AcquireLease`/`ReleaseLease`/`SnapshotSandbox`
     (→ `ScopeWrite`) and `Health` (→ falls through to the `ScopeRead`
     default); every other method name — confirmed directly against
     `authz_test.go:35`'s own `.../FutureMethod` case — hits `default:
     return errors.New("unknown sandbox-manager method")`.
     `ActivateLease`'s real gRPC method name
     (`SandboxManager_ActivateLease_FullMethodName`,
     `sandboxes_grpc.pb.go:25`) is **not** in either case, so any call to it
     through the real interceptor (`authz.UnaryInterceptor`, wired at
     `cmd/main.go:128`) is refused today, for any principal, valid token or
     not. This has never surfaced as an incident only because nothing has
     ever called it that way: every existing `ActivateLease` test
     (`server_test.go`) invokes `s.ActivateLease(...)` directly on the
     struct, bypassing the interceptor entirely, and
     `SandboxManagerClient::activate_lease` (Rust, built in B.1) has zero
     real callers today — B.2 deliberately never calls it (`sandbox_lease
     .rs`'s own comment: `code_interpreter` stays SCRATCH, hermetic, no
     capability needs more). 3.5.C is the first phase to give it one, and
     per `lease.go`'s own `BeginSnapshot` gate (`ErrLeaseNotActivated` when
     `SpaceID != "" && State == SCRATCH`), every Space-scoped
     `SnapshotSandbox` call would fail closed with `FailedPrecondition`
     today — not a design gap but a one-line interceptor fix, made now
     because this is the first phase load-bearing on it. Fix: add
     `"/model_plane.v1.SandboxManager/ActivateLease"` to the `ScopeWrite`
     case alongside the existing three.

     **New RPC — `GetWorkspaceManifest(lease_id, backend_id) returns
     (repeated WorkspaceManifestEntry{path, content_hash})`.** Mirrors
     `ActivateLease`'s request shape exactly (org_id from the verified
     principal via `s.principal(ctx)`, never caller-supplied, matching
     every other RPC on this service); needs its own `ScopeRead` case in
     `authz.go` (same treatment as `Health`) or it hits the identical
     unreachable-by-default failure just fixed above. Resolves `space_id`
     from `lease_id` via `lease.Store.GetScoped` (`lease.go:170-172` —
     real, tested, but per `internal/server/store.go:11-13`'s own comment
     "unused by `Server` today"; this is exactly the seam that comment
     flagged). Returns an empty list for a non-Space lease (`SpaceID ==
     ""`), the same exemption `BeginSnapshot` already grants. The manifest
     itself is the two-layer view — Space (`run_id IS NULL`) shadowed by
     this run's own overlay (`run_id = <lease_id>`) — via a `DISTINCT ON`
     query with no existing precedent in this Go codebase (checked: zero
     `DISTINCT ON`/`UNION` hits repo-wide today):
     ```sql
     SELECT DISTINCT ON (path) path, content_hash
     FROM workspace_files
     WHERE org_id = $1 AND space_id = $2 AND (run_id IS NULL OR run_id = $3)
     ORDER BY path, run_id IS NULL ASC  -- FALSE (the overlay row) sorts
                                        -- first, so DISTINCT ON keeps it
                                        -- over the Space row on a tie
     ```

     **`SnapshotRequest` gains `repeated WorkspaceChangedFile
     changed_files`** (a new message: `path`, `content_hash`, `size_bytes`,
     `base_hash` — the Space-level hash this path had when the run's
     hydrate observed it, empty if the path didn't exist yet). Empty for
     every non-Space snapshot — existing behavior, existing tests,
     unchanged. `base_hash` must be captured at hydrate time in
     execution-core (the only place that ever has the run's true
     hydrate-time baseline in hand) and carried through, not re-derived at
     snapshot time from whatever the Space row says then — step 4's
     compare-and-swap merge (§4) depends on this being what the run
     actually observed, not a value that could have drifted if another run
     promoted first. `workspace_hydrate.rs`'s existing `ChangedFile`
     (`path`, `content_hash`, `size_bytes`) gains a fourth field,
     `base_hash: Option<String>`, populated from the `baseline: &HashMap<
     String, String>` already in scope inside `diff_and_upload`'s loop — no
     new plumbing, the value is already there.

     **New Go file `internal/workspace/store.go`**, mirroring
     `capability-core/internal/registry/scope_store.go`'s exact template
     (confirmed via full read: narrow `workspaceDatabase` interface over
     `Exec`/`Query`, `NewStore(pool *pgxpool.Pool) (*Store, error)`
     validating `pool != nil`, inline SQL, `fmt.Errorf`-wrapped errors) —
     the same template `lease.go`/`scope_store.go` already both follow:
     - `GetManifest(ctx, orgID, spaceID, runID string) ([]ManifestEntry,
       error)` — the `DISTINCT ON` query above.
     - `UpsertOverlay(ctx, orgID, spaceID, runID string, files
       []ChangedFile) error` — one `INSERT ... ON CONFLICT (org_id,
       space_id, COALESCE(run_id, ''), path) DO UPDATE SET content_hash =
       EXCLUDED.content_hash, size_bytes = EXCLUDED.size_bytes, base_hash =
       EXCLUDED.base_hash, updated_at = now()` per file, using the existing
       `workspace_files_identity_uq` index — no new index needed, the
       migration already anticipated this.

     **`server.go` wiring**: `Server` gains a `workspace WorkspaceStore`
     field (new interface in `store.go`, injected the same way
     `LeaseStore`/`SnapshotStore` are — `NewServer`'s real, single caller in
     `cmd/main.go` updates alongside it; unlike execution-core's dead
     constructors earlier this initiative, this one has a real production
     call site to update, not delete). `GetWorkspaceManifest` handler:
     resolve + validate the lease via `leases.GetScoped`, return no entries
     for a non-Space lease, else `workspace.GetManifest`. `SnapshotSandbox`
     extended: after `BeginSnapshot` succeeds, if the lease is Space-scoped
     and `changed_files` is non-empty, call `workspace.UpsertOverlay`
     *before* `snapshots.Create` — fail closed (no snapshot record
     referencing content whose overlay row was never durably written)
     rather than risk an orphaned snapshot; if `snapshots.Create` itself
     later fails, the overlay upsert already landed and is safe to leave
     (idempotent, re-upserted identically on retry). `cmd/main.go` gains a
     `WorkspaceStore` wired the same Postgres-required /
     in-memory-fallback-for-dev split as `LeaseStore`/`SnapshotStore`
     already are.

     **Rust side**: `SandboxManagerClient` gains `get_workspace_manifest`
     (mirrors `activate_lease`'s shape) and `snapshot_sandbox` gains a
     `changed_files` parameter. execution-core needs its own `CasClient`
     constructed at startup — confirmed nothing does this today outside
     tests (`CasClient::from_env()` exists, §1, but has zero non-test
     callers) — stored alongside `SandboxManagerClient` on whatever shared
     state `grpc.rs`'s handlers already reach (mirrors how
     `SandboxManagerClient`/`CapabilityClient` themselves got threaded in
     B.2). A new per-run cache slot is needed on `StateStore` — hydrate
     baseline (`HashMap<String, String>`) and the persistent workspace
     `PathBuf` — reusing the exact "insert-once, read-many" idiom
     `sandbox_lease`/`owners` already established, not a new pattern.

     **`code_interpreter.rs` rewrite, Space-scoped path only** (the
     non-Space path — no `SandboxLeaseContext`, or one with an empty
     `space_id` — keeps today's `Workspace::create`/`Drop` ephemeral
     lifecycle byte-for-byte unchanged; this is the hard invariant the
     rewrite must not disturb):
     1. First Space-scoped `code_interpreter` call in a run: call
        `ActivateLease` (this lease's first — SCRATCH → ACTIVE), then
        `GetWorkspaceManifest`, then `workspace_hydrate::hydrate` into a
        persistent directory (`.../spaces/<space_id>/runs/<lease_id>/
        workspace/`, per §3's original plan) instead of `Workspace::
        create`'s fresh temp dir; cache the directory + hydrate baseline on
        `StateStore` keyed by `run_id`.
     2. Every later Space-scoped call in the same run reuses that cached
        directory instead of creating a new one — the one deliberate
        exception to `Workspace::create`'s own "never adopt an existing
        directory" doc-commented invariant, which stays true for every
        non-Space call.
     3. At the two existing observable "run ended" points
        (`grpc.rs::cancel_run`, `RunAgent`'s `finalize()`, per B.2 slice
        (d)) — before releasing the lease, if this run hydrated a
        workspace: `diff_and_upload`, then `SnapshotSandbox` with the
        resulting `changed_files`, then remove the persistent directory
        (mirrors `Workspace`'s own `Drop`, just keyed to lease release
        instead of per-call). Best-effort, same as the existing release
        call it now precedes — a snapshot/upload failure must not block the
        lease release or fail the RPC it rides with.

     **Sub-phases, in dependency order** (mirrors B.2's (a)-(d) cadence —
     smallest, most self-contained, most mechanically-verifiable first):
     - C.1 — Go: `internal/workspace/store.go` + unit/integration tests,
       `LeaseStore` gains `GetScoped`. No RPC wiring yet, fully testable in
       isolation like step 1's CAS client was.
     - C.2 — proto: `GetWorkspaceManifest` RPC + messages,
       `SnapshotRequest.changed_files`; regenerate Go/Rust/TS/Python
       bindings.
     - C.3 — Go: `server.go` handler + `SnapshotSandbox` extension +
       `authz.go`'s two fixes (the `ActivateLease` bug,
       `GetWorkspaceManifest`'s new case); `cmd/main.go` wiring.
     - C.4 — Rust: `SandboxManagerClient::get_workspace_manifest` +
       `snapshot_sandbox` signature change; `ChangedFile.base_hash`;
       `CasClient` construction in execution-core startup.
     - C.5 — Rust: `code_interpreter.rs`'s Space-scoped rewrite (the
       persistent-directory path above), the new `StateStore` cache slot,
       the two release-point wiring points.
   Blocks step 4 in practice (there is no real overlay to promote without
   3.5.C existing), even though 4's own SQL/RPC design doesn't depend on it.
4. **Merge/`PromoteWorkspace`** (§4) — depends on 3.5 existing in practice
   (an overlay a real run actually produced), though its own SQL/RPC design
   only assumes 3's data shapes exist.
5. **Data Plane promotion client** (§5) — independent of 1-4 in principle
   (it only needs *some* content to promote), but sequenced last since it's
   the lowest-priority piece for "a workspace that survives a restart," which
   is this doc's actual title.

## Open questions (flagged, not resolved)

- **Conflict UX** — §4 detects a conflict at the SQL level but does not
  design what a caller does with `conflicting_paths` in the response: retry
  with a fresh hydrate, surface to a human via the Space UI (which, per this
  research, currently has **no files/workspace tab at all** — confirmed
  against `SPACE_PAGE_AUDIT_2026-09-06.md`'s full tab inventory), or something
  else. Needs product input before `PromoteWorkspace`'s response contract is
  finalized.
- **Org read-only layer's actual scope for a first cut** — per
  `capability-ownership-matrix.md`'s own stated principle ("build provisioning
  only when a concrete consumer drives it"), there is today no concrete
  consumer asking for org-wide shared files across Spaces. Recommend
  sequencing step 3 to ship **Space + run-overlay only** first, with the
  `extra_ro_binds` mechanism built generally enough to add the org layer
  later without a second design pass, rather than building an unused org
  layer now.
- **`StorageBackend` trait** — explicitly NOT proposed here, matching the
  rejected `SandboxBackend` trait precedent. `CasClient` is a concrete MinIO
  client. Revisit only if a second real object-storage backend is actually
  being built.
- **Ownership-matrix reconciliation** — "in-mem + Redis target" vs. this
  doc's Postgres-via-`pgxpool` design (§6, docs housekeeping) needs the
  matrix's own owner to confirm before merge, not just this doc's say-so.
- **Whether MinIO or a Postgres large-object/bytea approach is right for
  CAS content at all** — MinIO is what's already provisioned in
  `docker-compose.yml` and is the natural fit for content that can be large
  (build outputs, browser artifacts), but wasn't independently re-litigated
  against alternatives here; flagged as an assumption carried from the
  existing (unused) compose wiring, not a fresh evaluation.
