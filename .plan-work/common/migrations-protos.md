# Migration / proto / contract ledger (collision map for parallel workstreams)

**Legend.** `W` = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). `L` = live tree `C:/dev/CoresSystem` (same HEAD + 324 entries in `.plan-work/live-tree-status.txt`). Each `file:line` cite is relative to the base named in its row or section (section 1: the service directory; section 2: `apps/Model Plane/proto/model_plane/v1/`; section 3: `apps/Model Plane/proto/` unless noted; section 4: the mp-contracts directory). Full repo-relative paths are given wherever the base is not obvious. `:n` = line in W. `L:n` = line in L only where it differs. `status n` = line n of `live-tree-status.txt`.

**Checks run (all read-only).** Glob/Grep/Read; `git --no-optional-locks` status, diff, ls-files, log, check-ignore; `PYTHONUTF8=1 python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` in W and L (check mode only, no writes); sqlx-core 0.8.6 source from the local cargo registry. Nothing was built, migrated, or run against a database.

---

## 0. Headlines

- **Live-only contract changes (not in HEAD):** session-core migrations `0044`, `0045` plus a `revert/` folder; `SessionRetentionService` in `sessions.proto`; ChatEvent variants `ZdrError` and `RetrievalNotice`; mp-contracts modules `retention` and `zdr_refusal`. See section 6.
- **HEAD fails the SSE taxonomy freshness check.** `--check` exits 1 in W: the committed doc lacks rows `plan_proposed` and `source_scope_applied`. L passes (`sse taxonomy up to date (23 events)`, exit 0).
- **auth-core bootstrap file is missing.** `init_better_auth.sql` is referenced by `apps/Control Plane/auth-core/docker-entrypoint.sh:181` but is absent in W and L. The loop skips missing files silently (`:184`).
- **Duplicate numeric migration prefixes:** capability-core `0013` (x2); user-core `008` (x2) and `021` (x2).
- **buf CI paths are unquoted** (`.github/workflows/buf.yml:21,23`). The path `apps/Model Plane/proto` contains spaces, so the shell is likely to split it. Not executed.

---

## 1. SQL migrations

### 1a. Owners, naming, latest 5 (W), runner, live delta

