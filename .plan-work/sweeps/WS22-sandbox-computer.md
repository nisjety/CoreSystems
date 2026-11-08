# WS22-sandbox-computer: code map for F-t3-009

Mechanical, read-only sweep, 2026-10-15. Audit item: F-t3-009 "Per-thread sandbox computer behind a SandboxBackend seam". Scope note: smallest slice is a thread-scoped lease and workspace on bwrap (also fixes the inline ExecuteStep edit loss); then an ADR reversing the 2026-08-22 ledger deferral; then the execution-core trait and typed lifecycle; microVM later. Must not alias Space workspaces; ZDR runs stay ephemeral. Overlaps sibling Space items F-t1-021 and F-t1-028.

**Conventions.** W = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). L = live tree `C:/dev/CoresSystem` (same HEAD; `.plan-work/live-tree-status.txt` has 324 entries: 197 M, 122 ??, 5 D). Paths are repo-relative. Line numbers are W unless marked "live". Live status: clean = absent from the status list; M = modified in L; ?? = exists only in L; D = deleted in L. Verdicts: MATCH, DRIFT (new range given), ABSENT, PARTIAL.
Audit doc: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` (card lines 468-482).

## 0. Summary

- Core premises hold: sandbox-manager is a Postgres lease, snapshot and process-registry service that pins each lease to a backend_id (lease.go:142-159; sandboxes.proto:122-148). execution-core runs bwrap per call plus an opt-in process host (sandbox.rs:279-366; process_host.rs:1-21). The Space-scoped CAS workspace is wired (sandbox_lease.rs:1-29, 436-488). The SandboxBackend trait is deferred in the ledger (ledger.md:1358-1375).
- Four card claims do not hold as cited: the lifecycle enum is at sandboxes.proto:97-103; the inline ExecuteStep edit-loss evidence is at sandbox_lease.rs:17-29, not shared.rs:205-245; there is no "lease sweeper"; and there is no SandboxBackend trait, no remote executor, no cost-core VM metering, no lease failure-cause field and no lease heartbeat.
- Thread scope is not wired on the caller side. The only caller acquires leases with scope_type "agent" keyed by run_id (sandbox_lease.rs:143-144). sandbox-manager already accepts "thread" (server.go:96). Non-Space turns mint no sandbox token (gateway shared.rs:236-238). Control's sandbox decision and sandbox-manager's verifier are Space-only.
- Live tree: every card-cited file is clean. Execution-core grpc.rs, runtime_loop/mod.rs, runtime_loop/agent.rs, auth.rs and workspace_promote.rs are M, and so are several model-gateway files (sse.rs, tool_loop.rs, http_routes.rs, auth.rs, zdr_precheck.rs). zdr_posture.rs and zdr_features.rs are ?? (not in W).
- Still open (audit 9.3 item 1, 10.2 item 8): thread vs Space workspace scope, microVM provider, region, cost model, VM-second metering, guest image owner.

## 1. Audit cross-references

| Source | Location | Content |
|---|---|---|
| Card | audit 468-482 | Priority, Partial, XL, Model Plane + Control Plane + cost-core metering, W2, confidence Medium (line 472) |
| 9.1 priority table | audit 1559 | XL; deps F-t3-002, "ADR sandbox backend", VM metering |
| 9.1 W2 goal text | audit 1553 | W2 includes "the first phase of the sandbox-computer seam" |
| 9.3 ADR candidate 1 | audit 1683 | Options: bwrap plus thread workspace (about M), or SandboxBackend seam plus microVM. ADR must reverse the 2026-08-22 deferral, show ZDR snapshot destruction, prove no alias or bypass of Space ACLs, name guest-image owner, define VM-second metering |
| 9.4 dependency notes | audit 1699 | Computer track strictly ordered: -009 then -035 then -024; -043, -084, -085, -087 wait for -009 |
| 10.2 open question 8 | audit 1731 | Self-hosted microVM vs attested managed provider; region and cost; thread, Space or both. Open |
| Appendix B row | audit 1827 | partial, XL, Model Plane, Control Plane, W2; references "Perplexity SPACE and Manus" |
| Appendix C entry | audit 1990-1998 | Reference check partial, confidence high. Claude desktop VM is local-only and the claude.ai VM is DOC-only; Cursor microVM is SRC strings only; Firecracker, warm pools and vsock are Verevon design |
| Appendix D | audit 2111-2161 | No F-t3-009 entry (no tier or effort change) |
| Dependents on -009 | audit 714 (-024), 786 (-030), 846 (-035), 942 (-043), 1300/1302/1304 (-084, -085, -087), 1631 (-070) | Cite F-t3-009 as a dependency |
| Sibling overlap | `01-verevon-space-audit.md` lines 676 (F-t1-021), 791 (F-t1-028) | Agent computer confinement, egress, persistent per-agent computer. Not swept here |

## 2. Citation verification (card F-t3-009)

Card id for every row: F-t3-009. The # column is the row index.

| # | Cited (card) | Claim (short) | W verdict | Live | Corrected path:lines |
|---|---|---|---|---|---|
| 1 | apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go:1-20 | Postgres lease store, bounded right | MATCH: package doc only; no state enum in this file | clean | same |
| 2 | lease.go:150-160 | Lease pinned to backend_id; SCRATCH start | MATCH: Create() 142; BackendID 156; State SCRATCH 159 | clean | same |
| 3 | apps/Model Plane/proto/model_plane/v1/sandboxes.proto:11-60 | SCRATCH/ACTIVE/SNAPSHOTTING/DESTROYED enum | DRIFT: RPC block is 12-90; enum is 97-103 (0 UNSPECIFIED, 1 SCRATCH, 2 ACTIVE, 3 SNAPSHOTTING, 4 DESTROYED) | clean | sandboxes.proto:97-103 |
| 4 | sandboxes.proto (no line) | Control-signed decision pins lease to one backend_id | DRIFT: capability_decision 122-125 (required only when space_id is set); backend_id 144-148 | clean | sandboxes.proto:122-125, 144-148 |
| 5 | rust/services/execution-core/src/sandbox.rs:270-360 | Measured profile; bwrap per call; process gate | MATCH: profile struct 279-294 (persistence "ephemeral" at 359); digest 306-311; process_host_enabled 327-332; capability_profile() 350-366 | clean | sandbox.rs:279-366 |
| 6 | execution-core/src/process_host.rs:1-25 | Opt-in background-process host | MATCH: module doc 1-21; env gate EXECUTION_CORE_PROCESS_HOST=enabled at line 18 | clean | process_host.rs:1-21 |
| 7 | execution-core/src/sandbox_lease.rs:1-50 (cited as "sandbox_lease.rs:1-50") | Space lease and hydrate; 1 h TTL | MATCH: module doc 1-29 (known-gap note 17-29); LEASE_TTL 50; ensure_sandbox_lease 106; ensure_hydrated_workspace 183 | clean | sandbox_lease.rs:1-50 |
| 8 | sandbox_lease.rs:431-510 | Space CAS workspace upload, then release | MATCH: release_sandbox_lease_if_any 436-488; upload_and_discard_workspace 495 | clean | sandbox_lease.rs:436-515 |
| 9 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/shared.rs:205-245 | Inline ExecuteStep path loses edits | DRIFT: shared.rs 205-246 holds is_space_scoped_turn (211-215) and sandbox_token (230-246), which returns None for non-Space turns (236-238). The loss mechanism is documented at sandbox_lease.rs:17-29 | clean | shared.rs:211-238; sandbox_lease.rs:17-29 |
| 10 | apps/Model Plane/docs/decisions/ledger.md:1358-1375 | SandboxBackend trait deferred 2026-08-22 | MATCH (exact). Second deferral note at 3577-3579 inside the S4.2 entry (heading 3540) | clean | ledger.md:1358-1375, 3577-3579 |
| 11 | SandboxCapabilityProfile (no line) | Reuse measured profile | MATCH: sandbox.rs:279-294 | clean | sandbox.rs:279-294 |
| 12 | workspace_cas.rs (short name) | Reuse CAS client | MATCH: CasClient 47; from_env 70 (env at 71-82: MINIO_ENDPOINT, MINIO_ACCESS_KEY, MINIO_SECRET_KEY, OBJECT_STORAGE_BUCKET, MINIO_REGION); put 140; get 172 | clean | execution-core/src/workspace_cas.rs |
| 13 | workspace_hydrate.rs (short name) | Reuse hydrate and diff | MATCH: hydrate 68; diff_and_upload 102. Header 1-14 says "not wired" and is STALE: called at sandbox_lease.rs:230 and :503 | clean | workspace_hydrate.rs:68, 102 |
| 14 | "lease sweeper" (no line) | Reuse lease sweeper | ABSENT: no sweep symbol in lease/ (0 matches). Expiry is lazy at lookup (lease.go:194-196). The only sweeper is the process staleness sweeper (internal/process/sweeper.go:24-27; cmd/main.go:144-153) | clean | lease.go:194-196; internal/process/sweeper.go:24-27 |
| 15 | "nats-provisioner contract-test approach" (no line) | Backend conformance pattern | MATCH for the pattern (contract tests TestRegistryRowsPointAtRealSource at manifest_contract_test.go:347 and TestEveryJetStreamPublishCallSiteIsRegistered at :418; bodies not read). No backend conformance suite exists | clean | apps/Model Plane/go/services/nats-provisioner/manifest_contract_test.go |
| 16 | "spawn/execute contract" and "a Rust trait with bubblewrap as implementation 1" (What to build) | Backend trait over spawn | ABSENT as trait. Trait grep finds only BrowserEventSink, CapabilityPolicy, RunOwnershipResolver, ManagedRunTokenProvider, SubagentDispatch. Spawn path is executor.rs:209, 250, 270; wrap at sandbox.rs:402-413; process_host.rs:428 | clean | executor.rs:209-270 |
| 17 | "remote-executor client" as implementation 2 (What to build) | Remote backend | ABSENT as client. MpSandboxPolicy::External (policy.rs:55-69) only forwards the network policy | clean | policy.rs:55-69 |
| 18 | "typed lifecycle, failure cause, heartbeat" (What to build) | Lease lifecycle fields | PARTIAL: 5-state enum only. No lease failure-cause or lease heartbeat. Heartbeat exists on process rows (sandboxes.proto:67-70, 397, 526-534); end_reason is free text (378, 499) | clean | sandboxes.proto |
| 19 | "cost-core metering" (card plane column); "VM-seconds metering" | Metering | ABSENT in cost-core code. Only docs/polar-inspired-usage.md:46 mentions sandbox (unrelated) | clean | none |
| 20 | "ZDR runs stay ephemeral" (How to build) | Ephemeral posture | MATCH: sandbox.rs:359. ZDR gate files are not in W | ?? (zdr_posture.rs, zdr_features.rs) | L-only: apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs (1259 lines) |

## 3. Facts the card does not state (verified)

1. **Thread scope is not wired.** The only caller passes `scope_id: run_id, scope_type: "agent"` (sandbox_lease.rs:143-144). `ensure_sandbox_lease(ctx, state, run_id, org_id, user_id)` (106-112) takes no thread id. sandbox_lease.rs has zero "thread" matches. A test comment at runtime_loop/mod.rs:1860 says the direct ExecuteStep RPC passes thread_id="". RunAgentRequest carries thread_id (execution.proto:81).
2. **sandbox-manager already accepts thread scope.** server.go:96 admits "thread" or "agent". The leases INSERT writes scope_id and scope_type (lease.go:163-166). The gap is the execution-core caller and the authority, not the RPC.
3. **Authority is Space-only today.** Non-Space leases need no decision: capability_decision is "required when space_id is set" (sandboxes.proto:118-125). The gateway mints the sandbox-manager bearer only for Space-scoped turns (shared.rs:211-238; streams.rs:112, 126). Control's sandbox decision is Space-only (space_capability_decision.go:14 action name; :54-59 intent fields; :131 issuer; :157 egress gate; :168 process entitlement). sandbox-manager's verifier (internal/authz/capability_verifier.go:30-122) has no thread field. The card's "own Control-signed decision" for thread workspaces would be a new rule.
4. **Control pattern available.** personal_thread_decision.go:94 defines PersonalThreadDecisionEvidence, with ValidateFor* methods (147-278, including ValidateForAgentAction at 278) and Issue* functions (364-462). Suitability: not determined (section 10).
5. **Entitlement defaults off.** Migration 027_space_sandbox_capability_effect_policy.up.sql adds space_effect_policies.sandbox_capability_entitled BOOLEAN NOT NULL DEFAULT FALSE, with the comment "A Space is not permitted to acquire a sandbox lease merely by existing."
6. **Release and acquire call sites.** Release: grpc.rs:1149 (cancel_run, W) and runtime_loop/agent.rs:984 (after finalize). Acquire: runtime_loop/mod.rs:604 and :612 (execute_step_inner, W) and :957 and :965 (execute_process_tool, process_start, W). The direct ExecuteStep path never reaches release (sandbox_lease.rs:17-29).
7. **Teardown order.** Release kills the lease's processes first (sandbox_lease.rs:455-457), then diffs and uploads the workspace (466-475), then releases the lease.
8. **Sandbox bearer path.** execution-core `authenticate_delegated_sandbox_manager` (auth.rs:504) verifies the bearer. model-gateway `VerifiedSandboxBearer` (model-gateway/src/auth.rs:227-237) is aud=sandbox-manager, accepted only for AcquireLease. sse.rs:8336-8342 forwards it as x-sandbox-authorization.
9. **Name collision.** workspace_promote.rs is the Data Plane v2 document promotion client (S3.3 step 5; header 1-6). It is not the CAS merge, which is sandbox-manager PromoteWorkspace (sandboxes.proto:35-44).
10. **ZDR inventory.** The live-only test zdr_refusal_inventory.rs (lines 15-25, ??) names workspace_promote.rs (reject_zdr_durable_mutation) and auth.rs (delegated sandbox bearer) as ZDR refusal sites. It has no sandbox_lease.rs or executor.rs site. A thread lease needs a named ZDR site, or the scan of all src files will flag bare refusals.
11. **No lease heartbeat or failure cause in schema.** Migration 0002 creates leases (line 11) and snapshots (line 27). Migration 0004 adds audience_revision_ceiling.
12. **No lifecycle SSE or route.** model-gateway sse.rs has 22 sandbox matches: bearer and header forwarding, plus one comment at line 1379. The only sandbox-adjacent route is http_routes.rs:349-350 (/v1/processes, /v1/processes/:id/output), which serves process rows.
13. **No sandbox action contract.** apps/Frontend Plane/verevonv3/src/shared/actions has zero sandbox hits in W.
14. **Name collisions to avoid.** src/shared/lib/sandbox-html.ts (iframe sandbox for previews). capability-core/internal/sandbox/sandbox.go (tool-sandbox contract shim: ResourceLimits 13, NetworkPolicy 21, FilesystemPolicy 27; no Go importer found by grep). home-updates.ts "workspace" feed enum (live M, unrelated).
15. **Stale docs.** capability-ownership-matrix.md:66 says real OS isolation is "GENUINE GAP - nothing exists", but sandbox.rs ships the bwrap argv builder. Row 126 says "No sandbox-policy type exists", while row 138 says MpSandboxPolicy exists (policy.rs:55).

## 4. Affected-file map

Columns: path | role | approx lines (W) | live | existing tests (count, names where cheap).

### 4a. Rust execution-core (apps/Model Plane/rust/services/execution-core/src/)

| Path | Role | Lines (W) | Live | Tests |
|---|---|---|---|---|
| sandbox.rs | bwrap argv; SandboxedCommand; measured profile; process gate | 65, 91, 121-132, 250-256, 279-366, 402-413 | clean | 15 |
| executor.rs | spawn/execute: execute_sandboxed, _in_dir, spawn_capture | 116, 149-156, 209-270 | clean | 12 |
| policy.rs | MpSandboxPolicy incl. External | 55-69 | clean | not counted |
| sandbox_lease.rs | Space lease, hydrate, release; LEASE_TTL 50; scope "agent" 143-144 | 17-50, 58, 106, 183, 287, 436-515 | clean | 9 (e.g. a_second_call_..._reuses_the_cached_lease_without_a_client; service_token_rejects_wrong_audience) |
| sandbox_manager_client.rs | gRPC client; scope validation; URL env | 147-148, 183-188 | clean | 5 |
| process_host.rs | OS side of background processes; wrap at 428 | 1-21, 428 | clean | 9 |
| workspace_cas.rs | CAS client (MinIO/S3) | 47, 70-82, 140, 172 | clean | 6 |
| workspace_hydrate.rs | hydrate, diff_and_upload | 30, 40, 68, 102 | clean | 6 |
| state.rs | SandboxLease (69), HydratedWorkspace (95), take_sandbox_lease 234, take_hydrated_workspace 257; RunStatus enum 16 | 16, 69, 95, 234, 257 | clean | 14 |
| capability_client.rs | Control client; decision path; env | 19, 22, 111, 116, 212 | clean | 3 |
| http_health.rs | backend_id resolution; profile in health | 51-52, 143-144 | clean | 4 |
| code_interpreter.rs | per-call ephemeral workspace; execute_sandboxed_in_dir call | 583 | clean | 20 |
| process_tools.rs | process_* tool family | n/a | clean | 7 |
| runtime_loop/mod.rs | execute_step 278; execute_step_inner 455 (leases 604, 612); execute_process_tool 921 (leases 957, 965) | as listed | M (+18/-0); live 467, 617, 625, 935, 971, 979 | 22 |
| runtime_loop/agent.rs | RunAgent finalize, then release at 984 | 984 | M (+200/-7; 2 sandbox lines in test helpers); live 988 | 119 |
| grpc.rs | execute_step 828; cancel_run 1134 (release 1149); run_agent 1203; token and CAS init 1370-1380 | as listed | M (+76/-22); live 846, 1156, 1171, 1225, 1397 | 15 |
| auth.rs | delegated bearer; authenticate_delegated_sandbox_manager 504 | 504 | M (+608/-23; 8 sandbox lines) | in-file |
| workspace_promote.rs | Data Plane v2 document promotion (not the CAS merge) | 1-6 | M (+4/-3) | in-file |
| agent_route.rs | run route validation | n/a | M (+253/-32; 0 sandbox lines) | in-file |

### 4b. Rust model-gateway (apps/Model Plane/rust/services/model-gateway/src/)

| Path | Role | Lines (W) | Live | Tests |
|---|---|---|---|---|
| sse.rs | sandbox bearer threading; x-sandbox-authorization forward | 845, 875, 8336-8342, 9186-9246 | M (+814/-69; 0 sandbox lines) | 11833-11855 (sandbox-header tests) |
| auth.rs | VerifiedSandboxBearer; audience mapping | 227-237, 940 | M (+389/-26; 2 sandbox lines) | in-file |
| http_routes.rs | /v1/processes routes | 349-350 | M (+190/-47; 0 sandbox lines) | in-file |
| zdr_precheck.rs | ZDR precheck (relation to zdr_posture.rs not verified) | n/a | M (+58/-226) | in-file |
| zdr_posture.rs | ZDR posture (cited by F-t3-011 card) | n/a | ?? (1259 lines; 0 sandbox lines) | none in W |
| zdr_features.rs | ZDR features | n/a | ?? (365 lines) | none in W |

### 4c. Go sandbox-manager (apps/Model Plane/go/services/sandbox-manager/)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| cmd/main.go | env wiring: SANDBOX_MANAGER_BACKEND_ID 60; ALLOW_EPHEMERAL_DEVELOPMENT 42, 63; DATABASE_URL 43; process sweeper 131-153 | as listed | clean | main_test.go 1 |
| internal/lease/lease.go | Store: Create 142; lookup 181-201 (expiry 194; backend check 197); GetScoped 207; Activate 217; BeginSnapshot 242; EndSnapshot 268; ReleaseScoped 302; GetAny 324 | as listed | clean | lease_test.go 21; lease_integration_test.go 4 (tag integration) |
| internal/authz/capability_verifier.go | Space decision verifier; claims 86; expected digest 250 | as listed | clean | capability_verifier_test.go 7 |
| internal/authz/authz.go; space_read_verifier.go | method switch; Space read verifier | n/a | clean | authz_test.go 4; space_read_verifier_test.go 8 |
| internal/server/server.go | RPC handlers; scope_type check | 96 | clean | server_test.go 32; errors_test.go 2 |
| internal/server/process.go; memory_store.go; types.go; store.go | process RPCs; in-memory store | n/a | clean | process_test.go 23 |
| internal/process/store.go; sweeper.go | registry rows; SweepStale 916; sweeper env 108-121; interface 24-27 | as listed | clean | store_test.go 25; store_integration_test.go 19 (tag); sweeper_test.go 5 |
| internal/workspace/store.go | workspace_files store (migration 0001) | n/a | clean | store_test.go 10; store_integration_test.go 6 (tag) |
| internal/snapshot/snapshot.go; exclude.go | snapshot path | n/a | clean | snapshot_test.go 5; snapshot_integration_test.go 2 (tag); exclude_test.go 2 |
| internal/redact/redact.go | command and output redaction | n/a | clean | redact_test.go 12 |
| migrations/0001-0004 | tables (section 5) | n/a | clean | n/a |

Sandbox-manager total: 188 `func Test` across 18 files.

### 4d. Control Plane (apps/Control Plane/user-core/)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| internal/spaces/space_capability_decision.go | sandbox capability decision, Space-only | 14, 39-41, 54-59, 64, 89, 131, 157, 168 | clean | space_capability_decision_test.go |
| internal/spaces/personal_thread_decision.go | personal-thread evidence and Issue* pattern | 94, 147-278, 364-462 | clean | personal_thread_decision_test.go |
| internal/http/server.go; spaces.go; service_auth.go | route 371; reauthorizer 135-141; service-auth path 120 | as listed | clean | service_auth_test.go 160 |
| migrations/027_space_sandbox_capability_effect_policy.up.sql | entitlement column default FALSE | 1-6 | clean | n/a |

### 4e. Frontend Plane gateway and UI (apps/Frontend Plane/verevonv3/)

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| apps/gateway/src/domains/chat/shared.rs | Space-only sandbox token mint | 205-246 (211-215; 230-246) | clean | not counted |
| apps/gateway/src/domains/chat/streams.rs | passes sandbox_token to outbound call | 112, 126 | clean | not counted |
| apps/gateway/src/domains/chat/json_handlers.rs | sandbox mention (same path, not read in detail) | n/a | clean | n/a |
| apps/gateway/src/domains/spaces.rs | Space decisions; sandbox mention (not read) | n/a | clean | n/a |
| src/shared/api/chat-client.ts | SSE client; no sandbox event | n/a | M (+53/-0) | chat-client.test.ts (M) |
| src/shared/actions/ | action contracts; no sandbox action | n/a | clean (home-updates.ts M; chat-work-period.ts ??) | n/a |
| src/features/chat/components/ChatArtifactPanel.tsx | artifact preview; no computer panel | n/a | clean | ChatArtifactPanel.test.tsx |
| docs/S3_2_SANDBOX_LEASE_CLOSEOUT_DESIGN_2026-09-10.md; docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md; docs/S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md | design docs for lease, workspace, process registry | n/a | clean | n/a |

### 4f. Model Plane docs, Go services, proto, scripts, deploy

| Path | Role | Lines | Live | Tests |
|---|---|---|---|---|
| apps/Model Plane/docs/decisions/ledger.md | ADR ledger, newest first | 1358-1375; 3540-3579 | clean | n/a |
| apps/Model Plane/docs/capability-ownership-matrix.md | one-owner rules | 3, 7, 22, 65-68, 126, 138, 158 | clean | n/a |
| apps/Model Plane/docs/sse-event-taxonomy.md | SSE taxonomy (CI-gated) | n/a | M (+4/-0) | CI gate |
| apps/Model Plane/docs/external-ideas-harvest.md; docs/gap-model.md | planned backend-trait shape (hermes, MIT) | harvest:84; gap:420 | clean | n/a |
| apps/Model Plane/go/services/capability-core/internal/sandbox/sandbox.go | tool-sandbox contract shim | 13, 21, 27 | clean | sandbox_test.go |
| apps/Model Plane/go/services/cost-core | no sandbox or VM metering | n/a | clean | n/a |
| apps/Model Plane/go/services/nats-provisioner/manifest_contract_test.go | contract-test pattern | 347, 418 | clean | in-file |
| apps/Model Plane/proto/model_plane/v1/sandboxes.proto; execution.proto; ids.proto; runs.proto | contracts (section 5) | as listed | clean | n/a |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | session contract | n/a | M (+42/-0; 0 sandbox lines) | n/a |
| apps/Model Plane/scripts/verify-sandbox-isolation.sh | live bwrap isolation check | n/a | clean | n/a |
| apps/Model Plane/deploy/docker-compose.yml | wiring: EXECUTION_CORE_PROCESS_HOST 1184; SANDBOX_MANAGER_ADDR 486; sandbox-manager 1575-1601 | as listed | clean | n/a |

## 5. Related contracts

- **Proto** (`apps/Model Plane/proto/model_plane/v1/sandboxes.proto`): service SandboxManager 12-91 with 14 RPCs (AcquireLease 14, ReleaseLease 17, SnapshotSandbox 20, ActivateLease 26, GetWorkspaceManifest 33, PromoteWorkspace 44, process RPCs 61-87, Health 90). Messages: AcquireLeaseRequest 105, AcquireLeaseResponse 134, ReleaseLeaseRequest 166, SnapshotRequest 179, ActivateLeaseRequest 204, GetWorkspaceManifestRequest 217, WorkspaceChangedFile 239, PromoteWorkspaceRequest 257, Process 345, RegisterProcessRequest 450, ReadProcessOutputRequest 613. Enums: SandboxLifecycleState 97-103, ProcessState 288.
- **Proto, others:** execution.proto RunAgentRequest 76 (thread_id 81; space_id 168), RunAgentResponse 212, ExecuteStepRequest 268, ExecuteStepResponse 321. ids.proto SandboxLeaseId 48-52. runs.proto:19 mirrors MpSandboxPolicy naming. Buf baseline: `apps/Model Plane/proto/baseline.binpb`.
- **Migrations, latest by name:**
  - sandbox-manager (8 files): latest `0004_audience_revision_ceiling.up.sql` (has .down). Tables: workspace_files (0001:9), leases (0002:11), snapshots (0002:27), sandbox_processes (0003:33), sandbox_process_output (0003:115).
  - Control user-core (45 files): latest `029_mail_task_addresses.up.sql`. Sandbox-related: `027_space_sandbox_capability_effect_policy.up.sql`, `028_space_process_registry_effect_policy.up.sql`.
  - session-core (43 files): latest `0043_audit_outbox_requeue.sql` (no .up suffix).
- **Config and env:**
  - execution-core: EXECUTION_CORE_PROCESS_HOST (sandbox.rs:330; default off; compose 1184); EXECUTION_CORE_BACKEND_ID and HOSTNAME (http_health.rs:52); SANDBOX_MANAGER_URL or SANDBOX_MANAGER_ADDR (sandbox_manager_client.rs:147-148); EXECUTION_CORE_CONTROL_SANDBOX_CAPABILITY_TOKEN (capability_client.rs:111, 212); EXECUTION_CORE_ALLOW_INSECURE_SANDBOX_CAPABILITY_LOOPBACK (capability_client.rs:116); EXECUTION_CORE_SERVICE_ID and EXECUTION_CORE_SERVICE_API_KEY (sandbox_lease.rs:329, 334); MINIO_* and OBJECT_STORAGE_BUCKET (workspace_cas.rs:71-82).
  - sandbox-manager: SANDBOX_MANAGER_BACKEND_ID (cmd/main.go:60; compose 1601); SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT (main.go:42, 63); DATABASE_URL (main.go:43); SANDBOX_MANAGER_AUTH_AUDIENCE (compose 1587); PROCESS_SWEEPER_ENABLED, PROCESS_SWEEP_INTERVAL_SECS, PROCESS_HEARTBEAT_STALE_SECS (internal/process/sweeper.go:108-121).
- **SSE:** no sandbox lifecycle event exists. Taxonomy doc `apps/Model Plane/docs/sse-event-taxonomy.md` (live M) is generated by `apps/Model Plane/scripts/gen-sse-event-taxonomy.py`. CI gate: `.github/workflows/sse-taxonomy.yml:44-45`. Source: `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts` (live M).
- **Routes:** model-gateway `/v1/processes` (http_routes.rs:349-350). Control internal `POST /api/v1/internal/spaces/sandbox-capability-decision` (user-core/internal/http/server.go:371; service_auth.go:120), called by execution-core capability_client.rs:22. No lease, workspace or lifecycle route exists.
- **Frontend action contracts:** none for the sandbox (apps/Frontend Plane/verevonv3/src/shared/actions).
- **ZDR contract goldens:** `.github/workflows/contract-goldens.yml:66, 71, 74` (zdr-contract-sync scripts).

## 6. Test and build commands

- **Rust** (cwd `apps/Model Plane/rust`): `cargo test -q -p execution-core --all-targets` and `cargo test -q -p model-gateway --all-targets` (apps/Model Plane/CLAUDE.md:30; crate names execution-core/Cargo.toml:2, model-gateway/Cargo.toml:2). Full: `cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:119`, working-directory at :57). Workspace members: rust/Cargo.toml:14, 17. Auto-memory (model-plane-build-and-deploy) says two cargo targets fail on Windows and to use `--lib`; not re-verified here.
- **Linux-only bwrap:** sandbox.rs is Linux-gated (apps/Model Plane/CLAUDE.md:19). Live isolation proof: `apps/Model Plane/scripts/verify-sandbox-isolation.sh` in a privileged Linux container (capability-ownership-matrix.md:158).
- **Go sandbox-manager** (cwd `apps/Model Plane/go/services/sandbox-manager`; module `github.com/triodelab/model-plane/services/sandbox-manager`, go.mod:1, go 1.25.1 at :3): `go vet ./...` and `go test ./...` (apps/Model Plane/CLAUDE.md:31). Integration tests need `-tags integration` (build tag present in lease_integration_test.go, process/store_integration_test.go, workspace/store_integration_test.go, snapshot_integration_test.go) and a database (not read). -race needs cgo, unavailable per auto-memory.
- **Go capability-core** (apps/Model Plane/go/services/capability-core): `go test ./internal/sandbox/...` if the contract shim changes (go.mod not read).
- **Proto** (repo root, per CI): `buf lint apps/Model Plane/proto` and `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (`.github/workflows/buf.yml:21, 23`). `buf generate` for regen (apps/Model Plane/CLAUDE.md:42). buf lint has pre-existing debt (same line).
- **Control user-core** (apps/Control Plane/user-core): `make test-coverage` (root CLAUDE.md:54). go.mod and the internal/spaces test target not read.
- **Gateway BFF** (apps/Frontend Plane/verevonv3): `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (CLAUDE.md:65; crate verevon-gateway-rs, apps/gateway/Cargo.toml:2). Frontend: `pnpm lint` (CLAUDE.md:61), `pnpm typecheck` (:62), `pnpm test` (:63). Auto-memory verevonv3-worktree-verification records pre-existing typecheck, lint and vitest failures.
- **SSE taxonomy:** `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (`.github/workflows/sse-taxonomy.yml:45`).
- **ZDR contract:** `bash scripts/tests/zdr-contract-sync.test.sh`, `bash scripts/tests/zdr-contract-sync.test.sh --self-test`, `node --test scripts/tests/zdr-contract-schema.test.mjs` (`.github/workflows/contract-goldens.yml:66, 71, 74`).
- **Stack:** `./scripts/compose.sh up -d` in apps/Model Plane (apps/Model Plane/CLAUDE.md:38); compose file `apps/Model Plane/deploy/docker-compose.yml`.

