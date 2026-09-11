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
3. **Hydrate/diff wiring in execution-core** (§3) — depends on 1 (CAS) and 2
   (manifest rows to hydrate from).
4. **Merge/`PromoteWorkspace`** (§4) — depends on 3 existing (an overlay to
   promote).
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