| Service (dir) | Naming / count (W) | Latest 5 (W) | Applied by (file:line) | Live (L) delta |
|---|---|---|---|---|
| **session-core** (Model Plane, Rust) `apps/Model Plane/rust/services/session-core/migrations/` | `NNNN_snake.sql`, simple (no up/down). 0001–0043, no gaps, prefixes unique. | 0039_memory_user_control.sql, 0040_direct_run_completion_key.sql, 0041_mail_task_thread_origin.sql, 0042_thread_persona_toggle.sql, 0043_audit_outbox_requeue.sql | Compile-time embed `sqlx::migrate!("./migrations")` at `apps/Model Plane/rust/services/session-core/src/store.rs:34`, inside `run_migrations` (`store.rs:33-37`). Startup call `apps/Model Plane/rust/services/session-core/src/main.rs:69`. sqlx 0.8.6 (`apps/Model Plane/rust/Cargo.lock:4116-4117`). Workspace dep `apps/Model Plane/rust/Cargo.toml:76`. | +`0044_run_retention_receipt.sql`, +`0045_zdr_waived_runs_org_index.sql` (untracked, status 320-321). +`migrations/revert/` (untracked, status 322) containing `0044_run_retention_receipt.down.sql` and `0045_zdr_waived_runs_org_index.down.sql`. |
| **capability-core** (Model Plane, Go) `apps/Model Plane/go/services/capability-core/migrations/` | `NNNN_snake.up.sql` + `.down.sql`. 0001–0015. **0013 twice:** `0013_privacy_tier_columns`, `0013_subagent_result_read_capability`. | 0012_run_watch_subscriptions, 0013_privacy_tier_columns, 0013_subagent_result_read_capability, 0014_memory_tool_capabilities, 0015_background_process_capability (all `.up.sql`) | `docker-entrypoint.sh:4-7` **requires** DATABASE_URL (exit 1). `:9-15` psql loop over `/app/migrations/*.up.sql`, `ON_ERROR_STOP`, **no ledger**, so re-run on every boot. `Dockerfile:48` COPY. | none |
| **cost-core** `apps/Model Plane/go/services/cost-core/migrations/` | `NNNN_snake.up.sql` + `.down.sql`. 0001–0010, no gaps. | 0006_provider_attribution, 0007_azure_actual_costs, 0008_erasure_replay_guard, 0009_usage_reporting_periods, 0010_billing_usage_outbox (`.up.sql`) | `docker-entrypoint.sh:8-10` psql loop, `ON_ERROR_STOP`, no ledger. Skip when DATABASE_URL unset (`:17`). Re-run every boot: comment at `internal/postgres/store_integration_test.go:311`. `Dockerfile:41` COPY. | none |
| **sandbox-manager** `apps/Model Plane/go/services/sandbox-manager/migrations/` | `.up/.down`. 0001–0004. | all four | `docker-entrypoint.sh:15-17` psql loop, no ledger. Skip message `:24`. `Dockerfile:40`. **Not** in release snapshot (section 7). | none |
| **orchestrator-core** `apps/Model Plane/go/services/orchestrator-core/migrations/` | `.up/.down`. 0001–0002. | both | `docker-entrypoint.sh:9-11` psql loop, no ledger. Skip message `:18`. `Dockerfile:50`. | none |
| letta-bridge (not requested; in release snapshot) `apps/Model Plane/go/services/letta-bridge/migrations/` | `.up/.down`. 0001–0002. | both | `docker-entrypoint.sh:10-12` psql loop, no ledger. `Dockerfile:40`. | none |
| **org-core** (Control) `apps/Control Plane/org-core/migrations/` | `NNN_snake.up.sql` + `.down.sql`. 001–020. | 016_auth_deletion_revision, 017_gdpr_audit_outbox, 018_org_deletion_ledger, 019_interactive_retention_outbox, 020_interactive_retention_outbox_org_id_check (`.up.sql`) | Go runner `internal/database/migrate.go:26-111` `RunMigrations`. Applies `*.up.sql` only (`:47`), sorted (`:51`). Ledger `schema_migrations` (`:27-35`). One tx per file (`:20-21`, `applyOne` `:66`). Startup per `scripts/shared-entrypoint.sh:41-42`. `Dockerfile:62` COPY. | none |
| **user-core** (Control) `apps/Control Plane/user-core/migrations/` | `NNN_snake.up.sql` + `.down.sql`. 001–029. **008 twice** (`008_avatar_text`, `008_performance_indexes`). **021 twice** (`021_space_recipient_audiences`, `021_space_schedule_fire_effect_policy`). 017–029 are up-only. Unsuffixed `001_init_text_ids.sql` is never applied. | 025_owner_effect_reservations, 026_space_thread_read_effect_policy, 027_space_sandbox_capability_effect_policy, 028_space_process_registry_effect_policy, 029_mail_task_addresses (`.up.sql`) | Go `internal/database/database.go:74-156` `RunMigrations`. Ledger `schema_migrations(version VARCHAR(255))` (`:77-81`). Bootstrap marks `001_init` applied when `users` exists (`:86-104`). Applies `*.up.sql` (`:114`), sorted (`:118`). Version = name minus `.up.sql` (`:121`). Legacy `Makefile:89-96` psql path, `001_init` only. `Dockerfile:100` COPY. | none |
| **auth-core** (Control, Node) `apps/Control Plane/auth-core/migrations/` | `NNN_snake.sql` (no up/down). 014–028, plus unnumbered `gdpr_hard_delete.sql` (16 tracked files via `git ls-files`). **`init_better_auth.sql` referenced but absent.** | 024_revisioned_cross_plane_projection, 025_gdpr_outbox_publish_fence, 026_invitation_removal_preflight, 027_membership_mutation_intents, 028_org_group_grants | `docker-entrypoint.sh:135-173` `apply_migration`: sha256 per file. Ledger `public.auth_schema_migrations(version,checksum)` (`:147-151`). Checksum mismatch aborts (`:154-158`). Advisory lock (`:146`). Loop `:178-187`: `init_better_auth.sql`, `gdpr_hard_delete.sql`, `[0-9][0-9][0-9]_*.sql`. Missing file skipped (`:184`). `Dockerfile:59`, `Dockerfile.prod:44` COPY. | none |
| **audit-core** (Control, Go) schema `internal/store/schema.sql` (= `001_init`); `internal/store/migrations/002..006` | `NNN_snake.sql`, no up/down. **Explicit `//go:embed` list**, not a folder scan. | 002_jetstream_inbox, 003_logical_control_audit_idempotency, 004_logical_usage_idempotency, 005_usage_source_identity, 006_v2_producer_authority | `internal/store/migrate.go:12-28` embeds each file by name. Slice `:36-43`. Ledger `schema_migrations` (`:55-62`). Per-file tx (`:73-98`). Startup `cmd/server/main.go:53-55` (fatal on failure). No Dockerfile COPY of migrations. Top-level `apps/Control Plane/audit-core/migrations/001_init.sql` is **unreferenced**. | none |
| **notification-core** (Application, Go) `apps/Application Plane/notification-core/migrations/` | `NNN_snake.up.sql` + `.down.sql`. 001–014. | 010_delivery_attempts, 011_delivery_callback_replays, 012_feed_projection_attempts, 013_register_run_watch_notifications, 014_register_mail_task_drafted (`.up.sql`) | `internal/database/migrate.go:68` `RunMigrations` to `:76` `runMigrations`. `.up.sql` filter `:129-137`. Ledger table `notification_core_schema_migrations` (`:17-19`). Per-file tx `:103-122`. Startup `cmd/server/main.go:43-44`. `Dockerfile:53` COPY. | none |
| **Convex** (Application) `apps/Application Plane/convex-core/convex/schema.ts` | **No migration files.** Code-first: `export default defineSchema({` at `:11`. 38 `defineTable(` lines (first `organizations` at `:13`). | n/a (edit schema.ts) | Convex dev/deploy loop in the `convex-gateway` container. Mounts `./convex` (`apps/Application Plane/convex-core/docker-compose.yml:146-154`). Comments say `npx convex dev` and a forced `convex deploy` at start (`:168-169`). | none |

