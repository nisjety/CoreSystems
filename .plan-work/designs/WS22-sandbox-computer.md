# WS22-sandbox-computer - Per-thread sandbox computer behind a SandboxBackend seam

Item: F-t3-009 (XL, W2). Path abbreviations: `MP` = `apps/Model Plane`, `EC` = `MP/rust/services/execution-core/src`, `MG` = `MP/rust/services/model-gateway/src`, `SM` = `MP/go/services/sandbox-manager`, `FE` = `apps/Frontend Plane/verevonv3`, `UC` = `apps/Control Plane/user-core`. Line numbers are the worktree (HEAD afb83928) unless marked "live" (live tree `C:/dev/CoresSystem`).

## 1. Scope, items resolved, owner

**Owner:** Model Plane. sandbox-manager (Go) owns lease, thread-workspace rows and lifecycle; execution-core (Rust) owns the bwrap spawn, per-step workspace sync and the future trait; model-gateway and the BFF only carry a bearer, an id and a typed event. Control Plane owns entitlements and (later) any signed decision. cost-core owns metering (later).

**IN (smallest slice first):** thread-scoped lease and a durable thread workspace on bwrap, written through after every step (this is what fixes the inline edit loss); ADR reversing the 2026-08-22 deferral; retention, quota, erasure and user reset; typed lifecycle and one SSE event; the execution-core `SandboxBackend` trait (gated); a microVM spike that ends in an ADR.

**OUT:** the microVM build itself (separate epic, not scheduled here); computer panel, desktop and browser inside a VM (F-t3-024/035/026, Quarry executes); Space workspace promotion and ACL (Space session); agent file tools (WS24, which consume this); Office deliverables (WS19); a billed compute meter (WS27 consumes counts).

| Slice | Tag | One line |
|---|---|---|
| S0 | [FINISH] | Land-first, ADR, doc drift, characterisation tests |
| S1 | [BUILD] | sandbox-manager: thread lease semantics, thread workspace store and RPCs |
| S2 | [BUILD] | execution-core: per-step open/commit sync, `WorkspaceScope`, code_interpreter wiring |
| S3 | [BUILD] | BFF + model-gateway plumbing, honest `workspace` result field, proof stack (dark) |
| S4 | [FINISH] | Space inline path: stop the silent edit loss (overlay write-through), scratch exclusion |
| S5 | [BUILD] | Retention, quota, erasure, user reset (gate for default-on) |
| S6 | [BUILD] | Typed lifecycle (`GetLeaseStatus`, failure cause) and `sandbox_state` event |
| S7 | [BUILD, gated D1] | `SandboxBackend` trait + conformance suite |
| S8 | [BUILD, gated D3/D4] | microVM spike and ADR (epic not scheduled) |

## 2. Verified current state

**What exists (code wins over the audit):**
- sandbox-manager already accepts `scope_type` "thread" (`SM/internal/server/server.go:96`) and inserts `scope_id`/`scope_type` (`SM/internal/lease/lease.go:163-166`). It is not idempotent: every `AcquireLease` mints a new lease (`lease.go:142-171`). A non-Space lease has `backend_id=""` (`server.go:114`, `lease.go:64-67`) and no workspace: `GetWorkspaceManifest` returns empty when `space_id==""` (`server.go:295-297`); `PromoteWorkspace` refuses (`:329-331`). Workspace rows are keyed `(org_id, space_id, run_id, path)` (`SM/migrations/0001_workspace_manifest.up.sql`) - there is no thread key anywhere.
- Authority: `authz.Authorize` lets any `user` principal through and requires `sandbox:read|write` scope for services (`SM/internal/authz/authz.go:81-86`); `OwnerFilter` is `ActorID` for users, "" for services (`:98-103`). Non-Space leases need no Control decision (`sandboxes.proto:118-125`). Control's decision is Space-only and requires `KindPersonal` (`UC/internal/spaces/space_capability_decision.go:138`; per WS18 and overlap-space row 11).
- Caller: the only `AcquireLease` caller passes `scope_id: run_id, scope_type: "agent"` (`EC/sandbox_lease.rs:143-144`), and only when `space_id` is set: `execute_step` demands the sandbox bearer only for a Space-scoped `code_interpreter` (`EC/grpc.rs:864-874`), `run_agent` for any Space run (`:1224-1232`), and the BFF mints it only for Space turns (`FE/apps/gateway/src/domains/chat/shared.rs:211-238`). The context is built at `grpc.rs:908-919` and `runtime_loop/agent.rs:1893-1910`.
- Inline plain-chat `code_interpreter`: `MG/tool_loop.rs:2555` -> `MG/tools.rs:163-267` -> direct `ExecuteStep`. `thread_id` is in scope at `tool_loop.rs:2472` but `ExecuteStepRequest` has no thread field (fields 1-11; `space_id`=11, `MP/proto/model_plane/v1/execution.proto:268-318`). The direct RPC passes `thread_id=""` into `execute_step_inner` (`grpc.rs:936-938`), so memory tools fail closed there; **this must not change** (section 7).
- Edit loss is real but narrower than the card says: a non-Space call runs in a per-call temp dir removed by `Drop` (`EC/code_interpreter.rs:305-348, 514-517`), so nothing persists by design. A Space inline call hydrates, caches the dir by `run_id` and has no run-end signal, so the upload at `sandbox_lease.rs:436-488` never fires (module doc `sandbox_lease.rs:17-29`). The card cites `shared.rs:205-245`; the mechanism is the `sandbox_lease.rs` doc.
- **New finding, not in the audit:** nothing calls `PromoteWorkspace` (`S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md:689-696`; grep finds only the client method and tests). A Space run's overlay is recorded under its own lease and no later run reads it. Space-scoped "durable" edits therefore do not persist across runs today, even for governed `RunAgent`.
- `workspace_hydrate::diff_and_upload` uploads every regular file under the root (`EC/workspace_hydrate.rs:176-198`), including the staged program file, `.tmp` and `.mplconfig` (`code_interpreter.rs:67,72,548-565`) and `files_in`; it cannot see deletions. `workspace_hydrate.rs` header 1-14 ("not wired") is stale.
- CAS keys are `cas/sha256:<hex>`, global, not tenant-prefixed (`EC/workspace_cas.rs:39,28`); there is no delete or GC and no Go-side S3 client. Erasure of thread content needs both.
- ZDR today is stronger than the card says: `execute_step` and `run_agent` refuse every ZDR request up front (`EC/grpc.rs:881, 1237`; live typed version at live `grpc.rs:664-676, 899, 1259`), so no ZDR code_interpreter step reaches a lease.
- `SandboxCapabilityProfile.persistence = "ephemeral"` (`EC/sandbox.rs:359`) describes the executor, not the workspace; Space workspaces already persist through CAS while claiming it. Keep it.
- No trait exists (`MpSandboxPolicy::External` only forwards a network policy, `EC/policy.rs:55-69`); no lease sweeper (lazy expiry only, `lease.go:194-196`); no failure-cause or lease heartbeat; no sandbox SSE event; no action contract in `FE/src/shared/actions`; no CI workflow runs sandbox-manager Go tests (grep of `.github/workflows` finds none; Postgres integration tests use testcontainers, `lease_integration_test.go:20-70`, and hard-code the migration list).

