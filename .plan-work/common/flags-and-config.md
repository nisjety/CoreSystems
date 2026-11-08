# Feature flags, config and rollout patterns (digest)

Read-only research. Nothing executed, nothing edited outside this file.

## Method and citation key

- **W** = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/` at HEAD afb83928 (clean). Plain `path:line` cites are W unless marked.
- **L** = live tree `C:/dev/CoresSystem/` (same HEAD plus uncommitted entries listed in `.plan-work/live-tree-status.txt`). `[L:n]` = same file, different line in L. `[live-only]` = absent in W. `[M-live]` = file modified in L, lines not re-checked.
- `FE` = `apps/Frontend Plane/verevonv3/`. `MP` = `apps/Model Plane/`. `CP` = `apps/Control Plane/`. Paths below are relative to W unless a root is named.
- Compose service ranges in `MP/deploy/docker-compose.yml` (from `^  name:` grep): model-gateway 191-711, session-core 712-807, inference-core 808-1004, execution-core 1005-1280, orchestrator-core 1281-1377, capability-core 1378-1574, sandbox-manager 1575-1611, browser-broker 1612-1629, letta-bridge 1630-1668, agent-memory-server 1680-1738, cost-core 1739-1815, bridge-core 1816-1842.

## 0. Bottom line

- **No flag registry and no flag SaaS.** Effective truth is the code default constant, the compose `${VAR:-default}` line, and a comment. `MP/deploy/.env.example` covers only a handful of flags. Grep for launchdarkly/unleash/growthbook/posthog/flagsmith/statsig/openfeature/flipt/configcat: 0 hits in `FE/src` and `CP/org-core`. Repo-wide grep timed out; MP and Go not exhaustively checked for SaaS SDKs.
- **Six or more truthy vocabularies in one plane** (section 1.2). A new flag copies whichever neighbour it sits next to.
- **Defaults disagree across layers** for at least four flags (section 1.5): LEARNING_REVIEW_ENABLED, DREAMING_LLM_EXTRACTION, AGENTIC_SUBSCRIPTION_ENABLED (snapshot), Quarry AUTOPROMPT (W vs L).
- **Do mode (OD2)** is one server env switch, `AGENTIC_SUBSCRIPTION_ENABLED`, default off in compose. It is advertised as a model capability, enforced in execution-core, and the composer follows the capability (section 3).
- **Frontend**: build-time `VITE_*` flags only for auth bypass, dev, and demo. Server-controlled features come from API capabilities, not VITE flags.
- **Control Plane**: user flags are typed settings with defaults in the handler. Org flags are entitlements (stored rows plus plan-computed ZDR). The ZDR lapse rule diverges between the contract matrix and code (section 5.2).
- **Rollout** = default-off in compose, then owner decision, then a dated live proof on a proof stack, then a signed release artifact for security-relevant keys. No percentage or cohort canary exists in code. The `mp.cutover` ladder in `MP/docs/CUTOVER.md` is documentation only (section 7).

## 1. Model Plane (Rust and Go) env flags

### 1.1 Naming conventions

- `<SCOPE>_ENABLED` for on/off features (majority). Examples: `WIDE_RESEARCH_FAN_OUT_ENABLED`, `EXECUTION_POST_MUTATION_VERIFY_ENABLED`, `LEARNING_RUN_EVENTS_ENABLED`, `MEMORY_GROUNDING_ENABLED`.
- `*_MODE` for enums: `LETTA_TOOL_SEARCH_MODE`, `MP_COMPAT_MODE`, `CONTROL_PLANE_ENFORCEMENT` (gateway).
- `*_ALLOW_*` for deny-by-default permissions: `MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT` (`MP/rust/services/inference-core/src/config.rs:276`), `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS` (`MP/rust/crates/mp-contracts/src/agentic_subscription.rs:47`), `LETTA_TOOL_SEARCH_ALLOW_INSECURE_LOOPBACK`.
- `*_ZDR_CONFIRMED` plus five attestation fields (`*_ZDR_RESOURCE_ID`, `_APPROVAL_REF`, `_EFFECTIVE_DATE`, `_REVIEW_BY`, `_REVIEWER`) and a recomputed `_DIGEST`: evidence-bound, not a boolean (`MP/deploy/docker-compose.yml:936-951`).
- Ingestion-only config-crate switches: `QUARRY_EDGE__<SWITCH>` with a written-out default (`apps/Ingestion Plane/docker-compose.yml`, L:129-136).
- Constant pattern: `pub const ENABLED_ENV: &str = "..."` in the owning module, so a test can assert the name. Examples: `MP/rust/services/execution-core/src/subagent/research_fan_out.rs:42`, `.../runtime_loop/post_mutation.rs:58`, `MP/rust/services/session-core/src/learning_events.rs:54` [L:55], `MP/rust/services/session-core/src/memory_grounding.rs:90`, `MP/rust/services/model-gateway/src/prompt_context.rs:26,30,35`.

### 1.2 Parse vocabularies in use (same plane)

| # | Vocabulary (trimmed, lowercased unless noted) | Used by | Unset means |
|---|---|---|---|
| A | falsey opt-out: `0/false/no/off` | `MP/rust/services/session-core/src/learning_events.rs:74-79`; `MP/rust/services/inference-core/src/provider/logprobs.rs:41-49`; `dream_extractor.rs:771-780` (`env_flag`, default param) | ON |
| A2 | falsey opt-out: `0/false/off/no/disabled` | `MP/rust/services/execution-core/src/subagent/research_fan_out.rs:67-72`; `MP/rust/services/inference-core/src/config.rs:227-234` (without `disabled`) | ON |
| B | truthy opt-in: `1/true/yes/on` | `MP/rust/services/session-core/src/memory_grounding.rs:106-111`; `dream_extractor.rs:773-776` | OFF |
| C | truthy opt-in: `1/true/on/yes/enabled` | `MP/rust/services/model-gateway/src/prompt_context.rs:162-171`; `MP/rust/services/execution-core/src/runtime_loop/post_mutation.rs:82-88` | OFF |
| D | exact `"true"` only (case-sensitive) | `MP/rust/services/execution-core/src/promote_on_use.rs:39-44` | OFF |
| E | exact `"1"` only | `MP/rust/services/model-gateway/src/finetune_routes.rs:315-317`; `finetune_poller.rs:499` | OFF |
| F | `1/true/TRUE/yes` (no `on`) | `MP/rust/services/execution-core/src/quarry_agent.rs:524-526` | OFF |
| G | `1/true` only | `inference-core/src/config.rs:360-362` (legacy ZDR), `:393-397` (`truthy_env`, deny-by-default globals) | OFF |
| H | exact `!= "false"` opt-out (`"0"` keeps it ON) | `MP/go/services/capability-core/cmd/main.go:286,320,365`; `MP/go/services/sandbox-manager/internal/process/sweeper.go:99-100` | ON |
| I | strict `true`/`false`, error on anything else | `MP/go/services/capability-core/internal/lettatools/client.go:243-254` | default param (false) |
| J | `(enabled, recognized)` with slog warn on unknown | `MP/go/services/capability-core/cmd/main.go:749-775` (off: `false/0/no/off/disabled`; on: `true/1/yes/on/enabled`) | ON |
| K | enum with fail-safe fallback | `MP_COMPAT_MODE` (`MP/go/pkg/natsx/mode.go:49-68`, unknown gives `v1_only`); gateway `CONTROL_PLANE_ENFORCEMENT` (`FE/apps/gateway/src/config.rs:160-164`, unknown gives `off`) | fallback |
| L | frontend exact string compare | `FE/src/shared/api/http.ts:40` [L:46] and `sse.ts:20` (`=== 'true'`); `FE/vite.config.ts:18` (`process.env.VEREVON_VITE_MCP_ENABLED === 'true'`) | off |

Consequence: `"0"` disables in A and A2, is ignored in H (stays ON), and is simply OFF in B/C/E. `"yes"` enables in B, C and F, is not falsey in A (so a default-on flag stays ON), and does nothing in D or E.

### 1.3 Flag table (default-in-code, default-in-deploy, where declared)

Compose default = the `${VAR:-x}` fallback in `MP/deploy/docker-compose.yml` unless stated. "not declared" = no hit in `MP/deploy/` (grep-verified).

| Flag | Owner (code) | Code default and parse | Compose default and service | Notes |
|---|---|---|---|---|
| LEARNING_REVIEW_ENABLED | `MP/go/services/capability-core/cmd/main.go:747-775, 786-793` | ON when unset (`:749-752`); vocabulary J | `:-false` at `compose:1429` (capability-core) | Compose overrides the code default. Go test `MP/go/services/capability-core/cmd/learning_review_switch_test.go` exists (not read). `:1426-1428` comment explains the kill-switch intent. |
| LEARNING_RUN_EVENTS_ENABLED | `MP/rust/services/session-core/src/learning_events.rs:54-70` [L:55] | ON unless vocabulary A | not declared | Session-core run-completed outbox. Cannot be switched off via repo compose. |
| AGENTIC_SUBSCRIPTION_ENABLED | `MP/rust/crates/mp-contracts/src/agentic_subscription.rs:40-41, 94-97` | OFF unless explicit on-word (`:40`) | `:-false` at `compose:378` (model-gateway), `:847` (inference-core), `:1038` (execution-core) | Do-mode master switch. Section 3. |
| AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS | `agentic_subscription.rs:43-47` | OFF; grants nothing while ENABLED is off (`:43-46`) | `:-false` at `compose:379` (model-gateway), `:1039` (execution-core) | Second switch. Until plan approval is wired, subscription runs are read-only by construction (`:43-46` comment). |
| WIDE_RESEARCH_FAN_OUT_ENABLED | `MP/rust/services/execution-core/src/subagent/research_fan_out.rs:38-42, 67-72` | ON unless vocabulary A2 | `:-true` at `compose:1073` (execution-core) | Kill switch for subscription rate limits (comment `compose:1064-1065`). Concurrency default 4, cap 16 (`research_fan_out.rs:57,62`). |
| EXECUTION_POST_MUTATION_VERIFY_ENABLED | `MP/rust/services/execution-core/src/runtime_loop/post_mutation.rs:56-58, 82-88` | OFF unless vocabulary C; read at each write (`:56-57`) | `:-false` at `compose:1062` (execution-core) | Read per call, but a running process cannot see a changed env, so a restart is still needed. |
| QUARRY_BROWSER_AGENT_ENABLED | `MP/rust/services/execution-core/src/quarry_agent.rs:519-530` | OFF unless vocabulary F; also needs `QUARRY_EDGE_URL` (`:530`) | `:-1` at `compose:1189` (execution-core) | Compose default ON differs from code default OFF. |
| VEREVON_INTENT_ENABLED | `MP/rust/services/inference-core/src/config.rs:226-234` | ON unless vocabulary A2 (no `disabled`) | `:-1` at `compose:856` (inference-core) | Verevon intent layer. Router policy refresh is `ROUTER_POLICY_REFRESH_SECS` (`config.rs:255-258`, default 60; compose `:861,866`). |
| INFERENCE_LOGPROBS_ENABLED | `MP/rust/services/inference-core/src/provider/logprobs.rs:41-49` | ON unless vocabulary A | not declared | Default ON in code, no compose passthrough. |
| PROMOTE_ON_USE_ENABLED / _EXECUTE | `MP/rust/services/execution-core/src/promote_on_use.rs:39-44` | OFF; exact `"true"` (vocabulary D) | not declared | Case-sensitive: `TRUE` does nothing. |
| MEMORY_GROUNDING_ENABLED | `MP/rust/services/session-core/src/memory_grounding.rs:84-111` | OFF unless vocabulary B | `:-0` at `compose:774` (session-core) | Comment `compose:769-773`. `.env.example:156` sets `0`. |
| DREAMING_LLM_EXTRACTION | `MP/rust/services/session-core/src/dream_extractor.rs:346, 771-780` | ON (`env_flag(..., true)`) | `:-0` at `compose:767` (session-core) | Comment `compose:756-766` says OFF since 2026-10-01 (paid model on user chats). `.env.example:147` says `=1`. See 1.5. |
| DREAMING_LLM_MODEL / MEMORY_GROUNDING_MODEL | `dream_extractor.rs:417` | `claude-haiku-4-5` | `compose:768`, `:775` | Model name, not a switch. |
| FINETUNE_ENABLED | `MP/rust/services/model-gateway/src/finetune_routes.rs:43, 315-332`; `finetune_poller.rs:51, 499` [L:310 for feature_enabled] | ON only for exact `"1"` (vocabulary E) | `:-0` at `compose:448` (model-gateway); dev override `MP/deploy/docker-compose.override.yml:14` sets `"1"` | Route gate returns 503 with hint `set FINETUNE_ENABLED=1` (`finetune_routes.rs:319-332`). |
| MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER | `MP/rust/services/model-gateway/src/prompt_context.rs:23-26, 88-129, 134-138, 162-171` | OFF unless vocabulary C; applies only to OpenAI-API chat-completions routes (`:134-138`) | `${...:-}` at `compose:415` (model-gateway) | Doc comment at `:23-25` lists `1/on/true/yes/enabled`. Read once via `OnceLock` (`:119-129`). Like every env flag, a change needs a restart. |
| MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET | `prompt_context.rs:29-30, 40-41, 173-178` | unset or 0 means message-count compaction; clamped to 1000 to 200000 | `compose:416` | Numeric, not boolean. |
| MODEL_GATEWAY_DEDUP_CONTEXT_HISTORY | `prompt_context.rs:32-35` | OFF unless vocabulary C | `compose:417` | Same pattern as the stable-order flag. |
| ALLOWED_MODELS | `MP/rust/services/model-gateway/src/normalize.rs:84-93, 366-367` | unset = all models; empty string = all models too (see 1.5) | not declared in `compose`; `MP/deploy/docker-compose.override.yml:12` sets `""` (dev) | Production allowlist is not in repo compose. |
| MP_COMPAT_MODE | `MP/go/pkg/natsx/mode.go:49-68`; `MP/rust/crates/mp-events/src/subjects.rs:47` | default `v1_only`; unknown gives `v1_only` (canonical v1 path) | literal `v1_only` at `compose:217` (model-gateway) and `:1305` (orchestrator-core); dev override `override.yml:55` | Fail-closed to the canonical path. |
| TASK_EXECUTOR_ENABLED | `MP/go/services/capability-core/cmd/main.go:716-740` | tri-state: `false` off; `true` on even without workflow dispatcher (logs error); unset on only if workflow-backed (`:738`) | `${...:-}` at `compose:1530` (capability-core), comment `:1531` | Three states, unlike other flags. |
| MCP_DNS_REVALIDATOR_ENABLED / CRON_SWEEPER_ENABLED / WATCH_SWEEPER_ENABLED | `capability-core/cmd/main.go:286, 320, 365` | ON unless exactly `"false"` (vocabulary H) | not declared | Default-on background jobs (MCP DNS revalidator, cron and watch sweepers). |
| PROCESS_SWEEPER_ENABLED | `MP/go/services/sandbox-manager/internal/process/sweeper.go:99-100, 121` | ON unless exactly `"false"` (vocabulary H) | not declared | Same. |
| LETTA_TOOL_SEARCH_ENABLED / _MODE | `MP/go/services/capability-core/internal/lettatools/client.go:91, 128, 243-254` | OFF; strict true/false (vocabulary I); mode default `hybrid` | `:-false` at `compose:1442`; `:-hybrid` at `:1445` (capability-core) | Contract test `MP/scripts/tests/letta-tool-search-compose-contract-test.sh` asserts 8 compose variables in the capability-core block. `.env.example:39-46`. |
| AZURE_OPENAI_ZDR_CONFIRMED / AZURE_ANTHROPIC_ZDR_CONFIRMED | `MP/rust/services/inference-core/src/config.rs:260-271, 355-369`; attestation in `MP/rust/services/inference-core/src/provider/zdr.rs:1-30, 41, 65, 109` | legacy boolean accepted only as `1/true` (`config.rs:360-362`) and it refuses boot without the attestation fields (`zdr.rs:1-15`) | `:-false` at `compose:936` and `:945`, five attestation fields plus digest each at `:937-942` and `:946-951` (inference-core) | Digest recomputed from the five fields, so a placeholder fails. `MP/docs/ZDR.md:216` says leave it false. |
| MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT / _ALLOW_GLOBAL_RESIDENCY | `inference-core/src/config.rs:275-277` (`truthy_env`) | deny-by-default (vocabulary G) | `:-false` at `compose:923` and `:926` | Global deployment voids the EU residency claim (`compose:921-922`). |
| MODEL_PLANE_ALLOW_NON_EU_EMBEDDING | `inference-core/src/config.rs:282` | deny-by-default (vocabulary G) | not declared | Hard reject, not warn. |

Gateway dev flags and Frontend flags are in section 4.

### 1.4 Where flags are declared for deployment

- **Canonical compose**: `MP/deploy/docker-compose.yml`, per-service `environment:` blocks (ranges above).
- **Operator template**: `MP/deploy/.env.example`. Covers only: LETTA_TOOL_SEARCH_* (`:39-46`), DREAMING_LLM_EXTRACTION=1 (`:147`), DREAMING_LLM_MODEL (`:148`), MEMORY_GROUNDING_ENABLED=0 (`:156`), MEMORY_GROUNDING_MODEL (`:157`), FINETUNE_POLLER_* (`:173-174`). Grep-verified absent: AGENTIC_*, LEARNING_*, VEREVON_INTENT_ENABLED, WIDE_RESEARCH_*, EXECUTION_POST_MUTATION_*, QUARRY_BROWSER_AGENT_ENABLED, MODEL_GATEWAY_CACHE_*, FINETUNE_ENABLED, ALLOWED_MODELS, AZURE_* (any).
- **Dev-only overrides**: `MP/deploy/docker-compose.override.yml` lines 12 (`ALLOWED_MODELS: ""`), 14-16 (`FINETUNE_ENABLED: "1"` and budgets), 54-55 (`ENABLE_COMPAT_ADAPTER: "false"`, `MP_COMPAT_MODE: v1_only`), 67 (`SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT: "true"`). Included only when `MODEL_PLANE_DEV != 0` and `MODEL_PLANE_PRODUCTION != 1` (`MP/scripts/compose.sh:109-120`).
- **Env-file chain** (`MP/scripts/compose.sh`): canonical `deploy/.env` (`:96-100`). `../Control Plane/.env.generated-secrets` is loaded first (`:141-144`), so `deploy/.env` wins (`:145`). `MODEL_PLANE_RELEASE_LOCK` is appended last (`:146-155`). Production (`MODEL_PLANE_PRODUCTION=1`) refuses plain compose and requires `scripts/release-artifact.sh compose` (`:109-116`).
- **Not declared anywhere in repo deploy files** (process-env only): LEARNING_RUN_EVENTS_ENABLED, INFERENCE_LOGPROBS_ENABLED, PROMOTE_ON_USE_ENABLED (+_EXECUTE), MCP_DNS_REVALIDATOR_ENABLED, CRON_SWEEPER_ENABLED, WATCH_SWEEPER_ENABLED, PROCESS_SWEEPER_ENABLED, MODEL_PLANE_ALLOW_NON_EU_EMBEDDING, ALLOWED_MODELS (production). Grep-verified over `MP/deploy/` and the cited sources.

### 1.5 Verified semantic hazards

1. **LEARNING_REVIEW_ENABLED**: compose default `false` (`compose:1429`), code default ON when unset (`main.go:749-752`). Deployed = off; a fresh build with no env = on. The code test exists; the compose comment at `:1426-1428` documents the intent.
2. **DREAMING_LLM_EXTRACTION** has three values: code ON (`dream_extractor.rs:346`), compose `:-0` (`compose:767`), `.env.example=1` (`MP/deploy/.env.example:147`). Copying the template to `deploy/.env` turns on paid extraction, which the compose comment (`:760-766`) describes as off on this stack and to be enabled only by an explicit decision. Not determined whether operators copy the template.
3. **ALLOWED_MODELS**: unset and empty both mean allow-all (`normalize.rs:366-367` skips when the list is empty). The dev override's `""` is a no-op, not a deny-all.
4. **Vocabulary splits** (1.2): `FINETUNE_ENABLED` accepts only `1`; `PROMOTE_ON_USE_ENABLED` accepts only lowercase `true`; `QUARRY_BROWSER_AGENT_ENABLED` rejects `on`; Go `!= "false"` makes `"0"` leave sweepers ON.
5. **Default-ON with no compose passthrough**: LEARNING_RUN_EVENTS_ENABLED, INFERENCE_LOGPROBS_ENABLED and the Go background jobs (MCP DNS revalidator, cron, watch and process sweepers) cannot be switched off from repo deploy files.
6. **Snapshot drift (dated)**: `docs/compliance/evidence/posture-snapshot-2026-10-01.txt:5, 99, 125` shows `AGENTIC_SUBSCRIPTION_ENABLED set true` in inference-core, model-gateway and execution-core, while compose defaults it to false. Snapshot only; not current state.
7. **Quarry AUTOPROMPT** (Ingestion, cross-plane): W `apps/Ingestion Plane/docker-compose.yml:130` is `${QUARRY_EDGE_AUTOPROMPT:-1}`. L `:129` is `:-0`, with six more switches written out at L:131-136 (see section 9).

## 2. SSE feature families (request-scoped opt-in, not deploy flags)

- **Server gating**: `MP/rust/services/model-gateway/src/sse_events.rs`. `family()` maps each event to a family string or `None` (`W:351-382`, `L:410-444`). `should_emit(features)` returns true for `None` (control events, always emitted) and otherwise requires the family in the client's `features[]` (`W:385-392`, `L:446` onward).
- **Families**: `reasoning`, `steps`, `tools`, `citations`, `artifacts`, `usage`, `memory`.
- **Control events (always emitted)**, W: Title, FollowUps, PlanProposed, QueuedInput, Stopped, Error, SubscriptionError (`W:369-380`). L adds RetrievalNotice and ZdrError (`L:437-441`).
- **Plain chat** (empty features): only control events pass. Test at `W:1364-1393`.
- **Client default**: `FE/src/shared/api/chat-client.ts:876-883` [L:893-900] = `usage, citations, reasoning, steps, artifacts, memory`. Feature set built at `W:1022-1056` [L:1052-1085] with `conversation_only`, `tools`, and `agentic` toggled per request.
- **Pattern**: a new rich event gets a new family string in `family()` and, if default-on for the composer, joins `DEFAULT_FEATURES`. A new control event returns `None`. Test by asserting `should_emit` both ways (`sse_events.rs` tests near `W:1364-1438`).

## 3. Do mode (OD2): how it is gated today

"Product route" is read as the user-facing composer on the ChatGPT-subscription route. This interpretation is not checked against a doc term.

| Layer | Gate | Cite (W, then L if different) |
|---|---|---|
| Compose default | `AGENTIC_SUBSCRIPTION_ENABLED:-false` (model-gateway, inference-core, execution-core); `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS:-false` (model-gateway, execution-core) | `MP/deploy/docker-compose.yml:378-379, 847, 1038-1039` |
| Switch parse | OFF unless explicit on-word; `Switches::from_env` reads both env names | `MP/rust/crates/mp-contracts/src/agentic_subscription.rs:40-47, 94-97` |
| Enforcement | execution-core refuses a subscription run when the switch is off, with code `agentic_subscription_disabled`; requires `provider_hint == openai-codex-subscription` | `MP/rust/services/execution-core/src/agent_route.rs:113-131` (refusal at `:120-124`, provider check `:126-130`); entry `MP/rust/services/execution-core/src/grpc.rs:745` [L:761], called at `grpc.rs:1242` |
| Capability advertised | inference-core adds `agentic` to subscription models only when the switch is on (switch passed from `list_models`) | `MP/rust/services/inference-core/src/provider/codex_subscription.rs:870-878` |
| Gateway edge | model-gateway derives `agentic` from features; refusal uses the same code | `MP/rust/services/model-gateway/src/normalize.rs:296` [L:304], `:329` |
| Composer | Do offered on every non-subscription route; on the subscription route only when the catalogue advertises `agentic` | `FE/src/features/dashboard/home/DashboardComposer.tsx:1128-1141` [L:1135-1147] |
| Request body | `agentic` sent only when plan mode is on and the turn is not conversation-only and not support-read-only | `FE/src/shared/api/chat-client.ts:1044-1056` [L:1081-1085] |
| UI error | `agentic_subscription_disabled` shown in words | `FE/src/features/chat/components/chat-normalizers.ts:444` [L:483] |
| Tests | e2e and unit | `FE/tests/e2e/agentic-subscription.spec.ts`; `FE/src/features/dashboard/home/DashboardComposer.subscription.test.tsx`; `FE/src/features/chat/components/use-chat-controller.agentic-subscription.test.ts` |
| Decision state | OD2 open. Recommended: "Code and compose default OFF; ON only in the proof stack for A6. Read-only for the first proof... Flip the compose default after the dated live proof and your acceptance." | `FE/apps/verevon-web/plans/system-audits/chat-gap-closure-plan-2026-10-01.json:347-352`; `.../CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:422` |
| Observed | Snapshot shows switch ON in three containers and ALLOW_EFFECTS off at 2026-10-01 | `docs/compliance/evidence/posture-snapshot-2026-10-01.txt:5, 99, 125` (ON), `:98, 124` (ALLOW_EFFECTS false) |
| Effects | Terra Do runs had effects off; honest limit L15 | `docs/compliance/evidence/honest-limits.md:110` (W; file M-live); `FE/apps/verevon-web/plans/system-audits/A6_DO_MODE_TERRA_PROOF_2026-10-01.md:51` |
| Ledger | Default off recorded | `MP/docs/decisions/ledger.md:297` |

Ordering of enforcement: the composer hides Do and the client only sends `agentic` in plan mode, both as convenience. The authoritative check is execution-core (`agent_route.rs:120-124`), so a stale catalogue or a hand-written request is still refused when the switch is off.

## 4. Frontend Plane flags

### 4.1 SPA build-time flags (`import.meta.env`)

No `src/shared/flags` module exists (glob found none). Flags are read inline. Typed list: `FE/src/vite-env.d.ts:4-7` [L:4-9].

| Name | Read at | Default and declaration | Notes |
|---|---|---|---|
| VITE_ALLOW_DEV_AUTH_BYPASS (`'true'`) | `FE/src/shared/api/http.ts:40` [L:46]; `FE/src/shared/api/sse.ts:20` | `FE/.env.example:39` = false; `vite-env.d.ts:6` | Pairs with gateway `ALLOW_DEV_AUTH_BYPASS` (section 4.3). |
| VITE_ALLOW_DEV_ACTOR_HEADERS (`'true'`) | `FE/src/features/onboarding/lib/api/actor.ts:3` | `DEV` OR the flag; `vite-env.d.ts:5` | Dev actor headers. |
| VITE_VEREVON_GATEWAY_URL | `FE/src/shared/api/config.ts:7` | `FE/.env.example:34` empty; `vite-env.d.ts:4` | Base override. |
| VITE_TURNSTILE_SITE_KEY | `FE/src/features/auth/components/AuthPage.tsx:111` | `FE/.env.example:46-47` (comment: auth-core needs `CAPTCHA_ENABLED=true`); `vite-env.d.ts:7` | |
| VEREVON_VITE_MCP_ENABLED (Node env, not `VITE_`) | `FE/vite.config.ts:18` (comment `:13-17`) | opt-in, default off | Earlier guess "VITE_MCP_ENABLED" was wrong; the grep matched a substring. |
| VITE_SCENE_MODE (`'1'`) [live-only] | `FE/src/index.tsx:13` [L], `FE/src/app/App.tsx:41` [L] | declared `vite-env.d.ts:9` [L]; `FE/src/demo/` untracked [live-only] | Compile-out guard: "a normal build contains none of it" (`App.tsx:40` [L]). |
| VITE_DEMO (`'1'`) [live-only] | `FE/src/features/dashboard/home/` [L]: `DashboardUpdatesCard.tsx:43`, `browser-omnibox.ts:13`, `dashboard-search-utils.ts:22`; `FE/src/shared/api/home-updates-client.ts:21` [L] | not declared in `vite-env.d.ts` | Runtime demo branch, not compiled out. |
| `import.meta.env.DEV` | `FE/src/features/core/components/CoreNavbar.tsx:221`, `CoreNavbarMore.tsx:41` (same folder); `FE/src/features/chat/lib/chat-appearance-preview.ts:26,31`; `FE/src/features/onboarding/lib/api/actor.ts:3` | Vite dev mode | Dev-only UI. |

### 4.2 verevon-web (Next.js) gating

- No `NEXT_PUBLIC_*` flags found in `FE/apps/verevon-web` (grep, excluding plans).
- Dev routes gate on `process.env.NODE_ENV`: `FE/apps/verevon-web/src/app/dev/product-demo/page.tsx:7` (`notFound()` unless development); `.../dev/module-photos/page.tsx:11`.
- Live tree deletes `FE/apps/verevon-web/src/app/dev/product-demo/page.tsx` and related product-demo files (status lines 44-49 " D") [live-only change].

### 4.3 Gateway Rust flags (`FE/apps/gateway/src/config.rs`)

| Flag | Lines | Behaviour |
|---|---|---|
| CONTROL_PLANE_ENFORCEMENT | `:160-164` | Enum `off/permissive/strict`; default `off`; unknown gives `off`. `FE/.env.example:38` = strict; `FE/docker-compose.production.yml:13` = strict. |
| ALLOW_DEV_AUTH_BYPASS, ALLOW_DEV_ACTOR_HEADERS | `:262-263` via `dev_only_flag` `:419-427` | Refused (logged) when `APP_ENV` is production or prod (`dev_flags_allowed` `:429-431`). `FE/.env.example:40` = `0`; `FE/docker-compose.production.yml:14` = `"0"`. |
| ALLOW_INSECURE_DEV_DEFAULTS | `:358`, `:373` via `env_bool` `:433-442` | Default false. Debug builds boot with the placeholder key (`:373-374`). `FE/docker-compose.production.yml:15` = `"0"`. |
| APP_ENV | `:169`, `:421` | Gates dev flags and whether the public origin may be plain http (`:462-466`). `FE/docker-compose.production.yml:12` = `production`. |

Pattern: `dev_only_flag` is the one place that makes a bypass refuse in production. A new bypass should use it, not a bare `env::var`.

### 4.4 Route and capability gates

- `FE/src/app/App.tsx:106` (W; `:113` in L) `RequireWorkspaceAdmin`: unauthenticated to `/login`, onboarding incomplete to `/onboarding`, then workspace-admin check. Wraps `/settings/:section` (W `:223-227`; L `:233-237`).
- Router policy admin page: `/settings/router-policy` (`FE/src/features/core/lib/sidebar-navigation.ts:380`; `FE/src/features/core/lib/shell-data.ts:51,252`); component `FE/src/features/router-policy/components/RouterPolicyPage.tsx:54`; client `FE/src/shared/api/router-policy-client.ts:81,103` (`GET/PUT /api/v1/router-policy`), wire shape 1:1 with inference-core `RoutingPolicy` (`router-policy-client.ts:3-7`). Tuning surface, not an on/off flag. Backend route not traced.

## 5. Control Plane settings and entitlements used as flags

### 5.1 user-core per-user settings (typed settings with defaults in code)

- Type `AISettings`: `aiEnabled`, `modelPreference`, `dataCollectionEnabled`, `personalizationEnabled`, `memoryEnabled`: `CP/user-core/internal/http/handlers.go:1405-1411`.
- Defaults in the handler, merged by `GetSettings(ctx, userID, "ai", defaults)`: `handlers.go:1419-1422` (`memoryEnabled` false; the other three true).
- Routes `CP/user-core/internal/http/server.go:270-273` (`GET`/`PUT /api/v1/settings/ai`). Gateway forwards `FE/apps/gateway/src/domains/actions/dispatchers.rs:666`.
- No reader of `memoryEnabled`, `personalizationEnabled` or `dataCollectionEnabled` found anywhere in the worktree (grep: only `handlers.go`). Stored-only in this repo. Not determined whether another repo reads them.

### 5.2 org-core entitlements and the ZDR posture

- Stored entitlement row: `CP/org-core/internal/org/types.go:28-32` (`Entitlement{Key, Enabled, UpdatedAt}`); organisation carries them at `types.go:259`.
- Read API: `CP/org-core/internal/http/handlers.go:377-390`; routes `server.go:95` (`/organizations/:id/entitlements`) and `server.go:113` (`/orgs/:id/entitlements`). Machine principals: `service_auth.go:290, 317`.
- Computed, never stored: `InteractiveZdrEntitlementKey = "feature.zero_data_retention"` (`types.go:276`), appended fresh at `CP/org-core/internal/org/service_enhanced.go:168-176` as `PlanAllowsZeroDataRetention(plan)`. Plan allowlist `types.go:281-284` = `pro`, `enterprise`. Predicate `service_enhanced.go:572-573`.
- Token-issuer view: `types.go:301-304` (`RetentionPosture{ZDRIntent, Entitled}`) and `types.go:314-316` (`Effective() = ZDRIntent && Entitled`; comment: lapsed plan "is not zero-retention"). Only test callers found (`retention_posture_test.go:42-43`).
- Auth Core computes the same: `CP/auth-core/src/auth/interactive-retention-policy.ts:212-213` (`isZdr = zdrIntent && entitled`), lapsed to `zdr=false` with a log at `:475-478`.
- **Divergence**: the contract matrix states the opposite applied default. `MP/api/contracts/zdr-posture-matrix.json:179` ("Plan lapsed... the org stays ZDR until an admin turns it off") and `:183` ("OD-LAPSE applied default: effective ZDR is the stored intent... Auth Core today computes intent AND entitled and must change"). Code in W still matches the "intent AND entitled" reading. Owner ruling not found in repo.

### 5.3 Org-level ZDR switch gate

- `MP/docs/decisions/ledger.md:345-355` (2026-10-03): the admin's org ZDR switch may be turned on only where a provider is attested. Plan entitlement alone is not enough.

## 6. Central registry, docs and contract tests

No single registry file. The effective registry is spread across these:

| Artifact | Role | Cite |
|---|---|---|
| Compose defaults | De facto registry, the only place most Model Plane defaults are written | `MP/deploy/docker-compose.yml` (ranges in the header) |
| Operator template | Partial | `MP/deploy/.env.example` |
| Decisions ledger | Settled defaults and rejected approaches | `MP/docs/decisions/ledger.md:272-273` (ZDR_WAIVER_ENABLED default off with boot check), `:297` (AGENTIC default off), `:404-409` (CONTEXTUAL_RETRIEVAL kill switch, on by default, needs `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON`), `:1114` (QB-T17 defaults off) |
| Posture allow-list | Names whose values may be printed; everything else is withheld | `docs/compliance/evidence/tools/export-provider-posture.sh:1-3`, `:179-183` (exact-name list `:180`; EXTENSION list `:183` includes DREAMING_*, MEMORY_GROUNDING_ENABLED, AGENTIC_*, VEREVON_INTENT_ENABLED); test `.../export-provider-posture.test.mjs`; dated snapshot `docs/compliance/evidence/posture-snapshot-2026-10-01.txt` |
| Signed key partitions | Each runtime key is public, secret or artifact. `*_RELEASE_IMAGE` is refused. A name containing KEY, TOKEN, SECRET, PASSWORD or CREDENTIAL is refused unless audited | `MP/scripts/release-artifact.sh:732-745` (`config_partition_for_key`), `:747-753`, `:755-772` (`validate_config_policy`) |
| ZDR contract goldens | Authored spec and schema test | `MP/api/contracts/zdr-posture-matrix.json`, `MP/api/contracts/connection-zdr-capabilities.json`, `MP/api/contracts/README.md`; root `scripts/tests/zdr-contract-schema.mjs`, `scripts/tests/zdr-contract-sync.test.sh` [M-live] |
| Compose contract test | Asserts 8 LETTA variables appear in the capability-core block; the script also references `deploy/.env.example` (rest not read) | `MP/scripts/tests/letta-tool-search-compose-contract-test.sh:1-22` |
| Go switch test | Asserts default-on parse | `MP/go/services/capability-core/cmd/learning_review_switch_test.go` (exists; not read) |
| Ingestion default-off test | Requires each Quarry switch to be written out with default 0, and AUTOPROMPT not to default to 1 | root `scripts/tests/quarry-model-plane-switches.test.mjs` [live-only, untracked] |
| Frontend typed env | Four VITE names | `FE/src/vite-env.d.ts:4-7` [L:4-9] |
| Config tests (not reviewed) | Exist; content not read | `MP/scripts/tests/scheduled-step-config-contract-test.sh`, `tickets-create-config-contract-test.sh`, `release-contract-test.sh`; `MP/go/services/bridge-core/internal/config/config_test.go` |

## 7. Canary and staged-rollout conventions

- **No percentage, cohort or traffic-split flag mechanism found in code.** Grep for `canary|rollout_pct|rollout_percent|traffic split|cohort|ramp` over `MP/rust/services` gives 32 hits. The one hit sampled (`codex_subscription.rs:1436-1522`) is a test sentinel string; other hits not inspected. Go not exhaustively checked.
- **Staged path actually used** (verifiable in cites): default OFF in compose (LEARNING_REVIEW `compose:1426-1429`; MEMORY_GROUNDING `:769-774`; DREAMING `:756-767`) with a comment saying what it costs; dev override turns on expensive or dev-only paths (`override.yml:14`); proof-stack ON is owner-gated (OD2, section 3); compose default flip only after a dated live proof (OD2 text).
- **Release gating for security-relevant keys**: `MP/scripts/compose.sh:109-116` (production must use release-artifact); `release-artifact.sh:137-141` (runtime env files, `MODEL_PLANE_RUNTIME_ENV_FILE` etc.), `:139` (`MODEL_PLANE_COMPATIBILITY_GATES_FILE`), `:835-866` (ZDR true needs passed compatibility gates, `ZDR_RETENTION_PATH=zdr-provider-route-attested`, and signed routing keys), `:842-847` (`STATUS=passed` needs auth identity evidence), `:1483` and `:1531` (release mode requires gates before any build), `:1241-1245` (rollback runtime env file). Subcommands `:1855-1901` (deploy, verify, compose, validate-lock).
- **Evidence records**: `MP/scripts/eval-evidence.sh:1-25` (validate, summarize, compare; two lanes `model-only` and `integrated-harness`; "does not certify a provider, benchmark, signer, or deployment"). Test: `MP/scripts/tests/eval-evidence-contract-test.sh`.
- **Kill switches with reasons**: `MP/docs/decisions/ledger.md:404-410` (incident-only switch; off requires a written reason, logged and reported by `/readyz`; day-to-day control per organisation with audit). `:272-273` (ZDR waiver switch, default off, boot-time check).
- **Documented, not implemented**: `MP/docs/CUTOVER.md:89-104` describes `mp.cutover.<namespace>` with shadow, canary (org `triodelab`), ramp, canary_global, promoted, decommission, and per-org canary flags. Grep over `MP/` finds no reader of `mp.cutover` (only doc hits). Treat as historical plan.
- **Data-driven promotion (not feature flags)**: `MP/go/services/orchestrator-core/internal/quality/quality.go:1-67` promotes or quarantines skills by Wilson lower bound with hysteresis (`PromoteBound`, `DemoteBound`, `RecoverBound`) and a bounded sweep (`MaxQuarantinesPerSweep`).
- **No staging compose** in `MP/` (glob `**/*{eval-evidence,canary,staging,rollout}*` returned only the eval-evidence script and its test).

## 8. Recommended pattern to copy for a new flag, per plane

Based on majority convention in each plane. Cites are the exemplars to copy.

### 8.1 Model Plane Rust (services)

1. Constant in the owning module: `pub const FOO_ENABLED_ENV: &str = "FOO_ENABLED";` (`research_fan_out.rs:42`).
2. Pure parser `fn parse_enabled(raw: Option<&str>) -> bool` plus a unit test that does not touch process env (`post_mutation.rs:82-88`; `learning_events.rs:65-70`).
3. Default: OFF for new risky behaviour, using vocabulary C (`1/true/on/yes/enabled`, `post_mutation.rs:82-88`). ON only for an existing shipped kill switch, using vocabulary A (`research_fan_out.rs:67-72`). Pick one of the two; do not invent a third.
4. Read the env at the point of use (`post_mutation.rs:56-57`) or once in a `OnceLock` (`prompt_context.rs:119-129`) when the value must not change mid-process. Either way an operator change needs a restart; say so in the doc comment.
5. Deploy: add `FOO_ENABLED: ${FOO_ENABLED:-false}` to the owning service in `MP/deploy/docker-compose.yml` with a comment saying what it costs (majority compose default is `:-false`; see 1.3). Add a line to `MP/deploy/.env.example` only if an operator must tune it.
6. If the value is non-secret and should appear in posture snapshots, add it to the EXTENSION list in `docs/compliance/evidence/tools/export-provider-posture.sh:183`. Do not name it with KEY, TOKEN, SECRET, PASSWORD or CREDENTIAL unless it is audited (`release-artifact.sh:740-741`).
7. If it changes a contract or a default, record it in `MP/docs/decisions/ledger.md` (CLAUDE rule). Add a compose contract assertion modelled on `letta-tool-search-compose-contract-test.sh`.
8. If the client must see it, surface it as a model capability or an SSE family (sections 2 and 3), not as a new frontend VITE flag.

### 8.2 Model Plane Go (services)

- Read in `cmd/main.go` with a `parseXSwitch(raw) (enabled, recognized bool)` and an slog warn on unrecognised values (`main.go:749-775`).
- For a new default-off feature, prefer strict parsing that fails startup on a typo (`lettatools/client.go:243-254`). Reserve the exact-`"false"` opt-out (vocabulary H) for background sweepers only.
- Use the tri-state pattern only when the answer depends on a runtime dependency (`TASK_EXECUTOR_ENABLED`, `main.go:725-739`).

### 8.3 Ingestion Plane (Quarry, cross-plane default-off)

- Write every switch out in the quarry-edge block as `QUARRY_EDGE__X: "${QUARRY_EDGE_X:-0}"`. Default 0. Enforce with a test that reads the compose file (live-only `scripts/tests/quarry-model-plane-switches.test.mjs`). Note the HEAD mismatch (section 9).

### 8.4 Frontend SPA

- Server-controlled behaviour: read a capability from the catalogue or a settings GET, never a `VITE_` flag (`DashboardComposer.tsx:1128-1141` pattern).
- Dev or demo only: declare in `FE/src/vite-env.d.ts` (`:4-7`), read in one place as `import.meta.env.VITE_X === 'true'` (`http.ts:40/46` pattern), default `false` in `FE/.env.example`. For code that must not ship, compile-out guard with `=== '1'` (live `index.tsx:13`, `App.tsx:41`).

### 8.5 Frontend gateway (Rust)

- Add a typed field in `FE/apps/gateway/src/config.rs` (`:160-170` pattern). Use an enum whitelist with a safe fallback (`CONTROL_PLANE_ENFORCEMENT`, `:160-164`).
- Bypasses only through `dev_only_flag` (`:419-427`), so production refuses them.
- Pin the production value in `FE/docker-compose.production.yml` (`:12-15`) and document in `FE/.env.example` (`:38-40`).

### 8.6 Control Plane

- Per-user setting: add a field to the typed struct and a default in the `defaults` map (`handlers.go:1405-1422`), route under `server.go:270-273`. No env var.
- Per-org feature: either a stored `Entitlement{Key, Enabled}` row (`types.go:28-32`) or a plan-computed key like `feature.zero_data_retention` (`types.go:276`, `service_enhanced.go:168-176`). Name a single predicate that every reader calls (`PlanAllowsZeroDataRetention`, `service_enhanced.go:572`). Do not add a second reader that reimplements the rule (the `Effective` vs matrix divergence in 5.2 shows the risk).

## 9. Live-tree deltas that affect this digest

| Item | W | L | Note |
|---|---|---|---|
| Ingestion Quarry switches (QB-T17) | `apps/Ingestion Plane/docker-compose.yml:130` AUTOPROMPT `:-1`; no other switches written out | `:129-136`: all seven `:-0` | L is M (uncommitted). |
| `scripts/tests/quarry-model-plane-switches.test.mjs` | absent | present, untracked | Enforces default 0. |
| VITE_SCENE_MODE, VITE_DEMO, `FE/src/demo/` | absent | present; `src/demo/` untracked | Demo guards. |
| product-demo dev route | `FE/apps/verevon-web/src/app/dev/product-demo/page.tsx:7` NODE_ENV gate | deleted (status " D") | |
| Client default features and `agentic` | `chat-client.ts:876-883`, `:1022-1056` | `:893-900`, `:1052-1085` | Same set and logic. |
| Composer Do gating | `DashboardComposer.tsx:1128-1141` | `:1135-1147` | Same logic. |
| SSE families | `sse_events.rs:351-392` | `:410-446` | L adds RetrievalNotice and ZdrError as control events. |
| LEARNING_RUN_EVENTS_ENABLED const | `learning_events.rs:54` | `:55` | |
| agent_route refusal | `agent_route.rs:120-124` | `:147` | |
| enforce_agent_route | `grpc.rs:745` | `:761` | |
| FINETUNE feature_enabled | `finetune_routes.rs:315-317` | `:310` | |
| ALLOWED_MODELS loader | `normalize.rs:84-93`, `:366-367` | `:84-87` same; `:366-367` not re-checked | File M-live. The agentic check moved from W:296 to L:304, so later lines have likely shifted. |
| Settings route guard, App.tsx | `FE/src/app/App.tsx:223-227` | `:233-237` | Router policy is served under it. |
| `honest-limits.md:110` | W | M-live, not re-checked | |
| `MP/deploy/docker-compose.yml`, `.env.example`, `compose.sh`, `release-artifact.sh`, `ledger.md`, `export-provider-posture.sh`, posture snapshot, `zdr-posture-matrix.json`, org-core and user-core Go, auth-core retention policy | W | not in live M list | Same as W. |

## 10. Not determined

- Whether operators copy `MP/deploy/.env.example` to `deploy/.env` (decides whether DREAMING_LLM_EXTRACTION is on in practice).
- Whether any consumer outside this repo reads user-core `memoryEnabled`, `personalizationEnabled` or `dataCollectionEnabled`.
- The owner's ruling on OD-LAPSE (matrix versus code, section 5.2).
- Whether the 2026-10-01 AGENTIC=true state still holds. The snapshot is dated.
- Whether the gateway-to-inference-core path for `/api/v1/router-policy` is as assumed. Not traced.
- Go flags outside the cited services and the Go background jobs were not exhaustively enumerated.
- Live-tree contents of files marked M-live were spot-checked at cited lines only.