### 1b. Other migration dirs (not requested; runners not inspected, for name-collision awareness)

- `apps/Control Plane/session-core/migrations/`: 001_init (up/down), 003_add_org_to_sessions.up, 004_add_orchestration_tables (up/down), 005_drop_agent_run_scaffold (up/down). **Same service name as Model Plane session-core.**
- `apps/Control Plane/billing-core/migrations/`: 0001–0008 (up/down mixed).
- `apps/Application Plane/conversation-core/conversation-core-go/internal/database/migrations/`: 001–031.
- `apps/Application Plane/insight-core/internal/database/migrations/`: 001–003.
- `apps/Application Plane/leads-core/internal/database/migrations/`: 001–003.
- `apps/Application Plane/social-core/internal/database/migrations/`: 001–002.
- Data Plane v2, Quarry v2 and Frontend migrations: out of scope.

### 1c. Next free numbers

| Surface | Next free (HEAD) | Next free (live) | Note |
|---|---|---|---|
| session-core | 0044 | **0046** | Live holds untracked 0044 and 0045. Another branch taking 0044 collides. |
| capability-core | 0016 | 0016 | 0013 already duplicated |
| cost-core | 0011 | 0011 | |
| sandbox-manager | 0005 | 0005 | |
| orchestrator-core | 0003 | 0003 | |
| org-core | 021 | 021 | |
| user-core | 030 | 030 | 008 and 021 already duplicated |
| auth-core | 029 | 029 | Bootstrap file gap (section 7) |
| audit-core | 007 | 007 | Must also be added to the `migrate.go` embed list and slice |
| notification-core | 015 | 015 | |

---

## 2. Protos: `apps/Model Plane/proto/model_plane/v1/*.proto`

### 2a. Files (W line counts; `Lines (L)` only where different)

| File | Lines | Services (start line; RPC count) | Enums (start line) | Messages | Notes |
|---|---|---|---|---|---|
| browser.proto | 114 | BrowserBroker (:11; 4) | | 8 | AcquireGrant / RevokeGrant / ValidateGrant / Health. First message AcquireGrantRequest :25 |
| browser_agent.proto | 236 | BrowserAgentService (:11; 3) | ActionType :23, ObservationStatus :50, AgentPlanStatus :68 | 9 | |
| capabilities.proto | 205 | CapabilityCore (:13; 6) | | 12 | ListCapabilities, EvaluatePolicy, PromoteSkill, etc. |
| eventlog.proto | 116 | EventLog (:26; 3) | | 7 | |
| events.proto | 185 | | EventType :59 | 1 | Only one message |
| execution.proto | 377 | ExecutionCore (:13; 6) | | 10 | RunAgentRequest :76, RunAgentResponse :212 |
| finetune.proto | 156 | FinetuneJobs (:20; 6) | | 10 | |
| gateway.proto | 1514 | ModelGateway (:17; 49) | | 120 | OrgPolicy :1343; GetPolicyRequest :1357; SetPolicyRequest :1367; ThreadMessage :1383 |
| ids.proto | 58 | | | 8 | ULID newtypes |
| inference.proto | 1351 | InferenceCore (:11; 20) | PrivacyTier :85 | 52 | |
| memory.proto | 220 | MemoryService (:12; 5) | MemoryProvenance :114 | 11 | |
| orchestration.proto | 1289 | OrchestrationCoreService (:16); OrchestratorWorkflowService (:134); 24 RPC total | PlanState :154, PlanStepState :178, **ApprovalKind :194**, ApprovalState :210, TodoState :224, TodoPriority :240, SubagentRole :254, ApprovalContinuationOutcome :843, ApprovalDeliveryAcknowledgement :878 | 53 | **Approval :340**, Todo :384 |
| proof_bundle.proto | 230 | | | 11 | No service or enum |
| routing_policy.proto | 50 | RoutingPolicy (:23; 2) | | 3 | |
| runs.proto | 313 | RunService (:41; 9) | AutonomyRung :22 | 17 | |
| sandboxes.proto | 657 | SandboxManager (:12; 14) | SandboxLifecycleState :97, ProcessState :288, ProcessSignal :310, ProcessStream :320 | 36 | |
| sessions.proto | 1207 (**L: 1249**) | SessionCore (:16; 32 with ManagedRunLifecycle); ManagedRunLifecycle (:159). **L only:** SessionRetentionService (L:200; rpc CountWaivedRuns L:204) | ManagedRunSource :455, TerminalOutcome :474 | 66 (**L: 69**) | UpsertAgentSkillRequest :831 (L:842), ListAgentSkillsRequest :857, AgentSkill :865, ListConversationRequest :890 (L:901), SessionMessage :911, ListThreadsRequest :947 |
| verified_outcome.proto | 117 | | VerificationStatus :34 | 3 | |