**[live-only] / differs-in-live (land before this stream starts; WS01 S0 owns the landing):**
| File | Status | Why it matters |
|---|---|---|
| `C:/dev/CoresSystem/MP/rust/crates/mp-contracts/src/zdr_refusal.rs` | ?? | typed `Refusal::feature_not_supported(key)`; S2 adds feature key `sandbox_workspace` to `FEATURES` (live lines 215-340) |
| `.../execution-core/tests/zdr_refusal_inventory.rs` | ?? | `NAMED_SITES` (live 15-25) gets a new named site (S2) |
| `EC/grpc.rs`, `EC/runtime_loop/mod.rs`, `EC/runtime_loop/agent.rs`, `EC/auth.rs` | M | S2/S3 edit the same hunks; live line shifts +12 to +14 in `mod.rs` |
| `MG/auth.rs`, `MG/sse.rs`, `MG/tool_loop.rs`, `MG/sse_events.rs` | M | S3/S6 |
| `FE/src/shared/api/chat-client.ts`, `MP/docs/sse-event-taxonomy.md` | M | S6 |

Everything in `SM/`, `sandboxes.proto`, `execution.proto`, `sandbox_lease.rs`, `code_interpreter.rs`, `workspace_*.rs`, `shared.rs`, `streams.rs` is clean in live.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Can proceed without |
|---|---|---|---|---|---|
| D1 | **Reverse the 2026-08-22 `SandboxBackend` deferral** (`MP/docs/decisions/ledger.md:1358-1375`; `MP/CLAUDE.md` code-style rule) | (a) trait now, as the card says; (b) new ledger entry that archives the old one, states the trigger ("a second real backend is planned": the microVM), and lands the trait in S7 together with the first remote implementation | **(b).** The ledger trigger is "second backend actually being built". S1-S6 deliver the user value with no trait. S0 writes the entry now so the reversal is explicit, never silent | S7 | S0-S6 |
| D2 | **Authority for a thread workspace.** Card: "its own Control-signed decision" | (a) delegated user-bound sandbox bearer, owner = `ActorID`, org flag + quota; (b) new Control decision `model.thread.sandbox_workspace` plus `thread_sandbox_entitled`. Control has no evidence for a non-Space thread (threads live in session-core) | **(a) for S1-S5 behind flags, default off. (b) required before default-on for an org and before any billed or persistent compute backend (S8).** Reason: (a) is user-private, org-scoped, owner-keyed, resettable; the alias risk is closed structurally (section 4 S1), and Space access is never widened. Owner must rule because it departs from the card | S3 default flip; S8 | S1, S2, S4, S6 |
| D3 | Thread, Space, or both (audit 10.2 item 8) | thread only / both | Thread workspace now. Space inline gets write-through only (S4); Space cross-run persistence needs a promote caller and ACLs = Space session | S4 scope | S1-S3 |
| D4 | microVM provider, region, cost, guest-image owner, VM-second metering | self-hosted Firecracker + executor pair / attested managed provider | Not decided here; S8 spike produces the ADR. Nothing in S1-S7 depends on it | S8 | S1-S7 |
| D5 | Retention and erasure of thread data | idle TTL, user reset, erase on thread delete, CAS prefix per thread | 30 days idle default, user "reset workspace", erase on thread delete (hook from WS17), blobs under `cas/thread/<org>/<thread>/` so delete is a prefix delete. **Default-on is blocked until S5 ships** | default flip | S1-S4 |
| D6 | What is persisted | everything / syncable subset | Exclude program file, `.tmp/`, `.mplconfig/`, `__pycache__/`, `attachments/` (WS08 reserved), anything over 8 MiB per file; cap 256 files and 64 MiB per thread | S2 | S1 |
| D7 | `code_interpreter` effect class once state persists (WS05) | stay `sandbox_compute` / `context_write` | Stay `sandbox_compute`: user-private, resettable, no external effect | none | all |
| D8 | ZDR | refuse / ephemeral | Refuse (today's behaviour). If the ZDR owner later un-refuses code_interpreter, it runs the per-call ephemeral path with no lease, no CAS, no manifest | none | all |

## 4. Slices (in build order)

Common flags (family vocabulary `enabled`, as `EXECUTION_CORE_PROCESS_HOST`, `EC/sandbox.rs:327-338`; none contains KEY/TOKEN/SECRET so `release-artifact.sh:732-753` needs no audit): `SANDBOX_MANAGER_THREAD_WORKSPACE`, `EXECUTION_CORE_THREAD_WORKSPACE`, BFF `SANDBOX_THREAD_WORKSPACE_ENABLED` (strict true/false, default false, precedent WS21 `BROWSER_DO_ENABLED`). Compose: `MP/deploy/docker-compose.yml` execution-core block 1005-1280 (near 1184), sandbox-manager 1575-1611. Rollout is the repo's: default off, proof stack, then flip; rollback is setting off (rows stay, unreachable). Rebuild recipe for every evidence run: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build sandbox-manager execution-core model-gateway`, confirm containers run the new images, drive the flow through the BFF.

### S0 [FINISH] Land-first, ADR, doc drift, characterise (S, 2-3 d)
**Goal.** Reversal is explicit; stale docs fixed; today's behaviour pinned.
- `MP/docs/decisions/ledger.md` - NEW entry at top (newest first, ~line 25): "2026-10-15 - Per-thread sandbox computer: thread lease and thread workspace now; SandboxBackend seam at microVM build start". Mark the 2026-08-22 entry `archived` with a forward pointer (header rule, lines 1-20). Cover: ZDR snapshot destruction, no alias or bypass of Space ACLs, D2 authority, guest-image owner and VM-second metering listed as open for S8.
- `MP/docs/capability-ownership-matrix.md` - fix stale rows 66 ("GENUINE GAP - nothing exists" vs shipped bwrap) and 126 vs 138 (`MpSandboxPolicy`); add row "Thread workspace rows: sandbox-manager".
- `EC/workspace_hydrate.rs` - correct the stale "not wired" header (1-14). Comment-only.
- NEW `EC/runtime_loop/` test (in `mod.rs` tests): `direct_execute_step_still_passes_empty_thread_id_to_memory_tools` pinning `grpc.rs:936-938`.
- NEW `.github/workflows/sandbox-manager-service.yml` modelled on `session-core-service.yml` (`go vet ./...`, `go test ./...`; integration job with Docker for `-tags integration`). Closes the no-CI gap.
**Proto/migration/flag.** None. **ZDR.** None. **Tests.** `cd "MP/go/services/sandbox-manager" && go vet ./... && go test ./...`; `cd "MP/rust" && cargo test -q -p execution-core --lib`. **DoD.** ledger entry merged; workflow green on a PR. **Evidence.** CI run link. **Deps.** WS01 S0 landing live files. **Risks.** none.

### S1 [BUILD] sandbox-manager: thread lease and thread workspace (L, 7-10 d)
**Goal.** A user-owned, idempotent thread lease and a durable, optimistic-concurrency thread workspace that cannot alias a Space.
**Files**
- `MP/proto/model_plane/v1/sandboxes.proto` - `AcquireLeaseRequest` field 8 `bool zdr` (next free; fields 1-7 verified, lines 105-132): a thread-scope acquire with `zdr=true` is refused (defence in depth). New RPCs appended after `Health`: `GetThreadWorkspace(GetThreadWorkspaceRequest{lease_id=1, backend_id=2}) -> {repeated WorkspaceManifestEntry entries=1 (reuse; adds size via new message ThreadWorkspaceEntry{path=1, content_hash=2, size_bytes=3} instead, to carry sizes), int64 revision=2}`; `ApplyThreadWorkspaceChanges({lease_id=1, backend_id=2, repeated WorkspaceChangedFile changes=3 (reuse, base_hash=CAS precondition), repeated ThreadDeletedPath deleted=4{path=1, base_hash=2}}) -> {repeated string conflicting_paths=1, int64 revision=2, int32 file_count=3, int64 total_bytes=4}`; `DeleteThreadWorkspace({thread_id=1, org_id=2}) -> {int32 files_deleted=1, string object_prefix=2}` (S5). Doc update on `AcquireLease`: thread scope is get-or-create with sliding expiry. Regenerate Go (`MP/go/gen/model_plane/v1/sandboxes.pb.go`, `sandboxes_grpc.pb.go`, committed) and Rust (`MP/rust/crates/mp-contracts/build.rs`) via `buf generate`.
- `SM/internal/authz/authz.go` - add the new methods to the switch (write: `ApplyThreadWorkspaceChanges`, `DeleteThreadWorkspace`; read: `GetThreadWorkspace`) in the same commit, plus rows in `authz_test.go`; an unlisted method is refused for everyone (comment at `authz.go:53-59`).
- `SM/internal/lease/lease.go` - NEW `AcquireThread(ctx, threadID, orgID, ownerID, backendID, ttl)` = `INSERT ... ON CONFLICT (org_id, owner_id, scope_id) WHERE scope_type='thread' AND state<>4 DO UPDATE SET expires_at=GREATEST(...)` (revives an expired lease; it carries no compute). Stamps `backend_id` = `SANDBOX_MANAGER_BACKEND_ID` (`cmd/main.go:60`) so thread leases are pinned.
- `SM/internal/server/server.go` - `AcquireLease` thread branch before line 113: require `PrincipalType=="user"`, `space_id==""` (else InvalidArgument "a thread lease cannot carry a space_id"), `!zdr`, `SANDBOX_MANAGER_THREAD_WORKSPACE=enabled`, a durable store; use `AcquireThread`. Thread RPC handlers refuse any lease with `SpaceID!=""` or `ScopeType!="thread"`. The Space RPCs (`GetWorkspaceManifest`, `PromoteWorkspace`, `SnapshotSandbox` overlay) never read the thread table.
- `SM/internal/server/store.go`, `types.go`, `memory_store.go` - `ThreadWorkspaceStore` interface; in-memory fake for server tests; in ephemeral-development mode (no DB, `cmd/main.go:42,63`) thread RPCs are refused like the process RPCs.
- NEW `SM/internal/threadworkspace/store.go` (+ `store_test.go`, `store_integration_test.go`) - transaction per apply, per-path compare-and-swap on `base_hash` (absent base = must not exist), deletes with base check, caps from env `SANDBOX_MANAGER_THREAD_MAX_FILES` (256) and `..._MAX_BYTES` (64 MiB), returns conflicts instead of overwriting ("never last-writer-wins", S3.3 rule).
- `SM/cmd/main.go` - construct the store and wire `WithThreadWorkspaceStore`.
- `MP/deploy/docker-compose.yml` - `SANDBOX_MANAGER_THREAD_WORKSPACE: ${...:-}` (default empty = off).
- `SM/internal/lease/lease_integration_test.go` and the workspace integration test - add the new migration to their hard-coded lists.
**Migrations (names only; integrator assigns numbers; idempotent, the entrypoint psql loop has no ledger, `SM/docker-entrypoint.sh:15-17`).** `<next>_thread_workspace.sql`: `thread_workspace_files(org_id, owner_id, thread_id, path, content_hash, size_bytes, updated_at, PRIMARY KEY(org_id, owner_id, thread_id, path))`, `thread_workspaces(org_id, owner_id, thread_id, revision, file_count, total_bytes, last_used_at, created_at, PRIMARY KEY(org_id, owner_id, thread_id))`; `<next>_thread_lease_unique.sql`: the partial unique index on `leases`. Both have `.down.sql`.
**ZDR.** Persists thread path, hash, size, owner. A `zdr` acquire is refused; nothing is written on a refusal. Golden-matrix case in S2 (execution-core is the ZDR boundary). **Alias guard tests (NEW, `server_test.go`).** `TestThreadLeaseRejectsSpaceID`, `TestThreadWorkspaceRPCsRefuseSpaceLease`, `TestSpaceManifestNeverReadsThreadRows`, `TestAnotherUserCannotAcquireOrReadAThreadLease`, `TestServicePrincipalCannotAcquireThreadLease`, `TestAcquireThreadIsIdempotentAndSlides`, `TestApplyReportsConflictWithoutOverwriting`, `TestApplyEnforcesQuota`.
**Tests.** `cd "MP/go/services/sandbox-manager" && go vet ./... && go test ./... && go test -tags integration ./internal/...` (Docker needed); `buf lint "apps/Model Plane/proto"` (known debt, no new findings) and `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"` (additive only).
**DoD.** All above green; with the flag off every new RPC returns FailedPrecondition. **Evidence.** Rebuild sandbox-manager; `grpcurl` with a user bearer: acquire twice -> same `lease_id`; apply two writers with a stale `base_hash` -> one conflict; row dump of both tables; second user gets NotFound. **Deps.** S0. **Risks.** Postgres get-or-create race (covered by the unique index and `ON CONFLICT`); proto regen conflicts (append-only).

### S2 [BUILD] execution-core: per-step open/commit workspace sync (L, 8-10 d)
**Goal.** Every non-Space `code_interpreter` call sees the thread's files and writes its changes back before returning; no run-end signal needed.
**Files**
- `MP/proto/model_plane/v1/execution.proto` - `ExecuteStepRequest` field 12 `string thread_id` (next free after `space_id`=11). Used only for the workspace; **not** forwarded into `execute_step_inner`'s `thread_id` (stays `""`).
- `EC/sandbox_lease.rs` - generalise `SandboxLeaseContext` with `scope: WorkspaceScope { Space{space_id}, Thread{thread_id} }`; `capability_client` is needed only for Space. NEW `ensure_thread_lease` (acquire, cached in `StateStore` by run_id like `sandbox_lease.rs:113-115, 162`, `scope_id=thread_id`, `scope_type="thread"`, `zdr=false`).
- NEW `EC/workspace_sync.rs` - `open(ctx, lease) -> OpenWorkspace` (manifest via `GetThreadWorkspace`, `workspace_hydrate::hydrate` into a fresh dir with a `Drop` guard, per-thread in-process mutex), `commit(open, baseline) -> WorkspaceReport{scope, state, conflicts, cause}` using `diff_and_upload` filtered by `is_syncable` (D6) plus new `diff_deletions`, then `ApplyThreadWorkspaceChanges`. A conflict is reported to the model, never overwritten.
- `EC/workspace_hydrate.rs` - add `diff_deletions(baseline, dir)` and an exclusion predicate; keep `diff_and_upload` signature for the Space path.
- `EC/workspace_cas.rs` - key prefix parameter (`put_scoped/get_scoped` with `cas/thread/<org>/<thread>/`); default behaviour unchanged for Space.
- `EC/sandbox_manager_client.rs` - client methods for the two new RPCs.
- `EC/runtime_loop/mod.rs` (arm at 597-623) - thread branch: lease -> open -> `execute_code_interpreter(.., Some(root))` -> commit -> merge `WorkspaceReport` into the result JSON (`"workspace": {...}`). Space branch unchanged here.
- `EC/grpc.rs` - `execute_step`: demand `authenticate_delegated_sandbox_manager` also when `thread_id` is non-empty, `space_id` empty and the flag is on (extends `grpc.rs:864-874`); `run_agent`: build the thread context when `space_id` empty and `thread_id` present (`:1224`); `EXECUTION_CORE_THREAD_WORKSPACE` read once at startup.
- `EC/runtime_loop/agent.rs` (1893-1910) - build the thread context for `code_interpreter` when the run is non-Space.
- `EC/code_interpreter.rs` - expose the program-file and scratch names for the exclusion predicate (no behaviour change); result JSON gains `workspace`.
- `MP/rust/crates/mp-contracts/src/zdr_refusal.rs` [live-only] - add `FEATURES` entry `sandbox_workspace`. `EC/../tests/zdr_refusal_inventory.rs` [live-only] - add `("sandbox_lease.rs", "thread workspace zdr guard")` to `NAMED_SITES`.
**Flag.** `EXECUTION_CORE_THREAD_WORKSPACE` (default off); off = today's per-call ephemeral path byte for byte. **ZDR.** `ThreadWorkspace::open` refuses with `Refusal::feature_not_supported("sandbox_workspace")` when `zdr` (unreachable today because `execute_step` refuses first; the guard survives a later ZDR un-refuse). Nothing hydrated or uploaded under ZDR. Golden-matrix case: add an enforcement row in `MP/api/contracts/zdr-posture-matrix.json` (re-pin the `.sha256`; run `bash scripts/tests/zdr-contract-sync.test.sh`) and a test `thread_workspace_refused_under_zdr`.
**Tests (NEW names).** In `workspace_sync.rs`: `second_call_sees_first_calls_file`, `deleted_file_is_recorded`, `scratch_and_program_files_are_not_synced`, `conflict_is_reported_not_overwritten`, `drop_removes_the_directory_on_every_path`, `files_over_quota_are_refused_with_a_typed_cause`; in `grpc.rs`: `thread_scope_demands_the_sandbox_bearer_only_when_enabled`, `space_scope_never_takes_the_thread_path`. Run `cd "MP/rust" && cargo test -q -p execution-core --all-targets` (bwrap tests are Linux-gated; on Windows use the `rust:1` container recipe from memory, or `--lib`) and `cargo test -q -p execution-core --test zdr_refusal_inventory`; `bash "apps/Model Plane/scripts/verify-sandbox-isolation.sh"` live.
**DoD.** Turn 2 of a thread reads what turn 1 wrote through two different `run_id`s; restart of execution-core between turns loses nothing. **Evidence.** Section 4 S3 flow. **Deps.** S1 (proto regen), S0. **Risks.** Per-step hydrate cost (mitigate with the 256/64 MiB cap and, if measured slow, an on-disk blob cache keyed by hash); `grpc.rs`/`mod.rs`/`agent.rs` are live-modified, so rebase on the landed live wave.

### S3 [BUILD] BFF and model-gateway plumbing, honest result, proof stack (M, 4-6 d)
**Goal.** The inline and Do paths reach S2 end to end with the flag on, dark by default.
- `FE/apps/gateway/src/domains/chat/shared.rs` (`sandbox_token`, 230-246) - also mint the `aud=sandbox-manager` bearer when `SANDBOX_THREAD_WORKSPACE_ENABLED` and the turn is non-ZDR (same source as WS21's `browser_token`); extend `is_space_scoped_turn` callers only, not its meaning. `streams.rs:112,126` already pass it on.
- `MG/tools.rs` (`handle_code_interpreter`, 163-267) - new `thread_id` parameter -> `ExecuteStepRequest.thread_id`; `MG/tool_loop.rs` (2555) passes `thread_id`; the refusal at 2546 stays Space-only. No bearer under thread mode = degrade to ephemeral, and the result says so.
- `MG/tools.rs` / `tool_loop.rs` - include `workspace` in the tool output the model sees (`"workspace":{"scope":"thread|space|ephemeral","state":"synced|conflict|refused","conflicts":[...]}`); an ephemeral result states "files are not kept after this call" so the model never assumes persistence. Output provenance stays `unscreened` (`tool_loop.rs:2578-2581`).
- `MG/sse.rs` (`spawn_run_dispatch` 8302-8346, `agentic_run_stream` 9167-9246) - no change expected beyond passing the bearer already present; verify only.
- `MP/deploy/docker-compose.yml` - pass the three flags to execution-core, sandbox-manager and the BFF env block.
**Tests.** `cd "MP/rust" && cargo test -q -p model-gateway --lib tools` and `-p model-gateway --all-targets`; `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`. NEW: `code_interpreter_forwards_thread_id_to_execute_step`, `ephemeral_result_declares_no_persistence`, `bff_mints_sandbox_bearer_for_a_personal_turn_only_when_enabled_and_not_zdr`.
**ZDR.** No bearer minted, no thread id sent for a ZDR turn; golden case from S2 reused. **DoD / Evidence (the deployment claim).** Flags on, rebuild all four images. Via the BFF: (1) turn 1 "create notes.txt with code_interpreter", turn 2 (new run, same thread) "print notes.txt" -> content returned; capture both SSE transcripts, the two `workspace` fields, and `SELECT path,content_hash FROM thread_workspace_files`. (2) Second user, same org: empty workspace. (3) A Space thread: no row appears in the thread tables. (4) ZDR turn: typed refusal. (5) Restart execution-core between turns: still readable. **Deps.** S2. **Risks.** An auth-core round trip per non-ZDR turn (whether `get_model_service_token` caches is not determined; measure, and mint lazily only for turns whose tool list includes `code_interpreter` if it costs).

### S4 [FINISH] Space inline path: stop the silent loss (M, 3-4 d)
**Goal.** A Space-scoped inline step no longer drops its edits, using the same `workspace_sync` with `WorkspaceScope::Space`.
- `EC/workspace_sync.rs` - Space variant: commit via `SnapshotSandbox` with `changed_files` (existing overlay path, `sandbox_lease.rs:495-550`), after each step instead of at run end; fix the over-upload with `is_syncable`.
- `EC/sandbox_lease.rs` - docs: update the "known gap" module doc (17-29); `release_sandbox_lease_if_any` stays for agent runs and cancel (`grpc.rs:1149`, `agent.rs:984`).
- `EC/runtime_loop/mod.rs` - Space arm uses open/commit when `EXECUTION_CORE_SPACE_WORKSPACE_WRITE_THROUGH=enabled` (default off).
**Honest limit (state it in the ledger):** with no promote caller the overlay is still visible only to its own lease. This slice stops the loss and records the overlay; making it cross-run visible is the Space session's decision (section 6). **ZDR.** Unchanged (refused earlier). **Tests.** `space_overlay_is_recorded_after_each_inline_step`, `space_path_flag_off_is_byte_identical`; `go test` untouched. **Evidence.** Space turn, query the overlay rows by lease. **Deps.** S2. **Risks.** Looks like a persistence fix; mitigated by the ledger note and the `workspace.scope="space"` field.

### S5 [BUILD] Retention, quota, erasure, user reset (L, 8-12 d) - the gate for default-on
**Goal.** Persistent user data has an end: idle expiry, a user control and erasure on thread delete.
- `SM/internal/threadworkspace/sweeper.go` (NEW, patterned on `SM/internal/process/sweeper.go:24-27,99-121`, wired like `cmd/main.go:131-153`): delete workspaces idle past `SANDBOX_MANAGER_THREAD_RETENTION_DAYS` (30) and old destroyed or expired lease rows (no lease sweeper exists today).
- `SM/internal/server/server.go` - `DeleteThreadWorkspace`; deletes rows then blobs.
- NEW Go S3 delete client in `SM/internal/objectstore/` (`GOWORK=off` for the new dependency, per repo note), prefix delete of `cas/thread/<org>/<thread>/`; or, if D5 prefers, `DeleteThreadWorkspace` returns the prefix and execution-core deletes (Rust `CasClient` gains `delete_prefix`). Pick one in S5; recommend the Go client so erasure does not depend on a Rust hop.
- `MG/http_routes.rs` (route block near 349-350) - `GET /v1/threads/{thread_id}/workspace` (list: path, size) and `DELETE` (reset), user bearer, owner-bound.
- `FE/apps/gateway/src/domains/chat/` - BFF routes; `FE/src/shared/actions/` - NEW `chat-workspace.ts` action contract (`chat.workspace.list`, `chat.workspace.reset`) **before** UI wiring (repo rule); SPA: a "Thread files / Reset" entry in the existing chat panel (`FE/src/features/chat/components/`), Norwegian and English strings.
**Migration:** none beyond S1 (indexes on `last_used_at` go in `<next>_thread_workspace.sql`). **ZDR.** Erasure is the ZDR-adjacent contract: a deleted thread leaves no rows and no blobs; test asserts both. **Tests.** `sweeper_test.go`, `TestDeleteThreadWorkspaceRemovesRowsAndPrefix`, integration with MinIO container; gateway route tests; `pnpm exec vitest run` for the action contract and panel. **Evidence.** Create files, DELETE via BFF, show empty tables and an empty MinIO prefix (`mc ls`); advance the clock past retention in a test stack. **Deps.** S3; WS17 for the thread-delete hook (section 5). **Risks.** Blob GC correctness; thread-delete hook owned elsewhere (until it exists, retention is the only erasure, so default-on waits).

### S6 [BUILD] Typed lifecycle and the visible computer state (M, 5-7 d)
**Goal.** Lease state, failure cause and activity are typed and visible.
- `sandboxes.proto` - `enum SandboxFailureCause { UNSPECIFIED=0; BACKEND_UNAVAILABLE=1; START_TIMEOUT=2; HEARTBEAT_LOST=3; WORKSPACE_HYDRATE_FAILED=4; WORKSPACE_COMMIT_FAILED=5; QUOTA_EXCEEDED=6; WORKSPACE_CONFLICT=7; EXPIRED=8; POLICY_DENIED=9 }`; `GetLeaseStatus({lease_id=1, backend_id=2}) -> {lease_id=1, state=2 SandboxLifecycleState, failure_cause=3, last_activity_at=4, heartbeat_at=5, expires_at=6, scope_type=7, file_count=8, total_bytes=9}`. `HEARTBEAT_LOST`/`heartbeat_at` are reserved for a persistent backend (S8); bwrap leaves them unset.
- Migration `<next>_lease_lifecycle.sql`: `leases.failure_cause SMALLINT NOT NULL DEFAULT 0`, `last_activity_at`, `heartbeat_at`. `SM/internal/lease/lease.go` scan/insert, `server.go` handler, `authz.go` switch.
- `MP/rust/crates/mp-contracts/src/lifecycle_notice.rs` (NEW by WS15 S1; if not landed, create the closed-token module there) - tokens `SANDBOX_STATES=[kept, ephemeral, syncing, conflict, refused, unavailable]`, causes as above.
- `MG/sse_events.rs` - one `SandboxState{scope,state,cause}` variant inside WS15's fenced block, name `sandbox_state`, family steps; emitted from the `workspace` field of the tool result (counts and closed tokens only, never paths or content). `MP/docs/sse-event-taxonomy.md` regenerate (`python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py"` then `--check`).
- `FE/src/shared/api/chat-client.ts` (`case 'sandbox_state'`, add-only), `FE/src/shared/chat/verevon-ui-events.ts`, `FE/apps/gateway/src/domains/ag_ui.rs` CUSTOM list; a small chip "Workspace: kept / not kept / conflict" in the lifecycle-notices node.
**ZDR.** Frame has no content; ZDR turns never reach it. **Tests.** `lease_test.go`, `sandbox_state_frame_contract`, `chat-client.test.ts`, taxonomy `--check`. **Evidence.** SSE transcript with the frame for kept, conflict and refused (over-quota) cases. **Deps.** S3, WS15 S1. **Risks.** `sse_events.rs` is a hot file (5 streams): fenced block, second lander regenerates golden.

### S7 [BUILD, gated D1] SandboxBackend trait and conformance suite (L, 6-8 d)
**Goal.** One seam over spawn and lifecycle, with bwrap as implementation 1 and a remote-executor client as implementation 2; built when D1 releases it.
- NEW `EC/sandbox_backend.rs` - `trait SandboxBackend { fn id(&self)->&str; fn profile(&self)->SandboxCapabilityProfile; async fn exec(&self, ExecSpec)->Result<ExecOutcome,BackendError>; async fn spawn(&self, SpawnSpec)->Result<ProcessHandle,BackendError>; async fn health(&self)->BackendHealth }`, `BackendError` carrying a `SandboxFailureCause` (S6). Impl `BwrapBackend` wraps `executor::execute_sandboxed_in_dir` (`EC/executor.rs:250-266`) and `process_host.rs:428`; `RemoteExecutorBackend` is a client for the executor protocol (typed requests, deadline, heartbeat).
- `EC/code_interpreter.rs`, `EC/process_host.rs`, `EC/http_health.rs` (profile and backend id, 51-52, 143-144) call the trait; `MpSandboxPolicy::External` (`policy.rs:55-69`) routes to the remote impl; ro binds (WS08 `extra_ro_binds`) must be refused on a backend that cannot honour them.
- NEW `EC/tests/backend_conformance.rs` - one suite run against every impl (hermetic fs, no network, timeout kill, scrubbed output, ro bind refusal, typed cause), modelled on the contract-test approach in `MP/go/services/nats-provisioner/manifest_contract_test.go:347,418` (pattern only; bodies not read).
**Flag.** none (internal refactor); behaviour unchanged. **ZDR.** Unchanged. **Tests.** `cargo test -q -p execution-core --all-targets`. **Evidence.** Existing isolation proof unchanged: `verify-sandbox-isolation.sh`. **Deps.** S6; D1. **Risks.** Premature abstraction if no second implementation follows; the conformance fixture must be a real executor stub, and the slice does not start before D1.

### S8 [BUILD, gated D3/D4] microVM spike and ADR (M spike, 5-8 d; epic XL, not scheduled)
Spike a second attested executor plus sandbox-manager pair with its own `backend_id`, idle-stop and encrypted snapshot, then write the ADR: provider, region, cost, guest-image owner, patch and supply-chain duty, snapshot retention and destruction, VM-second metering with cost-core, and the Control decision (D2 option b) that gates it. No code ships. Quarry executes any browser inside the VM under Quarry-brokered grants.

## 5. Interfaces

**PROVIDES**
| To | Contract |
|---|---|
| WS24 (file tools) | thread workspace RPCs (`GetThreadWorkspace`, `ApplyThreadWorkspaceChanges`), `WorkspaceScope::Thread`, `workspace_sync::{open,commit}`; file tools must go through them, never a raw directory |
| WS03 (durable loop) | per-step write-through means a restart or pause loses no committed thread edit; closer `WorkspaceStateLost` applies only to the Space overlay. Pause leaves nothing cached for thread scope |
| WS15 | closed tokens and one `sandbox_state` event (S6) |
| WS27 | counts only: `file_count`, `total_bytes`, later VM-seconds; no content |
| WS19 | files written in the thread workspace are listable; Office outputs still return through the `files` result |
| WS05 | `code_interpreter` stays `sandbox_compute` (D7); `process_*` unchanged |
| WS17 / WS07 | `DELETE /v1/threads/{id}/workspace` as the erasure hook |
| F-t3-024/035 later | the `SandboxBackend` trait and `SandboxFailureCause` |

**CONSUMES**
| From | Contract |
|---|---|
| WS01 S0 | live ZDR wave committed (files in section 2) |
| WS08 | reserved name `attachments/` and `extra_ro_binds` read-only mount; excluded from sync |
| WS15 S1 | `lifecycle_notice.rs` and the fenced block in `sse_events.rs` |
| WS17 | thread-delete event or call into S5's route |
| WS10 / WS25 | optional org entitlement and `org_agent_policy` key for D2 (b); until then env flags |
| WS04 | execution-core state stays in-memory per run; the thread path is stateless per step, so replica-safe by construction |
| WS20 | scheduled runs mint no sandbox bearer, so they get the ephemeral path |

**Conflict hot-spots and edit order**
| File | Others | Rule |
|---|---|---|
| `EC/grpc.rs`, `EC/runtime_loop/mod.rs`, `EC/runtime_loop/agent.rs` | WS02, WS03, WS04, WS05, WS24, WS25 | land the live wave first (WS01 S0), then WS03; WS22 hunks are call-ins to `workspace_sync.rs` |
| `sandboxes.proto`, `execution.proto` | WS03 (execution), WS13/14/17 (sessions) | append-only; integrator assigns field numbers (next free verified: `AcquireLeaseRequest`=8, `ExecuteStepRequest`=12) |
| `MG/sse_events.rs`, `chat-client.ts` | WS02, WS04, WS12, WS13, WS14, WS15, WS27 | WS15 first; S6 is last in that file |
| `MG/tool_loop.rs`, `MG/tools.rs` | WS02, WS05, WS25 | small hunks; WS02 first |
| `MP/deploy/docker-compose.yml`, ledger, `zdr-posture-matrix.json` | all | additive rows; second lander re-pins the sha |

## 6. Cross-session interfaces

- **Space session (a), F-t1-021/028 (agent computer confinement, persistent per-agent computer).** Model Plane owns lease, lifecycle and the backend seam (overlap-space row 11). WS22 assumes the Space session delivers: (1) the Control "shared-Space decision family" so shared-room sandbox decisions can be signed (today `KindPersonal` only, `space_capability_decision.go:138`); (2) a ruling on promote: who calls `PromoteWorkspace` and with which ACL check; (3) CPU, memory and pid caps and audience-bound egress as claims in the signed decision, not new lease fields. WS22 will not add a per-agent home; if F-t1-028 wants one it should be a new `scope_type` on the same lease model (one lease model, decided once), keyed by (Space, agent), through the same store interface and not through `thread_workspace_files`. Rule: thread rows and Space rows never share a table or an RPC.
- **Quarry/browser session (b).** No shared piece in S0-S7. In S8, a browser inside a VM uses the broker grant flow (WS21); Model proposes, Quarry executes. WS22 assumes WS21's browser bearer and consent vocabulary land first (the computer track order -009, -035, -024).

## 7. Regression guards (audit section 8)

| Strength | Touched how | Proof |
|---|---|---|
| Kernel-enforced sandbox and Space CAS workspaces; never lets a thread workspace bypass a Space ACL | bwrap argv untouched; thread data in separate table and RPCs | S1 alias tests; `cargo test -p execution-core` sandbox argv tests; `verify-sandbox-isolation.sh` live; `space_path_flag_off_is_byte_identical` |
| ZDR by construction | new store refuses under ZDR; typed refusal; golden row | `thread_workspace_refused_under_zdr`, `zdr_refusal_inventory.rs`, `zdr-contract-sync.test.sh` |
| Two loops separate | thread context built per loop; no shared dispatch | `direct_execute_step_still_passes_empty_thread_id_to_memory_tools` (S0) |
| Signed capability gate | `code_interpreter` still passes `evaluate_with_evidence` before the sandbox arm (`mod.rs:488-539`); no tool added in S1-S6 | existing gate tests; S7 adds none |
| Credential isolation | sandbox env cleared, bearer user-bound; scrub on upload (`workspace_hydrate.rs:146-151`) | existing `child_env` tests; `scratch_and_program_files_are_not_synced` |
| Honest outcomes | the result states `ephemeral` vs `thread`; conflicts surfaced, never hidden | `ephemeral_result_declares_no_persistence` |
| Typed ChatEvent taxonomy | one new event through the CI gate | taxonomy `--check`, `chat-client.test.ts` |
| Multi-tenant GDPR erasure | erasure path for the new store | S5 delete tests |

## 8. Effort and risks

**Total (without the microVM epic):** S0 2-3 + S1 7-10 + S2 8-10 + S3 4-6 + S4 3-4 + S5 8-12 + S6 5-7 + S7 6-8 = about 43-60 engineer days; S8 spike 5-8 d. **Critical path:** S0 -> S1 -> S2 -> S3 -> S5 (default-on). S4, S6 follow S2/S3 in parallel; S7 waits for D1.

**Top risks**
1. **Persistent user data without an erasure path.** Mitigation: default-on blocked on S5; CAS blobs under a per-thread prefix; thread-delete hook negotiated with WS17.
2. **Authority departs from the card (D2).** A thread workspace without a Control decision. Mitigation: owner-bound user bearer, structural no-alias, flags off by default, an explicit owner ruling before default-on, Control decision required before any billed compute.
3. **Per-step hydrate cost and per-turn token mint.** Mitigation: 256 files / 64 MiB cap, optional blob cache, lazy bearer minting if measured costly.
4. **Hot-file conflicts and the unlanded live wave** (`grpc.rs`, `mod.rs`, `agent.rs`, `sse_events.rs`). Mitigation: WS01 S0 first; WS22 hunks are call-ins; the 2 new proto fields are append-only.
5. **One-implementation trait and an unfunded microVM.** Mitigation: S7 gated on D1 with a real executor stub; S8 ends in an ADR, not code; the ledger entry says so.
6. **Silent assumption of persistence in Space** (no promote caller). Mitigation: S4 states the limit in the ledger and in the `workspace.scope` field; the Space session owns the fix.