## 7. Owners

| Area | Owner | Rule (cite) |
|---|---|---|
| Lease registry, TTL, backend pin, snapshots | sandbox-manager (Go) | capability-ownership-matrix.md:65; master-ownership-matrix.md:45 (registry is Go); apps/Model Plane/CLAUDE.md:22 |
| bwrap spawn, background OS processes | execution-core (Rust) | capability-ownership-matrix.md:68; apps/Model Plane/CLAUDE.md:22 (Rust on hot path) |
| Process rows | sandbox-manager | capability-ownership-matrix.md:67 |
| Space and thread authority (signed decisions) | Control Plane (user-core) | root CLAUDE.md:23 (Control owns entitlements, quotas); sandboxes.proto:122-125 |
| Sandbox entitlement | Control Plane | migration 027; root CLAUDE.md:23 |
| Deployed-agent loop vs plain-chat loop | execution-core execute_step_inner vs model-gateway dispatch_tool; keep separate | apps/Model Plane/CLAUDE.md:24 |
| Backend seam | Model Plane; ADR in ledger first | apps/Model Plane/CLAUDE.md:25, 27; ledger.md:1358-1375 |
| Document promotion of workspace paths | Data Plane v2 | workspace_promote.rs:1-6; root CLAUDE.md Architecture Rules (Data Plane owns documents) |
| Chat turn token minting; BFF normalization | Frontend Plane gateway | root CLAUDE.md:29 |
| Browser or desktop inside a sandbox | Quarry-v2 executes; Model proposes | root CLAUDE.md:34 |
| VM-second metering | Model Plane cost (cost-core) | root CLAUDE.md:27; no metering in cost-core code |
| Tool-sandbox contract shim | capability-core (candidate duplicate owner) | capability-ownership-matrix.md:3 (one canonical owner); :22 (sandbox/ under capability-core) |
| ZDR propagation | every content-persisting boundary | root CLAUDE.md:35 |
| No cross-plane database access | all | root CLAUDE.md:32 |
| Model Plane v1 only | apps/Model Plane v2 deprecated | capability-ownership-matrix.md:7 |