Totals (W): 437 `message` declarations across 18 files; 183 RPCs across 14 service-bearing files. Five DP v2 protos sit under `proto/dataplane/` (documents_v2, graph_v1, knowledge_v2, retrieval_v2, wiki_v1). They are compiled by mp-contracts but are not in `model_plane/v1`.

### 2b. Highest used field numbers (for safe new fields)

| Message | Location (W; L if different) | Fields used | Highest | Next free | Notes |
|---|---|---|---|---|---|
| **RunAgentRequest** | `execution.proto:76-209` | 1–17 | **17** `subscription_connection_id` | **18** | `autonomy_rung = 12` is declared after `plan_mode = 13` (`:125-154`). No `reserved`. Renumbering warning at `:139-142`. |
| **ListConversationRequest** | `sessions.proto:890-907` (L:901-918) | 1–5 | **5** `space_read_decision_token` | **6** | No `reserved` |
| **UpsertAgentSkillRequest** | `sessions.proto:831-843` (L:842-854) | 1–9 | **9** `origin` | **10** | No `reserved` |
| **Approval** | `orchestration.proto:340-381` | 1–13 | **13** `org_id` | **14** | Comment at `:377-379` cites D-1 (IDOR-safe rehydrate) |
| **ApprovalKind** (enum) | `orchestration.proto:194-207` | 0–5 | **5** `APPROVAL_KIND_COST` | **6** | Enum values, not fields |
| **OrgPolicy** | `gateway.proto:1343-1355` | 1–6 | **6** `rate_limit_rpm` | **7** | Wrapped by Get/SetPolicy (`:1357-1375`) |

---

## 3. buf configuration, breaking rule, CI, codegen

- **`apps/Model Plane/proto/buf.yaml`** (13 lines): `version: v2`. Module `buf.build/triodelab/model-plane` (`:4`). Lint `STANDARD` + `COMMENTS`, except `PACKAGE_VERSION_SUFFIX` (`:5-10`). **Breaking rule `FILE`** (`:11-13`).
- **`apps/Model Plane/proto/buf.gen.yaml`** (40 lines): `managed.enabled` (`:2-3`). Rust deliberately absent, because mp-contracts `build.rs` handles it (`:5-10`).
  - Go: `protocolbuffers/go` (`:13-16`) and `grpc/go` (`:17-20`), output `../go/gen`, `paths=source_relative`.
  - Python: `protocolbuffers/python` (`:23-24`), output `../python/mp-events-py/src/mp_events/_gen`.
  - TS: `bufbuild/es` (`:31-35`) and `connect-es` (`:36-40`), output `../../Frontend Plane/verevon/src/lib/model-plane/gen`.