## 8. Uncommitted-only (live tree C:/dev/CoresSystem)

No WS22 file exists only in L among the card-cited paths. The rows below are relevant live changes. Numstat uses `--ignore-cr-at-eol`. Sandbox-keyword count is the number of added lines matching sandbox, lease, workspace, process_host, bwrap or sandbox_manager.

| Path (L) | Status | Size | What changed |
|---|---|---|---|
| apps/Model Plane/rust/services/execution-core/src/grpc.rs | M | +76/-22 | 0 sandbox lines. Waived-subscription admission tests and a route_caller helper. Live line shifts listed in 4a |
| execution-core/src/runtime_loop/mod.rs | M | +18/-0 | 0 sandbox lines. Live lines shifted (+12 to +14) as listed in 4a |
| execution-core/src/runtime_loop/agent.rs | M | +200/-7 | 2 sandbox lines: test helpers test_sandbox_manager_client and test_sandbox_tokens. Waived-run and delegated-child lineage tests |
| execution-core/src/auth.rs | M | +608/-23 | 8 sandbox lines: delegated sandbox-manager identity tests and "does not match caller" message. Touches the sandbox bearer path |
| execution-core/src/workspace_promote.rs | M | +4/-3 | Small edit to the Data Plane document promotion client; 0 sandbox lines |
| execution-core/src/agent_route.rs; knowledge_tools.rs; runtime_loop/session_core_rules_double.rs | M | +253/-32; +125/-6; +536/-1 | 0 sandbox lines |
| model-gateway/src/sse.rs; sse_events.rs; tool_loop.rs; http_routes.rs | M | +814/-69; +202/-1; +370/-55; +190/-47 | 0 sandbox lines each |
| model-gateway/src/auth.rs | M | +389/-26 | 2 sandbox lines: VerifiedSandboxBearer return type and sandbox-manager header mapping |
| model-gateway/src/zdr_precheck.rs | M | +58/-226 | 0 sandbox lines; large deletion, relation to zdr_posture.rs not verified |
| model-gateway (other 18 M files in src) | M | various | 0 sandbox-keyword lines each |
| proto/model_plane/v1/sessions.proto | M | +42/-0 | 0 sandbox lines |
| apps/Model Plane/docs/sse-event-taxonomy.md | M | +4/-0 | SSE taxonomy (CI-gated) |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | M | +53/-0 | 0 sandbox lines |
| apps/Frontend Plane/verevonv3/src/shared/actions/home-updates.ts | M | +1/-1 | "workspace" feed enum. Unrelated to the sandbox |
| .github/workflows/model-plane-rust-ci.yml | M | +50/-0 | Adds "Session Core retention receipt (Postgres)" job (cargo test -p session-core zdr_receipt, ignored tests). No sandbox job |
| apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs | ?? | 90 lines | ZDR NAMED_SITES (lines 15-25) include workspace_promote.rs and auth.rs delegated sandbox bearer |
| apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs | ?? | 1259 lines | ZDR posture; cited by F-t3-011; 0 sandbox lines |
| apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs | ?? | 365 lines | ZDR features; 0 sandbox lines |
| apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs | ?? | not measured | ZDR tests |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.zdr-refusal.test.ts | ?? | not measured | ZDR refusal client test |
| 5 D entries (verevon-web product-demo files) | D | n/a | Unrelated |