- **`apps/Model Plane/proto/buf.gen.go-only.yaml`** (15 lines): Go-only, same two Go plugins (`:8-15`).
- **Breaking baseline:** `apps/Model Plane/proto/baseline.binpb` exists. Regenerate only after a reviewed, intentional breaking change: `buf build "apps/Model Plane/proto" -o "apps/Model Plane/proto/baseline.binpb"` (`apps/Model Plane/proto/README.md:7-11`).
- **CI job:** `.github/workflows/buf.yml` at repo root.
  - Triggers on push and PR to main/develop for `apps/Model Plane/proto/**` (`:3-13`).
  - `bufbuild/buf-setup-action@v1` (`:19`).
  - `buf lint apps/Model Plane/proto` (`:20-21`).
  - `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (`:22-23`).
  - **Both run lines are unquoted.** Bash will split the path on the space. Static reading only; not run.
- **Gate test:** `apps/Model Plane/go/pkg/cibuf/workflow_test.go:69-79` asserts the YAML text contains `buf lint`, `buf breaking`, and a baseline reference. Text check only.
- **Lint status, conflicting docs:** `apps/Model Plane/CLAUDE.md:42` says lint has pre-existing debt. `apps/Model Plane/docs/decisions/ledger.md:2479` says 1,502 errors across 20 protos. `apps/Model Plane/docs/decisions/ledger.md:2481-2483` says the baseline is configured, CI-wired and committed. `apps/Model Plane/MODEL_PLANE_STATUS.md:151` says a usable breaking baseline is missing. Current truth: not determined.
- **Exact regen command:** `cd "apps/Model Plane/proto" && buf generate` (`apps/Model Plane/CLAUDE.md:42`; `apps/Model Plane/README.md:170` says `cd proto && buf generate`).
  - Go only: `buf generate --template buf.gen.go-only.yaml --path model_plane/v1/<file>.proto`. `apps/Model Plane/docs/decisions/ledger.md:3769-3771` describes this approach. The full flag line is standard buf CLI, not re-run here.
  - Remote plugins (`remote: buf.build/...`, `buf.gen.yaml:13,17,23,31,36`) need buf registry access.
- **Generated code locations:**
  - **Go:** `apps/Model Plane/go/gen/` (checked in). `apps/Model Plane/go/gen/go.mod:1` is `module github.com/triodelab/model-plane/gen/go`. Files are `*.pb.go` and `*_grpc.pb.go`. Parity tests: `go/gen/model_plane/v1/proto_wire_parity*_test.go`.
  - **Python:** `apps/Model Plane/python/mp-events-py/src/mp_events/_gen/` (checked in). Header at `_gen/__init__.py:3-4`.
  - **TS:** `apps/Frontend Plane/verevon/src/lib/model-plane/gen/` (checked in, `*_pb.ts` and `*_connect.ts`). **No file under `apps/Frontend Plane/verevonv3/src` imports `model-plane/gen`** (grep, no matches). The output targets the legacy `verevon` tree.
  - **Rust:** nothing checked in. `rust/crates/mp-contracts/build.rs:24-103` runs `tonic_build` over `proto/` into OUT_DIR. `src/lib.rs:17-20` (L:19-20) pulls it in with `include_proto!("model_plane.v1")`.
  - **`clients/`:** does not exist. `apps/Model Plane/` top level is `api, bridges, deploy, docs, go, proto, python, rust, scripts`.

---

## 4. mp-contracts and the other mp-* crates

### 4a. `apps/Model Plane/rust/crates/mp-contracts` (cites relative to this dir; W; L differences in brackets)

- **`src/lib.rs`:** `pub mod agentic_subscription` (:1), `autonomy` (:2), `dataplane_posture` (:3), `skill_recovery` (:4), `tool_arguments` (:5). `pub mod model_plane` (:10) → `pub mod v1` (:17) → `include_proto!("model_plane.v1")` (:18). `pub mod dataplane` (:23) with retrieval_v2, documents_v2, knowledge_v2, graph_v1, wiki_v1 (:30-71). **[L: +`pub mod retention;` at L:4, +`pub mod zdr_refusal;` at L:7; status 146 M.]**
- **`build.rs`:** `mp_protos` list `:37-58` (16 files). `dp_protos` `:60-66` (5 files). `tonic_build` `:100-103`. Docker cache and fingerprint note `:74-94`.
- **Modules:**
  - `agentic_subscription.rs`: kill switch for agentic (Do-mode) runs on the ChatGPT-subscription provider, shared by three services (`:1-6`).
  - `autonomy.rs`: autonomy ladder and well-formed widening requests, shared by model-gateway and execution-core (`:1-6`).
  - `dataplane_posture.rs`: sovereignty posture for DP v2 retrieval, `sovereign_required` = proto field 14 (`:1-14`). **[L: modified, +67.]**
  - `skill_recovery.rs`: read back a truncated or dropped skill in full (`:1-6`).
  - `tool_arguments.rs`: validate tool-call args against the declared schema, fail-open (`:1-45`).
  - **[L only]** `retention.rs`: ZDR retention resolver. Pure decision, no I/O (`L:1-6`). Status 303.
  - **[L only]** `zdr_refusal.rs`: typed ZDR refusal catalogue (`L:1-6`). Status 304.
- **Tests:** W: `tests/proto_wire_parity.rs`, `tests/proto_wire_parity_execution.rs`, `tests/grounding_replay.rs`. **[L only]** `tests/zdr_retention_matrix.rs` and `tests/fixtures/` (status 305-306).

### 4b. Other mp-* crates (`rust/crates/`)

| Crate | Purpose (lib.rs header) | Modules under src/ |
|---|---|---|
| mp-authctx | Auth context: validates auth-core JWT against JWKS; observe and enforce modes (`lib.rs:1-25`) | claims, jwks, layer |
| mp-events | Event envelope, NATS subject helpers, idempotency-key hashing (`lib.rs:1`) | envelope, idempotency, jetstream, publisher, subjects, temporal |
| mp-eventlog | Shared domain types for the EventLog service; filter validation (`lib.rs:1-6`) | cursor, error, filter, idempotency |
| mp-ids | Typed ULID newtype identifiers with validation (`lib.rs:1-5`) | lib only |
| mp-orchestration | Canonical record types: plans, todos, approvals, subagent lineage; owner session-core (`lib.rs:1-6`) | approval, error, plan, subagent, todo, proto_shim, run_event |
| mp-slo | SLO harness: first-token p95 under 200 ms, step throughput (`lib.rs:1-6`) | harness, histogram, percentile, recorder, slo |
| mp-telemetry | tracing-subscriber JSON plus optional OTLP exporter (`lib.rs:1-11`) | lib only |
| mp-tokens | o200k_base token counting for budget gates (`lib.rs:1-40`) | lib only |
| mp-toon | TOON display encoding, lossy and not reversible (`lib.rs:1-16`) | lib only |

---

## 5. Typed SSE ChatEvent taxonomy

### 5a. Locations

**Producer:** `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` (W; L in brackets)

- `pub enum ChatEvent` at `:105` (21 variants, `:108` to `:337`). **[L: `:144`, plus `RetrievalNotice` L:368 and `ZdrError` L:384.]**
- `family()` `:351-382`: exhaustive match, no wildcard. Control group (family `None`) at `:365-380`. **[L: `:410`; arms L:438-441.]**
- `should_emit()` `:389-394`. **[L: `:451`.]**
- `name()` `:398-421`. `Error` and `SubscriptionError` both map to `"error"` (`:419`). **[L: `:460`; `retrieval_notice` L:480; `ZdrError` grouped under `"error"` L:483.]**
- `payload()` `:425`. **[L: `:490`; RetrievalNotice L:672; ZdrError L:688.]**
- `to_sse()` `:639-643`. **[L: `:732`.]**
- Gate application: `emit()` at `:933` records citations and artifacts before the gate (`:934-968`). The gate is `if !event.should_emit(&self.features) { return; }` at `:969-971`. **[L: `emit` at `:1026`.]**
- Client opt-in: `features[]` request field, `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:6680-6684`. Empty means plain stream.

**Consumer:** `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts` (W; L in brackets)

- `dispatchAgUiEvent` at `:1149` (AG-UI adapter, lowercase `case` labels `:1288-1411`). **[L: `:1179`.]**
- `dispatchEvent` at `:1429` (native). AG-UI branch `:1449-1454`. Native `switch (event.event)` at `:1456`. Native cases `:1457-1707`. `default:` `:1708-1722` calls `handlers.onUnknownEvent` at `:1719`. **[L: `:1460`, switch L:1487, `case 'retrieval_notice'` L:1740.]**
- `onUnknownEvent?` hook type at `:664`. **[L: `:681`.]**
- Native case labels (W): connected 1457, chunk 1466, done 1471, stopped 1479, error 1494, artifact 1514, attachment 1523, reasoning_delta 1533, tool_call 1536, tool_result 1543, citation 1551, source_scope_applied 1554, citations (legacy) 1566, search_results (legacy) 1567, grounding 1576, step_update 1579, awaiting_approval 1588, usage 1600, turn_timing 1613, result_receipt 1620, title 1627, follow_ups 1632, plan_proposed 1641, verification 1664, memory_recall 1679, queued_input 1693.

**Generated doc:** `apps/Model Plane/docs/sse-event-taxonomy.md`. Header at `:3-5` says "GENERATED FILE, do not edit by hand". W has 19 table rows (`:21-39`) and 5 SPA-only rows (`:45-49`).

**Generator:** `apps/Model Plane/scripts/gen-sse-event-taxonomy.py`

- Roots: `REPO_ROOT = parents[3]` (`:36`). `PRODUCER` `:37`, `CONSUMER` `:38`, `DOC` `:39`.
- Transport-only names allowed in the SPA: `NON_CHATEVENT_CONSUMER_CASES` `:43-49` (connected, chunk, done, citations, search_results). Still-present check `:211-218`.
- Parsing: `VARIANT_ARM` regex `:81-83`. `parse_arms` `:117-127`. Reads the `name()` body (`:187`) and `family()` body (`:188`).
- Every name arm needs a family arm: `:190-195`.
- Every produced name needs `case '<name>':` in chat-client.ts: `consumer_cases` `:130-134` (regex over the whole file), undropped check `:197-209`.
- `render` `:137-178`. Write mode `:220-225` (rewrites the doc). `--check` diff `:227-244`.

**CI gate:** `.github/workflows/sse-taxonomy.yml`

- Triggers on push and PR for five paths (`:16-30`): sse_events.rs, the doc, the script, chat-client.ts, the workflow itself.
- Check step `:44-45`: `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`.
- Drift-proof step `:49-62`: deletes the `memory_recall` row and expects the check to fail.

### 5b. Add a variant, end to end

1. **Rust variant:** add to `pub enum ChatEvent` (W `:105`), with a doc comment.
2. **`family()`** (`:351-382`): add an arm. `Some("<family>")` makes it opt-in; the control group `None` (`:365-380`) makes it always-emit. No wildcard, so the compiler rejects a missing arm. The script also requires it (`:190-195`).
3. **`name()`** (`:398-421`): add the SSE `event:` name. The script does not check uniqueness; the doc shows two `error` rows by design.
4. **`payload()`** (`:425`): add the JSON body, including `request_id` as the other arms do.
5. **Gating is automatic** in `emit()` (`:969-971`). A family event reaches a client only if the client sends that family in `features[]` (`http_routes.rs:6680-6684`).
6. **SPA:** add `case '<name>':` to `dispatchEvent`'s native switch (`:1456`, before `default` at `:1708`). Add a handler to the `ChatStreamHandlers` type (the `onUnknownEvent` hook sits at `:664`). Wire the controller, per the script's own error text (`gen-sse-event-taxonomy.py:207-208`). The controller is `apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts`; not inspected.
   - **Gate weakness:** a `case` placed in `dispatchAgUiEvent` (`:1149-1428`) satisfies the gate, because the regex reads the whole file. Native frames never reach that switch.
7. **Regenerate:** `python3 'apps/Model Plane/scripts/gen-sse-event-taxonomy.py'`. This rewrites `apps/Model Plane/docs/sse-event-taxonomy.md` (script `:221-225`). Then confirm `--check` passes (`sse-taxonomy.yml:45`).
8. **Transport-only name** (not a ChatEvent): add to `NON_CHATEVENT_CONSUMER_CASES` (script `:43-49`).
9. **Commit source and doc together.** The workflow fires on the five watched paths (`sse-taxonomy.yml:16-30`).

### 5c. Verified state (this audit, check mode, `PYTHONUTF8=1`)

- **W (afb83928): exit 1.** Diff: `plan_proposed` (`ChatEvent::PlanProposed`) and `source_scope_applied` (`ChatEvent::SourceScopeApplied`) are missing from the committed doc. Doc has 19 rows; source yields 21.
- **L (live tree): exit 0.** `sse taxonomy up to date (23 events)`. Live doc adds 4 rows: `error`/ZdrError, `plan_proposed`, `retrieval_notice`, `source_scope_applied` (`git diff`).

---

## 6. Live-only contract deltas (from `live-tree-status.txt`)

| Area | Live change | Status lines | Line effects |
|---|---|---|---|
| session-core migrations | +0044, +0045, +`revert/` (two `.down.sql`) | 320-322 | sqlx skips `revert/` (sqlx-core `source.rs:84-87`). Live next free = 0046. |
| session-core src | +`retention_receipt.rs` (untracked) | 323 | none on migrations |
| sessions.proto (M) | +`SessionRetentionService` (L:196-206 incl. comments; service L:200; rpc L:204). Three messages appended at L:1219-1249: CountWaivedRunsRequest L:1223, CountWaivedRunsResponse L:1230, WaivedRunCount L:1237. | 144 | W lines from 196 on shift +11. File total 1249 (W 1207). UpsertAgentSkillRequest and ListConversationRequest move to L:842 and L:901. |
| mp-contracts (M) | lib.rs +2 module lines; dataplane_posture.rs +67 | 145-146 | lib.rs include_proto moves to L:19-20 |
| mp-contracts (new) | retention.rs, zdr_refusal.rs, tests, fixtures | 303-306 | |
| SSE (M) | sse_events.rs (~+200), chat-client.ts (+53), taxonomy doc (+4 rows) | 177, 84, 143 | gate passes in L |
| SSE tests (new) | chat-client.zdr-refusal.test.ts | 272 | |
| API contracts | api/contracts/README.md (M); zdr-refusals.json + .sha256 (new) | 140, 301-302 | |
| SPA contracts (new) | src/shared/api/contracts/; zdr-refusals.ts | 273, 277 | |
| CI (M) | model-plane-rust-ci.yml adds a receipt-contract job against a migrated DB (L:124-167; REQUIRE_CONTRACT L:150; ">= 8 tests" check L:166) | 3 | |
| buf / baseline | Unchanged in L (only sessions.proto under proto/ is modified) | | |

---

## 7. Risks and collision rules

1. **Next-free numbers differ by tree.** Session-core is 0044 at HEAD and 0046 live. Any branch from HEAD that takes 0044 collides with the live set when it lands. Coordinate, or take 0046 and above.
2. **Proto edits are add-only.** Never renumber a field already on the wire (`execution.proto:139-142` precedent). Use the next-free numbers in 2b. The messages listed have no `reserved` ranges.
3. **Go services re-run every `*.up.sql` at every boot** (no ledger; see 1a and `apps/Model Plane/go/services/cost-core/internal/postgres/store_integration_test.go:311`). New migrations must be idempotent (`IF NOT EXISTS`, `ON CONFLICT`). capability-core also aborts if DATABASE_URL is unset (`apps/Model Plane/go/services/capability-core/docker-entrypoint.sh:4-7`).
4. **auth-core checksums applied files.** Editing an applied migration aborts boot (`apps/Control Plane/auth-core/docker-entrypoint.sh:154-158`). Add a new numbered file instead.
5. **auth-core bootstrap file is missing.** `init_better_auth.sql` is referenced at `apps/Control Plane/auth-core/docker-entrypoint.sh:181` and `apps/Control Plane/auth-core/src/auth/deployment-contract.spec.ts:261`. It is absent in W and L, and not gitignored (`git check-ignore` exit 1). The most recent add-or-delete event for the path is commit 932b30d2 (2026-09-23), which is an ancestor of HEAD. The file is absent at HEAD, so that commit deleted it. The entrypoint skips it silently (`:184`). Effect on a fresh database: not run.
6. **audit-core ignores new files in its folder.** A new migration must be added to the `//go:embed` list and the `migrations` slice in `apps/Control Plane/audit-core/internal/store/migrate.go` (`:12-28`, `:36-43`).
7. **Duplicate prefixes.** capability-core 0013 x2; user-core 008 x2 and 021 x2. Apply order is by full filename. Ledgers in org-core and user-core key on the full name, so no ledger collision. Do not add a third.
8. **Release snapshot is partial.** `apps/Model Plane/scripts/release-artifact.sh:96-101` snapshots only session-core, capability-core, cost-core and letta-bridge. sandbox-manager and orchestrator-core migrations are not in the manifest.
9. **Two services named session-core.** Model Plane `rust/services/session-core` and Control Plane `apps/Control Plane/session-core`. Keep the paths distinct in any script or CI filter.
10. **buf CI likely never enforced.** The unquoted paths at `buf.yml:21,23` would break the job. Do not cite the proto gate as passing until a run confirms it.
11. **SSE gate is a text check.** Any `case '<name>':` in chat-client.ts passes (`gen-sse-event-taxonomy.py:130-134`), including one in the AG-UI switch. Verify the case sits in `dispatchEvent`.
12. **HEAD SSE doc is stale.** `--check` fails in W (see 5c). Any PR touching the five watched paths will fail until the doc is regenerated. The fix exists only in the live working tree (status 143).
13. **buf generate writes TS to `verevon/`**, not `verevonv3/` (`buf.gen.yaml:31-40`). No verevonv3 file imports those bindings.
14. **Rust protos rebuild by fingerprint.** After a proto edit, Docker builds can miss the change (`mp-contracts/build.rs:74-94`). Editing build.rs or dropping the cache mount is the documented workaround.

---

## 8. Not determined

- Whether `buf lint` and `buf breaking` pass today (not run; buf CLI not invoked).
- Whether CI has been green recently (no CI log access).
- Whether sqlx rejects duplicate integer prefixes in session-core (not tested; none exist at HEAD).
- Whether the missing `init_better_auth.sql` breaks a fresh deploy (not run).
- Whether live migrations 0044 and 0045 are complete and correct (not read).
- Whether `apps/Frontend Plane/verevon` is still built or consumed (not checked).
- Whether `use-chat-controller.ts` already handles every native case (not inspected).