Clean in L (not in the status list): sandbox-manager (all), sandboxes.proto, execution.proto, ids.proto, runs.proto, sandbox.rs, sandbox_lease.rs, process_host.rs, executor.rs, workspace_cas.rs, workspace_hydrate.rs, state.rs, capability_client.rs, sandbox_manager_client.rs, http_health.rs, code_interpreter.rs, process_tools.rs, policy.rs, ledger.md, capability-ownership-matrix.md, master-ownership-matrix.md, Control user-core spaces/ and migrations/, gateway domains/chat/shared.rs and streams.rs, both root and Model Plane CLAUDE.md, deploy/docker-compose.yml, .github/workflows/buf.yml, sse-taxonomy.yml and contract-goldens.yml.

## 9. Overlaps and dependents

- **Sibling F-t1-021 and F-t1-028** (`01-verevon-space-audit.md` lines 676, 791; table 97; summary 71, 111). Both touch per-agent confinement, egress and persistence. The sandbox lease is per run with a one-hour TTL (sandbox_lease.rs:45-50), which F-t3-085 also cites (audit line 1302). Any persistent per-agent computer needs one lease model; decide once.
- **Dependents** (audit): F-t3-024 (705-716), F-t3-030 (786), F-t3-035 (837-848), F-t3-043 (933-944), F-t3-070 (table 1631), F-t3-084, F-t3-085, F-t3-087 (1300-1304). 9.4 says the computer track is strictly ordered (line 1699).
- **Upstream:** F-t3-002 (durable agent loop, card line 482); ADR sandbox backend; VM metering (audit 1559).
- **Feeds:** F-t3-024 (card line 482).

## 10. NOT DETERMINED

1. Whether sandbox-manager authz admits a non-Space thread lease without a Control decision. server.go:96 accepts "thread"; internal/authz/authz.go was not read in full.
2. Whether live (M) changes in grpc.rs, runtime_loop/mod.rs, runtime_loop/agent.rs or auth.rs alter lease behaviour. Only numstat and keyword counts were taken; hunks were not read in full.
3. Whether every RunAgent turn carries a stable thread_id into execution-core. execution.proto:81 is the only thread_id seen in RunAgentRequest. Direct ExecuteStep is thread-less (test comment, runtime_loop/mod.rs:1860).
4. Suitability of personal_thread_decision.go as the template for a thread-sandbox decision (pattern exists; design not read).
5. Whether capability-core/internal/sandbox is meant to own the tool-sandbox contract. No Go importer found; no ruling found in capability-ownership-matrix.md.
6. microVM provider, guest-image owner, region, cost model, VM-second metering and snapshot retention rules. Not in code; open per audit 9.3 item 1 and 10.2 item 8.
7. Whether zdr_posture.rs (live-only, not read) gates any sandbox path. Zero sandbox keyword matches in the file.
8. Behaviour of bwrap-gated tests on Windows (is_supported() is false there). Not run.
9. Whether CI runs sandbox-manager `go test ./...` and which integration tag/DB job covers it. A grep for go test/vet/build in model-plane-rust-ci.yml, buf.yml, contract-goldens.yml and sse-taxonomy.yml found none. session-core-service.yml, eval-lab.yml and quarry-v2-ci.yml were not checked.
10. S3.3 design doc (ACL and workspace-scope rules) and S4.2 design doc claims were not read beyond the citations.
11. The sibling F-t1-021 and F-t1-028 card citations were not swept.
12. Card claim that "cost-core metering" is planned: no plan text found in cost-core docs beyond the unrelated polar-inspired doc.
