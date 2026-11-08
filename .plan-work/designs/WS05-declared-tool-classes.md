# WS05-declared-tool-classes - Declared tool side-effect classes and MCP through the signed gate

Paths are relative to `apps/Model Plane/` unless shown in full. `rust/services/X` = `apps/Model Plane/rust/services/X`. `WT` = worktree at afb83928 (clean). `LIVE` = `C:/dev/CoresSystem` (same HEAD plus uncommitted work). Line numbers are WT unless marked.

## 1. Scope, items resolved, owner

**Item:** F-t3-003 (Priority, L, W1). **Owner plane:** Model Plane. **Services:** capability-core (Go, registry + policy engine, system of record), execution-core (Rust, gate + permission enforcement + MCP dispatch), model-gateway (Rust, MCP discovery + transport only), mp-contracts (shared Rust crate). Frontend Plane gets one BFF route set and action contracts. No Control, Data, Ingestion or Quarry change.

**IN**
- A closed side-effect-class vocabulary and a declared class on every capability-core row, held per tool binding because several tools share one capability row.
- MCP discovery annotations turned into a derived class, pinned by a content digest, with drift detection.
- `mcp__<server>__<tool>` resolved to a capability id by the registry, never by string synthesis.
- The permission engine, autonomy rung, leaf blocklist, plan-mode refusal and retry gate all read the declared class. `is_risky_call` / `is_risky_tool` substring guessing is deleted at the end.
- A registry-owned never-auto set (class `owner_effect` plus an explicit approval floor).
- Per-tool allow/ask/never policy at user and org scope, stored in capability-core, merged stricter-wins in the engine.
- MCP tools callable in Do runs (execution-core `RunAgent`) through the existing signed gate. ZDR refuses them.

**OUT (explicit)**
- Ask-mode (inline chat loop) MCP. `inline_tool_allowed` (`rust/services/model-gateway/src/tool_loop.rs:615-620`) keeps refusing `mcp__*`, `mcp_call`, `mcp_catalog`. Needs an inline continuation contract; that would push the item to XL.
- Durable resume of an approved MCP call (WS03/WS13). `approval_delivery_worker.rs:16-27` resumes only `execute_provider_action`, `book_shipment`, `tickets.create`.
- Settings UI for policies (WS25 org registry UI, WS23 connected-apps UI). WS05 ships API, BFF route and action contract only.
- Space-scoped policy, per-site browser policy (WS21), staged `mcp_catalog`/`mcp_call` inside Do runs (F-t3-070), stdio MCP transport.

| Slice | Tag | One line |
|---|---|---|
| S0 | [FINISH] | Characterisation golden of today's behaviour, doc drift fixes, ledger entries |
| S1 | [BUILD] | Class vocabulary (`mp-contracts` + Go), registry columns/table, seed, proto read fields |
| S2 | [BUILD] | MCP observation, derivation, digest pin, drift quarantine (gateway to capability-core) |
| S3 | [BUILD] | Signed decision v2 (class, tool, user), `ResolveToolBindings`, execution-core registry binding |
| S4 | [FINISH] | Permission engine reads the class; MCP offered, gated, executed, ZDR-refused in Do runs |
| S5 | [BUILD] | Per-tool allow/ask/never store, engine merge, gateway + BFF + action contract |
| S6 | [FINISH] | Flip to enforce, delete heuristics, docs |

## 2. Verified current state

### 2.1 What exists (opened)
| Fact | Evidence |
|---|---|
| Risk is guessed by name substring; every `mcp__` name is risky | `rust/services/execution-core/src/permission/mod.rs:229-285` (arm 280-282); `is_risky_call` 195-200 adds one argument-aware case, `execute_provider_action` via `operation_is_write` (206-218; `integration_tools.rs:747-761`, unknown counts as write) |
| Rung comes from the same classifier, not from `risk_level` | `permission/mod.rs:100-108` (`rung_required_for`), 124-143 (`check_autonomy_rung`, `Unspecified` = unconstrained). `risk_level` appears nowhere in execution-core's permission code |
| Never-auto set is two predicates, not one | `requires_durable_owner_approval` 77-79 (`tickets.create`) and `requires_consent_to_disclose` 160-162 (`read_subagent_result`); both gate even `auto` (53-59) |
| Six call sites consume the heuristic | `permission/mod.rs:43-70, 100-108`; `runtime_loop/agent.rs:1639-1661` (leaf), 1685-1706 (rung), 1800-1822 (plan mode); `runtime_loop/retry.rs:150` |
| Dynamic MCP names have no binding, so every call is refused before the permission gate | `capability_policy.rs:836-838` (`_ => return None`), 361-362 (`permission_denied`), test 965; `runtime_loop/mod.rs:488-539` (gate), 535 (wording), 557 (permission gate), 747-748 (arm), 1069-1093 (`execute_mcp`, no `zdr` parameter) |
| The signed decision binds tuple (capability id, version, org, run, agent, scope, decision), not the tool, class or user | `go/services/capability-core/internal/server/decision_proof.go:31-44`; Rust verifier `capability_policy.rs:461-537` (capability id recomputed from the name at 518) |
| `EvaluatePolicyRequest` has no tool name and no user id; scope is `global`, agent `execution-core` | `proto/model_plane/v1/capabilities.proto` (fields 1-5); `capability_policy.rs:367-373`. `EvaluatePolicyResponse` fields 1-6; `CapabilityDetail` 1-17 (comment at 78 lists a risk vocabulary the Go model does not use: `capability.go:24-29`) |
| Engine decides from `risk_level` only: high=ask, medium/low=allow with budgets, unknown=deny | `go/services/capability-core/internal/policy/engine.go:97-154`; `server.go:457-494`, `capabilityPolicyBlockReason` 539-564 |
| Every row needs a health attestation fresh within 5 minutes or the call is denied `runtime_unavailable` | `internal/models/availability.go:35-41`, `DeriveAvailabilityAt`; `Upsert` resets attestation on any change (`internal/registry/capabilities_store.go:158-186`) |
| MCP servers live in `mcp_servers` (+ `config_json` with exact `tool_allowlist`); no tool-level rows, no class, no policy | `migrations/0003_capabilities_registry.up.sql:188-213`; `internal/api/registry_apis.go:471-476` (strict config), 1274-1284 (upsert), 925-976 (routes). Only static MCP row: `cap.mcp.filesystem` (`internal/registry/registry.go`) |
| Discovery, annotation parse and `McpSideEffect` exist in the gateway with no production consumer | `rust/services/model-gateway/src/mcp_jsonrpc.rs:64-97, 108-124, 223-261`; `runtime_registries.rs:497-558, 743-866`. Wire drops annotations: `ToolDefinition` is name/description/parameters only (`proto/model_plane/v1/inference.proto:178-185`), reused by `ListMcpToolsResponse` (`gateway.proto`, tools = 2) |
| Per-server allowlist is not a human approval: the connect flow auto-allowlists every tool the server reports | `rust/services/model-gateway/src/http_routes.rs` `resolve_tool_allowlist` (2786) |
| Gateway ZDR guard on MCP execution exists on caller identity; run `zdr` never reaches `execute_mcp` | `model-gateway/src/grpc.rs` `proxy_mcp_tool` (WT 1778) |
| MCP offering to Do runs depends on gateway memory: `ListMcpTools` passes no bearer, so it never hydrates from capability-core | `model-gateway/src/grpc.rs` `list_mcp_tools` (WT ~1812-1835, `capability_bearer: None`); `runtime_registries.rs:350-410` (hydration needs a bearer), 830-866. Warm only after an inline chat turn or HTTP list on that replica |
| Above 40 tools the gateway returns synthetic `mcp_catalog`/`mcp_call`, which execution-core cannot execute or bind | `runtime_registries.rs:566-677`; `agent.rs:652-656` |
| Admin is read from signed token scopes | `model-gateway/src/ownership.rs:284-289`, `http_routes.rs:2700-2703`. Gateway ZDR mutation guard: `require_non_zdr_durable_mutation` (defined 823, used in `mcp_register`) |
| capability-core principal for execution-core has `capability:read`; model-gateway has no capability-core audience | `apps/Control Plane/config/plane-service-principals.json:134-181` and `289-316` |
| `AuthorizeHTTP` refuses every durable mutation from a ZDR principal | `internal/authz/authz.go` (`AuthorizeHTTP`, non-read branch) |
| Existing test patterns to extend | `permission/mod.rs` tests 292-701; `tests/autonomy_ladder_contract.rs` (source-text pins, incl. `rung_required_for` must contain `is_risky_call(`); `agent.rs:3805` `every_offered_tool_has_a_capability_binding`, next test `every_offered_tool_capability_has_an_attestor`; `tests/cross_service_loop_contract.rs` (cross-service source read) |

### 2.2 [live-only] and land-first
| File | State | What must land first |
|---|---|---|
| `rust/crates/mp-contracts/src/zdr_refusal.rs` (feature `mcp_execution`, LIVE 292-295) and `api/contracts/zdr-refusals.json` (LIVE 153) | untracked | Commit before S4's typed ZDR refusal. Until then S4 uses the existing gateway refusal (WT `grpc.rs:1778`) as the only backstop and refuses locally with `failed_precondition` |
| `model-gateway/src/grpc.rs` (M, 47 lines; LIVE `proxy_mcp_tool` 1782, key renamed 1787) | modified | S4 touches `list_mcp_tools` and `proxy_mcp_tool` in the same file: rebase on the committed ZDR-helper refactor |
| `execution-core/src/runtime_loop/mod.rs` (M, +18: `waived: bool` through `execute_step*`, gates at LIVE 501-552, MCP arm 761) and `runtime_loop/agent.rs` (M, 207) | modified | WS03/WS02 own these signature changes; S3/S4 edit after they land (section 5) |
| `rust/services/model-gateway/src/tool_loop.rs` (M, MCP refs shifted to LIVE 625-644, 2515-2518, 3686-3689) | modified | Comment-only edit in S6 |
Gate bodies in `permission/mod.rs`, `capability_policy.rs`, `mcp_gateway.rs`, `mcp_jsonrpc.rs`, `runtime_registries.rs`, `retry.rs`, all of capability-core, `capabilities.proto`, `gateway.proto` are clean in LIVE.

### 2.3 Where the audit is stale or wrong
1. Card cites match LIVE, not WT, for `tool_loop.rs` and `runtime_loop/mod.rs` (shift 13-24 lines). Logic identical.
2. Card says reuse `risk_level` in `permission/mod.rs:100-200`. It is absent there; the rung derives from `is_risky_call`. `risk_level` is read only by capability-core, for registered rows, so it never applied to MCP.
3. Card says never-auto is only `tickets.create`; there is a second predicate (`read_subagent_result`).
4. Card says "MCP tools are offered to Do runs". Offering depends on a warm gateway cache and breaks above 40 tools (synthetic names). Neither is noted in the audit.
5. The allowlist is not approval (auto-allowlist at connect), so annotation trust cannot rest on it.
6. Approved MCP calls cannot be cold-resumed (worker allowlist), so "ask" is a dead end for MCP writes today.
7. A per-row 5-minute attestation would make per-tool MCP rows unusable; not mentioned.
8. `docs/capability-ownership-matrix.md:142` says the stdio MCP transport is wired; code quarantines everything but `http` (`runtime_registries.rs:909-913, 1788-1795`). Sweep question "do MCP tool-level rows exist" is answered: no.
9. `mcpIdentifier` allows `__` in server ids (`registry_apis.go:376`); `parse_mcp_tool_name` splits on the first `__` (`mcp_gateway.rs:108-111`), so a registry lookup, not parsing, must decide the binding.

## 3. Decisions and open questions
No ledger entry is reversed. `ledger.md:1553` references a rejected "service token for MCP" fix (original text not located: not determined). D5 is scoped so it is not that fix. Add ledger entries in S0.

| # | Decision | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | Where the class lives | (a) column on `capabilities` only; (b) per-tool table only; (c) both | (c): `capabilities.side_effect_class` = ceiling (strictest tool), `capability_tools` = per-tool truth. Needed because `cap.browser.open` covers `browser_agent`, `browser.act`, `browser.observe`; `cap.tool.http`, `cap.process.background` cover several tools | S1. Internal; proceed |
| D2 | MCP annotation trust | (a) never trust a read-only claim; (b) always trust (Hermes default); (c) per-server flag, default untrusted | (c). `annotation_trust` in `mcp_servers.config_json`, set by the server registrant; org-scope needs admin. Untrusted `readOnlyHint:true` derives `external_write` | S2 derivation default. Owner call; S1/S3 proceed. S2 ships (c) default untrusted meanwhile |
| D3 | MCP writes in Do runs before resumable approval | (a) offer and ask (dead-end: worker returns `invalid_continuation`); (b) withhold non-read MCP tools; (c) build MCP resume now | (b): flag `MCP_DO_TOOLS=read` until WS03/WS13 prove resume for MCP. (c) is out of scope (unknown idempotency, `approval_delivery_worker.rs:16-27`) | S4 `all` mode only. Read-class MCP proceeds without a decision |
| D4 | Cold gateway cache | (a) exec-core forwards its org-bound `capability:read` token in gRPC metadata so the gateway can hydrate; (b) add a capability-core audience for `model-gateway` in Control's principals file; (c) accept warm-cache dependency | (a) now, (b) later (Control owns the file). WS04 may supersede | S4 e2e reliability. S1-S3, S5 proceed |
| D5 | Auth of gateway to capability-core observation write | (a) existing org-derived HMAC internal route (`X-Mcp-Service-Token`, `registry_apis.go:499-543, 973-976`), limited to observation upsert; (b) user bearer (read-only users cannot write); (c) Control principal | (a), restricted: may create/refresh observations only; cannot approve drift, lower a class, or touch policy. Document the boundary in a new ledger entry | S2. Needs owner/security nod |
| D6 | May users relax approvals | (a) tighten-only everywhere; (b) relax built-ins too; (c) relax MCP rows only, bounded | (c) for v1: `allow` honoured only for `kind=mcp_tool`, ceiling `external_write` for user, `destructive` for org admin, never `owner_effect`/`unclassified`-by-user, never over a stricter org policy; built-ins are tighten-only | S5. Owner call; tighten path ships without it |
| D7 | Asserted `user_id` on EvaluatePolicy | assert from execution-core service principal (as `ListMcpTools` does) vs verify against session-core | Assert; EvaluatePolicy is service-only (`authz.go` AuthorizeGRPC). Record the trust assumption | S3 |
| D8 | ZDR org policy writes | inherit blanket refusal vs carve out | Inherit (policy store read-only for ZDR principals; defaults apply). MCP is refused under ZDR anyway | S5 |
| D9 | `browser.observe` asks today (shares high-risk `cap.browser.open`) | keep parity vs split row | Keep parity in WS05 (class never loosens a row's decision); split row is WS21's | none |

## 4. Slices (PR-sized, in build order)

Common to all slices: preserve the file's existing line endings when editing (worktree files may be CRLF); Go migrations re-run at every boot, so every migration is idempotent (`IF NOT EXISTS`, `ON CONFLICT`; `docker-entrypoint.sh:4-7`); capability-core already has two `0013` files, do not add a third prefix; the integrator assigns numbers.

### S0 [FINISH] - Characterise, fix drift, record decisions
**Goal:** freeze today's behaviour before any change; correct stale docs.
**Files**
- `rust/services/execution-core/src/permission/mod.rs` - add test `legacy_classification_golden` enumerating `offered_tool_defs()` + `process_tool_defs()` names + `plan.propose`, `subagent.task`, `tickets.create`, `read_subagent_result`, `mcp__s__t` and asserting the tuple (is_risky_call with representative args, is_restricted_context_write, rung_required_for, requires_* predicates). This tuple becomes the S1/S4 parity oracle. (`offered_tool_defs` is private to `runtime_loop/agent.rs`; put the test in `agent.rs` tests next to 3805 or export a `#[cfg(test)]` helper.)
- `proto/model_plane/v1/capabilities.proto` - fix the `risk_level` comment (line 78) to `low | medium | high`. Comment-only; no `buf generate` needed beyond comments.
- `docs/capability-ownership-matrix.md` - correct line 142 (stdio not wired: http only) and note tool-level rows arrive in WS05.
- `docs/decisions/ledger.md` - append three entries (state `proposed`): declared classes replace name heuristics; class never loosens a seeded row; observation-write route boundary (D5); MCP writes withheld from Do runs until resumable approval (D3, `deferred`).
**Proto/migration:** none. **Flag:** none. **ZDR:** none (tests and docs).
**Tests:** `cd "apps/Model Plane/rust" && cargo test -q -p execution-core --lib permission` and the new golden.
**DoD:** golden passes on HEAD; docs corrected; ledger entries merged.
**Evidence:** test output only.
**Effort:** S, 1-2 d. **Deps:** none. **Risks:** the oracle must include argument variants for `execute_provider_action` (read and write operations).

### S1 [BUILD] - Class vocabulary, registry model, seed, read surface
**Goal:** every capability-core row and tool binding carries a declared class; nothing enforces it yet.
**Vocabulary** (closed; unknown parses as `unclassified`, never as a lower class)
`read`, `sandbox_compute`, `context_write`, `external_write`, `destructive`, `owner_effect`, `unclassified` (default, strictest). Total mapping, chosen to reproduce today's three observable tiers:
- risky (ask-posture gate, leaf blocklist, plan mode, no retry): `external_write`, `destructive`, `owner_effect`, `unclassified`
- restricted context write: `context_write` (needs rung `workspace_write`)
- rung: `read`/`sandbox_compute` -> `read_only`; `context_write` -> `workspace_write`; risky -> `danger_full_access`
- `approval_floor` in `none | ask | durable_owner`; invariant `owner_effect` implies `durable_owner` (CHECK + test). `read_subagent_result` = class `read`, floor `ask`.
**File-level changes**
- `rust/crates/mp-contracts/src/tool_class.rs` - NEW: `SideEffectClass`, `ApprovalFloor`, `from_wire` (unknown -> `Unclassified`), `is_risky()`, `is_restricted_context_write()`, `required_rung()` (uses `crate::autonomy`), `stricter()`, `derive_mcp_class(declared: DeclaredSideEffect, annotation_trust: bool)`. Pure, table-tested. `rust/crates/mp-contracts/src/lib.rs` - `pub mod tool_class;` (file is M in LIVE, +2 lines: rebase).
- `go/services/capability-core/internal/models/tool_class.go` - NEW: Go twin constants, `IsSupportedSideEffectClass`, `Stricter`, `DeriveMCPClass(declared string, trust bool)` with the same known-answer vectors as the Rust test (twin-test pattern used for the HMAC token, `registry_apis.go:519-523`). `models/capability.go` - add `SideEffectClass`, `ApprovalFloor` to `Capability` (103-133); `KindMCPTool = "mcp_tool"` (7-22).
- `go/services/capability-core/migrations/<next>_capability_tool_classes.up.sql` (+ `.down.sql`) - (1) `ALTER TABLE capabilities ADD COLUMN IF NOT EXISTS side_effect_class TEXT NOT NULL DEFAULT 'unclassified' CHECK (...)`, `approval_floor TEXT NOT NULL DEFAULT 'none' CHECK (...)`. (2) `CREATE TABLE IF NOT EXISTS capability_tools` PK `(org_id, capability_id, tool_name)`: `side_effect_class`, `approval_floor`, `call_refinement TEXT NULL` (closed set, one value `provider_operation`), `source` (`seed|derived|override`), MCP-only nullable columns `server_id`, `qualified_name UNIQUE per org`, `pinned_digest`, `observed_digest`, `description_sha256`, `schema_sha256`, `declared_side_effect`, `declared_annotations JSONB`, `review_state` (`approved|pending|drifted`), `observed_at`, `created_at`, `updated_at`. No description or schema text is stored. (3) Seed rows for org `global` from this table, `ON CONFLICT ... DO UPDATE ... WHERE capability_tools.source='seed'`:

| capability id | tools | class | floor |
|---|---|---|---|
| cap.command.shell | shell | destructive | none |
| cap.command.sandbox | code_interpreter | sandbox_compute | none |
| cap.process.background | process_start/read/stdin/signal/list | sandbox_compute | none |
| cap.browser.open | browser_agent, browser.act | external_write | none |
| cap.browser.open | browser.observe | read | none |
| cap.tool.http | web_search, web_fetch, web.search, web.read | read | none |
| cap.retrieval.query | knowledge_search | read | none |
| cap.tool.information.read | yr_weather, traffic, news, company_lookup | read | none |
| cap.tool.shipping.track / .read | track_shipment; get_shipping_quotes, shipping_carriers | read | none |
| cap.tool.shipping.book | book_shipment | external_write | none |
| cap.tool.social.read / .publish | list_social_accounts; publish_social_post | read; external_write | none |
| cap.tool.provider.read / .execute | list_provider_actions; execute_provider_action (`call_refinement=provider_operation`) | read; external_write | none |
| cap.tool.ticket.create | tickets.create | owner_effect | durable_owner |
| cap.memory.index | save_memory, plan.propose | context_write | none |
| cap.memory.search | recall_memory | read | none |
| cap.agent.lineage.read | list_subagent_results; read_subagent_result | read; read | none; ask |
| cap.skill.summarize | reattach_skill | read | none |
| cap.agent.spawn | subagent.* | read (control tool; loop exemptions by `is_subagent_tool` stay) | none |

Row ceiling backfill: strictest of its tools; rows with no tools: non-effect kinds (inference, routing_policy, safety_policy, memory_adapter, model, retrieval, skill, memory) -> `read`; kinds command/sandbox/plugin/browser/mcp_server -> `destructive` if `risk_level='high'` else `external_write`; anything else stays `unclassified`. A Go test enumerates every seeded id and fails on an unintended `unclassified`.
- `internal/registry/capabilities_store.go` - add the two columns to `CapabilityRow` (31-65), `Upsert` (103-192), and the SELECT/scan sites (245, 264, 283, 327, 640, `scanCapabilityRow` 692, `scanCapabilityRowFull` 710). Extend `enforceRiskFloor` (194-240): lowering a class's strictness requires `authz.RiskOverrideScope`; `side_effect_class` is never settable from the generic upsert for `kind=mcp_tool`. `internal/registry/capability_tools_store.go` - NEW: `Get/ListForOrg/Upsert(tool)` with the same floor.
- `internal/api/capabilities.go` (upsert at 147) - reject `kind=mcp_tool` from the generic API; pass class through.
- `internal/server/server.go` + `toDetail`/`rowToDetail` - map new fields to `CapabilityDetail`.
- `proto/model_plane/v1/capabilities.proto` - `CapabilityDetail`: `string side_effect_class = 18; string approval_floor = 19;` (next free verified: last is 17). Regenerate Go (`go/gen/model_plane/v1/capabilities*.pb.go`, committed) with a scoped `buf generate` and review the diff (the repo notes unscoped regen rewrites unrelated files, matrix §G8 note). `api/openapi.yaml` (risk_level at 1553) and `api/sdk/typescript/src/models/Capability.ts` (130, 159) - add fields.
- Rust parity: `rust/services/execution-core/tests/tool_class_registry_parity.rs` - NEW, source-text read of the migration (pattern of `cross_service_loop_contract.rs`): every arm of `trusted_capability_id` (`capability_policy.rs:741-841`) has a seeded `(capability_id, tool_name)` and the seeded class equals the S0 oracle's derived class; fails when a new offered tool has no row (extends the "offered -> bound -> seeded -> attested" chain with "-> classed").
**Flag:** none (additive columns, nothing reads them yet). Rollback: leave columns; `.down.sql` drops them.
**ZDR:** only tool metadata; no user content. Golden-matrix: none new.
**Tests:** `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`; `go test -tags integration ./internal/registry/...` (testcontainers Postgres; add `capability_tools_integration_test.go` and `class_floor_integration_test.go`); `cd "apps/Model Plane/rust" && cargo test -q -p mp-contracts --all-targets && cargo test -q -p execution-core --test tool_class_registry_parity`; `buf lint apps/Model Plane/proto` and `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (additive only).
**DoD:** every seeded row classed; floor test proves a plain `capability:write` caller cannot lower a class; parity test green; contract crate mapping reproduces the S0 oracle.
**Evidence:** rebuild capability-core image (`./scripts/compose.sh up -d` recipe), apply migrations on the running Postgres, `GET /api/v1/capabilities` shows classes, `capability-health-proof.sh` still green.
**Effort:** M, 4-6 d. **Deps:** S0; D1. **Risks:** seven SELECT/scan sites drift (mitigation: one shared column list const); seeded `unclassified` leaks.

### S2 [BUILD] - MCP observation, derivation, digest pin, drift
**Goal:** every discovered MCP tool gets a registry row with a derived, pinned class; a changed definition quarantines the tool.
**File-level changes**
- `rust/services/model-gateway/src/mcp_jsonrpc.rs` - add `McpToolDef::digest()` (SHA-256 over a recursively key-sorted canonical JSON of `{v:1,name,description,inputSchema,annotations(five hints, null when absent)}`; do not rely on `serde_json` map order, `preserve_order` can be enabled transitively) plus `description_sha256`, `schema_sha256`. Gives `side_effect()` (108-124) its first production consumer: emit `declared_side_effect` as `read_only|destructive|non_destructive_write|undeclared`. Rust implements the MCP spec rules once; Go only applies trust.
- `rust/services/model-gateway/src/runtime_registries.rs` - in `full_mcp_tool_defs` (743-823) after `filter_allowlist`, collect `McpToolMeta` for each exposed tool and, when the caller is non-ZDR and `MCP_TOOL_OBSERVATION_ENABLED`, POST an observation batch (best effort, bounded, never blocks the turn). New `capability_observation.rs` (NEW in model-gateway) builds the request with the org-derived header (`derive_org_scoped_service_token`, `mcp_oauth.rs:436` twin).
- `proto/model_plane/v1/gateway.proto` - `message McpToolMeta { string qualified_name=1; string server_id=2; string tool_name=3; string digest=4; string declared_side_effect=5; }`; `ListMcpToolsRequest`: `bool unstaged = 4;` (last is 3); `ListMcpToolsResponse`: `repeated McpToolMeta meta = 3;` (last is 2); `ProxyMcpToolRequest`: `string expected_digest = 7;` (last is 6). All additive. Regenerate Go gen (scoped).
- `rust/services/model-gateway/src/grpc.rs` - `list_mcp_tools` fills `meta`, honours `unstaged`; `handle_proxy_mcp_tool` (`runtime_registries.rs:1741-1838`) re-checks `expected_digest` against the cached discovery (error `tool_definition_changed`). The ZDR guard stays.
- `go/services/capability-core/internal/api/mcp_tools.go` - NEW handler `POST /api/v1/internal/mcp/tools/observe`, mounted in `RegisterInternal` (`registry_apis.go:973`), HMAC-gated by `authorizedInternalServiceCall`. Strict JSON, body-size cap. Per tool: upsert `capabilities` row (`kind=mcp_tool`, `name=<qualified_name>`, `org_id`=org, `enabled_for_scopes={global}`, `risk_level` derived: `read`/`context_write`/`sandbox_compute` -> low, else high, `version` bumped on digest change) and a `capability_tools` row. Rules: first sight = TOFU pin, `review_state=approved` if the server is registered and the name is allowlisted; digest change after pinning = `review_state=drifted`, row `enabled=false`, class unchanged until approval; never lowers a class; never touches policy. Appends `registry_audit_log` (`action=mcp_tool_observed|mcp_tool_drifted`).
- `internal/api/mcp_tools.go` also: `POST /api/v1/mcp/{id}/tools/{tool}/approve` (server owner for user-scope, admin for org-scope; requires `capability:write`; non-ZDR principal) re-pins the digest and re-derives; `GET /api/v1/mcp/{id}/tools` lists rows with class, state, class_source. Wire into `Register` (925-962).
- `internal/api/registry_apis.go` - `mcpServerConfig` (471-476) add `AnnotationTrust bool json:"annotation_trust,omitempty"`; `normalizeMCPRegistration` (570-638) reject `__` in new server ids (`mcpIdentifier` at 376 allows it) and require admin for `annotation_trust` on org scope; `rust/.../runtime_registries.rs` `mcp_capability_payload` (1085-1128) sends it (strict decode would otherwise reject).
- `internal/models/availability.go` - for `kind=mcp_tool` replace the 5-minute rule by an observation window (`MCP_TOOL_OBSERVATION_TTL`, default 1 h) and derive from the parent server `enabled`/`rollout_state` and `review_state`. `server.go` `capabilityPolicyBlockReason` (539-564) maps `drifted` to `tool_definition_changed`. Server disable/delete cascades `enabled=false` on its tool rows (`registry_apis.go` patch 1326, delete 1416).
**Flags:** `MCP_TOOL_OBSERVATION_ENABLED` (gateway, default on after S2 proves, vocabulary C; kill switch). **Rollback:** disable flag; rows persist unused.
**ZDR:** observation carries digests and hints only, never descriptions, schemas, arguments or outputs. Skipped when the triggering caller is ZDR (no durable write from a ZDR principal, `authz.go`); MCP is not offered under ZDR in S4 anyway. Golden-matrix: add `mcp_tool_observation_under_zdr` (no write, no row).
**Tests:** Go unit `mcp_tools_test.go`, `derive_mcp_class_test.go` (TOFU, drift, no-lowering, trust matrix, `__` rejection, HMAC wrong-org rejected); integration `mcp_tools_integration_test.go`; Rust `mcp_jsonrpc.rs` digest tests (canonicalisation stable under key order; description-only change changes digest; known-answer vectors shared with Go); `runtime_registries.rs` observation test with a mock capability-core (wiremock, already a dev-dependency in execution-core; use axum test server in gateway). Commands as S1 plus `cargo test -q -p model-gateway --all-targets`.
**DoD:** connect a fixture server, three rows appear with expected classes; mutate the fixture's description, next discovery marks the tool `drifted` and gateway proxy returns `tool_definition_changed`; approve re-pins.
**Evidence:** new fixture `scripts/tests/fixtures/mcp-annotated-server.mjs` (NEW; tools `echo_read` readOnlyHint true, `create_item` readOnly false destructive false, `drop_all` no annotations) run via the compose stack; capture `capability_tools` rows (psql), registry audit rows, the drift refusal.
**Effort:** L, 6-8 d. **Deps:** S1, D2, D5. **Risks:** gateway-supplied annotations are third-party claims (default untrusted; derive only tightens); gateway cache TTL (`MCP_CATALOG_TTL` 60 s, `runtime_registries.rs:56`) bounds drift detection latency; connect-flow auto-allowlist means TOFU pins at connect time, a human click.

### S3 [BUILD] - Signed decision v2 and registry-owned binding
**Goal:** the signed gate binds tool, class and user; `mcp__` names resolve through the registry.
**File-level changes**
- `proto/model_plane/v1/capabilities.proto` - `EvaluatePolicyRequest`: `string user_id = 6; string tool_name = 7;` (last is 5). `EvaluatePolicyResponse`: `string side_effect_class = 7; string approval_floor = 8; string policy_source = 9; string tool_digest = 10;` (last is 6). New `rpc ResolveToolBindings` on `CapabilityCore`: request `{ string org_id=1; string run_id=2; string user_id=3; repeated string tool_names=4; }`, response `{ repeated ToolBinding bindings=1; }`, `ToolBinding { tool_name=1; capability_id=2; capability_version=3; side_effect_class=4; approval_floor=5; tool_digest=6; review_state=7; effective_policy=8; }`. Scoped `buf generate`.
- `go/services/capability-core/internal/server/decision_proof.go` - claims gain `tool_name`, `user_id`, `side_effect_class`, `approval_floor`, `policy_source`, `tool_digest`; `Sign` writes `Version = "2"` (currently hard-coded `"1"`). Update `decisionProofClaimsFor` and tests in `decision_proof_test.go`.
- `internal/server/server.go` - `EvaluatePolicy` (457-494): when `tool_name` present, load the `capability_tools` binding and pass class/floor to the engine; absent `tool_name` uses the row ceiling and no policy (compat, strict). New `ResolveToolBindings` handler (verified principal; non-service callers may only ask for themselves, as `ListMcpTools` does). `internal/authz/authz.go` `AuthorizeGRPC`: add the method to the read-scope branch and pin it in `authz_test.go` (`TestAuthorizeGRPCPinsEvaluatePolicy...`).
- `internal/policy/engine.go` - new pure `Decide(cap, binding, policies, userID)`; in S3 it implements only: base risk switch unchanged (class never loosens it), floors (`owner_effect`/`ask`/`unclassified` -> ask), `review_state != approved` -> deny. `engine_class_test.go` - table tests.
- `rust/services/execution-core/src/capability_policy.rs` - replace the sync `trusted_capability_id` call at 361 and 518 with an async `Binding` (capability id, version, class, floor, digest). Built-ins keep the static map (rename `trusted_builtin_capability_id`, tests 940-984 stay). `mcp__*` resolves only from a per-run binding table filled by `resolve_tool_bindings` (one RPC at run start through the existing service token and channel, `capability:read`); a miss makes one bounded registry lookup, then `permission_denied` "tool has no governed capability binding". The verifier (461-537) checks `claims.tool_name`, class, floor, digest and `v` (accept `"1"` while flag is `off`; require `"2"` under `enforce`). `CapabilityEvaluation` (32-37) carries class, floor, policy_source. Per-process cache keyed `(run_id, tool_name)` pins `(capability_version, class)` for the run: a mismatch at dispatch fails closed "tool classification changed since this run started" (no signature changes to `execute_step*`).
- `rust/services/execution-core/src/runtime_loop/mod.rs` - `execute_step_inner` capability gate (488-539) unchanged in shape; passes the evaluation's class forward to the permission gate in S4. Keep the hunk minimal (the live tree adds `waived: bool` to this signature).
**Flag:** `TOOL_CLASS_GATE=off|shadow|enforce` (capability-core emits v2 always; execution-core behaviour by flag; code default `off`; compose default `shadow` after S3 proves; vocabulary K, unknown -> `off`). Rollback: set `off`.
**ZDR:** EvaluatePolicy and `ResolveToolBindings` are read paths (allowed for ZDR principals) and return metadata only. ZDR runs never reach them for MCP (S4). Golden-matrix: none new here.
**Tests:** Go `go test ./...` in capability-core; Rust `cargo test -q -p execution-core --all-targets` (add `capability_policy_v2_tests`: replay of a `read` decision against `browser.act` rejected; forged `mcp__x__y` with no registry row denied; v1 evidence refused under `enforce`; tamper and expiry tests extended), `cargo test -q -p mp-contracts --all-targets`; proto lint/breaking.
**DoD:** with flag `shadow`, every offered built-in call produces v2 evidence that verifies; MCP names resolve only via the registry.
**Evidence:** run a Do turn on the rebuilt stack; decode a v2 JWS from execution-core debug log; show denial for an unregistered `mcp__` name.
**Effort:** L, 6-8 d. **Deps:** S1 (S2 for MCP rows). **Risks:** hot file `capability_policy.rs`; evidence rollout order (capability-core before execution-core); `v:"1"` accepted only while `off`.

### S4 [FINISH] - Permission engine reads the class; MCP runs in Do mode
**Goal:** replace heuristics at all six call sites with the declared class; MCP read tools execute through the gate.
**File-level changes**
- `rust/services/execution-core/src/permission/mod.rs` - add `ToolClassView` (name to `Binding`) and class-aware `evaluate_call_with_class`, `rung_required_for_class`, `is_risky_class`. `evaluate_call` (43-70) reads floors: `durable_owner` -> `AwaitApproval` on every posture, `ask` floor likewise (replaces the two predicates at 53-59; `Deny` posture stays strictest); under `ask` posture risky class -> `AwaitApproval`. `rung_required_for` (100-108) and `is_risky_call` (195-200) delegate to the class when `TOOL_CLASS_GATE` is `enforce`; under `shadow` both are computed and `tool_class_divergence_total{point,tool}` increments, legacy decides. `execute_provider_action` per-call refinement stays closed: only `call_refinement=provider_operation` may lower `external_write` to `read` when `operation_is_write` is `Some(false)`; unknown operation stays `external_write` (`integration_tools.rs:747-761`).
- `rust/services/execution-core/src/runtime_loop/agent.rs` - replace the three sites (1641, 1802, plus `check_autonomy_rung` at 1688) with class lookups from the run's snapshot (resolved in `merged_tool_defs`, 622-679, once per run). `merged_tool_defs` gains `zdr` and: when `MCP_DO_TOOLS != off` and not ZDR, calls `McpGatewayClient::list_tools(..., unstaged=true)` (`mcp_gateway.rs:50-66`, add the field), intersects with `ResolveToolBindings`, offers only tools with `review_state=approved`, `binding.digest == meta.digest`, effective policy not `never`, class allowed by mode (`read`: class `read` only; `all`: any), sorted by qualified name (prompt-cache order, WS02) and capped at `MCP_DO_TOOL_CAP` (default 40; excess counted in `mcp_tools_withheld_total{reason}`, not silently dropped from telemetry). Under ZDR MCP is not offered. `data_category` (314-327) unchanged.
- `rust/services/execution-core/src/runtime_loop/retry.rs` (150) - class-based retryability (MCP `read` retryable, writes never).
- `rust/services/execution-core/src/runtime_loop/mod.rs` - `execute_mcp` (1069-1093) takes `zdr` and the binding: refuse under ZDR before any network call (typed refusal key `mcp_execution` once `zdr_refusal.rs` is committed; until then `failed_precondition`), pass `expected_digest` to `proxy_tool` (`mcp_gateway.rs:74-101`, new argument). MCP arm (747-748) unchanged.
- `rust/services/execution-core/tests/autonomy_ladder_contract.rs` - re-point `the_required_rung_is_decided_from_the_call_not_the_tool_name` (68-83): `rung_required_for` must reference the class lookup AND still read `tool_input`; the unstated-rung and loop-placement tests stay as they are.
- `rust/services/execution-core/src/permission/mod.rs` tests - rewrite `external_mcp_tools_are_gated_under_ask` (383-396) into class-driven cases: MCP `read` proceeds under `ask`, `external_write` asks, `unclassified` asks, hook `Ask` still wins.
- `rust/services/model-gateway/src/grpc.rs` (`list_mcp_tools`, WT ~1812) - D4(a): when the request metadata carries `x-capability-authorization`, use it as `capability_bearer` for hydration (today `None`). Execution-core sets it from its `capability:read` service token (`capability_policy.rs` token source).
**Flags:** `MCP_DO_TOOLS=off|read|all` (execution-core; code default `off`; compose `read` on the proof stack first; `all` blocked on D3); `TOOL_CLASS_GATE` as S3. Rollback: both to `off`; MCP stops being offered, built-ins return to legacy classification.
**ZDR:** never offered, never executed; no approval continuation for MCP under ZDR (`agent.rs:4556-4562` already refuses descriptors). Add golden case `mcp_tools_under_zdr_not_offered_not_executed` (`api/contracts/zdr-posture-matrix.json` + Rust fixture test; `bash scripts/tests/zdr-contract-sync.test.sh`).
**Tests:** `cargo test -q -p execution-core --all-targets` (Windows: `--lib`, two targets fail pre-existing); new `runtime_loop` tests: MCP read call completes through gate with verified v2 evidence; MCP unregistered denied; plan mode allows MCP `read` and refuses the rest; leaf role refuses MCP writes; mid-run class change fails closed; shadow divergence metric for MCP; `every_offered_tool_has_a_capability_binding` extended to MCP fixtures; parity: class-derived tuple equals the S0 oracle for every built-in (only MCP rows differ, listed). `cargo test -q -p model-gateway --all-targets`.
**DoD:** shadow shows zero built-in divergences on the proof stack; MCP read tool answers a Do prompt end to end; writes withheld under `read`.
**Evidence:** rebuilt images; fixture server connected; Do-mode chat asks the model to call `echo_read`: capture SSE step events, v2 evidence claims, `capability_tools` row, step audit; second prompt requesting `create_item` shows the tool absent (withheld) or, with policy `ask`, an approval pause; ZDR run shows MCP absent and a refused forced call; subscription-route (plan-mode) run allows `echo_read`, refuses `create_item`.
**Effort:** L, 7-10 d. **Deps:** S2, S3, WS03 signature rebase, D3/D4. **Risks:** behaviour change under `ask` posture for MCP reads (intended); stale gateway cache (D4); six-site edit collides with WS03/WS24 (section 5).

### S5 [BUILD] - Per-tool allow/ask/never at user and org scope
**Goal:** durable policy in capability-core, merged stricter-wins, with a management API.
**File-level changes**
- `go/services/capability-core/migrations/<next>_tool_policies.up.sql` (+down) - `tool_policies(id, org_id, scope_kind CHECK IN ('org','user'), scope_value, capability_id, decision CHECK IN ('allow','ask','never'), pinned_digest NULL, reason, set_by, created_at, updated_at, revoked_at)`; partial unique index on active `(org_id, scope_kind, scope_value, capability_id)`. `capabilities.policy_relaxable BOOLEAN NOT NULL DEFAULT FALSE`; set true only for `kind=mcp_tool` rows on creation in S2 code.
- `internal/policy/engine.go` - extend `Decide`: tighten (`ask`/`never`, org or user) applies to every row; `allow` relaxes only when `policy_relaxable`, floor `none`, class within the scope ceiling (user `external_write`, org `destructive`), no stricter org policy, and `pinned_digest` equals the row digest (else treated as `ask`); reasons `tool_policy_never|tool_policy_ask|tool_policy_allow`, `policy_source` in the evidence. `telemetry.PolicyDecisionsTotal` adds `class` and `policy_source` attributes.
- `internal/policy/policy_store.go` (NEW, `registry/` or `policy/`) and `internal/api/tool_policies.go` (NEW): `GET /api/v1/tool-policies?capability_id=`, `PUT /api/v1/tool-policies/{capability_id}` `{scope_kind, decision}`, `DELETE` (revoke). User scope: caller's own `user_id` from the signed principal. Org scope: admin from signed scopes (`admin`, `org:admin`, `*:admin`, Go twin of `ownership.rs:284-289` with a shared-vector test). Non-ZDR principal required (`authz.go`). Writes append `registry_audit_log` (feeds WS06 later).
- `rust/services/model-gateway/src/http_routes.rs` (routes at 330-343) - `GET/PUT/DELETE /v1/tool-policies...`, `GET /v1/mcp/servers/:server_id/tools`, `POST /v1/mcp/servers/:server_id/tools/:tool/approve`, proxying with the forwarded capability bearer (pattern of `mcp_list`, `mcp_delete`); `require_non_zdr_durable_mutation` on writes; test mirroring `issuer_zdr_capability_mutations_do_not_forward_or_touch_mcp_registry` (9681).
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/mcp.rs` (routes 46-54) - BFF routes with org-admin gate for org scope (`is_org_admin`, 71-74), no org id in bodies. `src/shared/actions/action-registry.ts` (MCP contracts at 785-795, 2431-2449) - add `tools.set_policy` and `mcp.approve_tool` input schemas and contracts before any UI. Tests: `action-registry.test.ts`, gateway Rust tests.
- `proto/model_plane/v1/capabilities.proto` - `ToolBinding.effective_policy` already carries the result (S3).
**Flag:** `TOOL_POLICY_ENFORCED` (capability-core, vocabulary H opt-in variant: default off; when off, stored policies are ignored and only class defaults apply). Rollback: off.
**ZDR:** policy rows are preferences, no content. Writes refused for ZDR principals (D8); reads and evaluation allowed. Golden-matrix: `tool_policy_write_under_zdr_refused`.
**Tests:** Go unit (`engine_policy_test.go` full merge matrix: user allow vs org ask, org never vs user allow, digest mismatch demotes allow, built-in allow ignored, owner_effect never relaxed), integration `tool_policies_integration_test.go`, `authz_test.go` pins; gateway and BFF tests; `cd "apps/Frontend Plane/verevonv3" && pnpm lint && pnpm typecheck && pnpm test` and `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`.
**DoD:** user sets `allow` on a trusted-read tool; org sets `never` on another; both enforced on a live run; drift demotes an `allow` to `ask`.
**Evidence:** curl through BFF with admin and non-admin sessions; two Do runs showing allow, ask pause, never denial with `policy_source`; audit rows.
**Effort:** M, 5-7 d. **Deps:** S3 (engine), S2 (MCP rows), D6. **Risks:** scope-name assumption for admin (confirm Control mints `admin`/`org:admin`: not determined here); policy keyed by capability id, so re-created rows lose policy (rows keep ids across re-observation).

### S6 [FINISH] - Enforce and delete
**Goal:** class is the only classifier.
**File-level changes:** delete `is_risky_tool` keyword list and the `mcp__` blanket (`permission/mod.rs:229-285`), `requires_durable_owner_approval`/`requires_consent_to_disclose` bodies become floor lookups (keep the names as thin wrappers used by `grpc.rs:707` and `agent.rs:4447-4460, 4567`); `provider_action_is_risky` stays as the refinement; update comments in `tool_loop.rs:603-613` (no longer "until a binding exists"), `approval_delivery_worker.rs:16-27` (class vocabulary), `capability-ownership-matrix.md`; ledger entries to `implemented`. Compose default `TOOL_CLASS_GATE=enforce`.
**Gate to start:** two weeks of `shadow` with zero built-in divergences and the MCP-read proof stack evidence.
**Tests:** full gates (section 4 commands) plus `cargo test --workspace --no-fail-fast`, `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`; goldens unchanged.
**Effort:** S, 2-3 d. **Deps:** S4, S5. **Risks:** deleting the fallback removes the rollback for built-ins; keep the flag `off` path until one release has run in `enforce`.

## 5. Interfaces

**PROVIDES**
- To WS13 (approvals v2) and WS14: `approval_floor` + registry never-auto set; `policy_source` and `side_effect_class` in the signed decision for cause codes and the step-up set; single-use for `owner_effect`.
- To WS20 (scheduled runs): non-delegable set (`owner_effect`, floor != none) and class ceilings for an unattended allowlist (`read` only by default).
- To WS23/WS57 (connected apps, custom connectors): `capability_tools` MCP rows, `ResolveToolBindings`, `annotation_trust`, `tool_policies` API, flag `MCP_DO_TOOLS`.
- To WS24 (typed action registry) and WS30 (workspace file tools): contract "a new tool is not offered until it has a `capability_tools` row, a class, and an attestor", enforced by the S1 parity test; `mp-contracts::tool_class`.
- To WS25 (org policy registry, hooks): `tool_policies` store; rule that hooks may only narrow (class floor and `never` always win; `hook/mod.rs:117-120` fail-open stays a non-authority).
- To WS21/WS22: class rows for `browser_agent`, `browser.*`, `process_*`, `code_interpreter` (declarations only).
**CONSUMES**
- WS03: signature change `waived: bool` in `execute_step*` (`runtime_loop/mod.rs`), durable resume that could later allow D3(c) for MCP.
- WS02: ordering and size of the merged tool list (`merged_tool_defs`), run-context resolver.
- WS04: replica-safe MCP registry hydration (overlaps D4); until then the exec-core-forwarded bearer.
- WS01: deployed stack, `SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY` provisioned (audit §9.1 dependency).
- WS06: audit emitter for policy and observation events (today `registry_audit_log` only).
**Conflict hot-spots and order**
| File | Shared with | Rule |
|---|---|---|
| `execution-core/src/runtime_loop/agent.rs`, `runtime_loop/mod.rs` | WS02, WS03, WS04, WS24 | WS03 lands the `waived` signature first; WS05 edits stay to the three call-site replacements and `merged_tool_defs`; rebase before S3/S4 |
| `execution-core/src/permission/mod.rs`, `capability_policy.rs` | WS13, WS14, WS24, WS25 | WS05 S1/S3 first (class lookup is the seam), then others add predicates as floors/rows, not new name lists |
| `capability-core` `server.go`, `engine.go`, migrations | WS23, WS25, S4.3 Watch work | WS05 S1 first; integrator assigns migration numbers |
| `gateway.proto`, `capabilities.proto` | WS04, WS07, WS23 | additive fields only; WS05 field numbers above are the next free as of HEAD, re-verify at merge |
| `model-gateway/src/grpc.rs`, `runtime_registries.rs` | WS04, ZDR live changes | rebase on committed ZDR helper first |

## 6. Cross-session interfaces
- **(a) Verevon Space session:** WS05 introduces org and user policy scopes only. Space-scoped policy is not designed; if Space needs it, it must be tighten-only through the same engine, keyed by `SpaceRef` (ADR-0001) in a later `scope_kind`, never widening an owner-plane ACL. `tickets.create` (`owner_effect`, durable owner approval) stays the Application Plane owner effect. Capability-core migration numbering and `plane-service-principals.json` edits (S4.3 Watch, memory note: capability-core cannot address sandbox-manager yet) collide with S1/S5: coordinate numbers through the integrator. Assumes the Space session delivers nothing WS05 blocks on.
- **(b) Quarry/browser session:** WS05 only declares classes for `browser_agent`, `browser.act` (`external_write`) and `browser.observe` (`read`). Model proposes, Quarry executes is untouched. `browser.observe` still asks today because it shares high-risk `cap.browser.open` (D9); splitting that row belongs to the browser workstream (WS21) with per-site policy layered stricter-wins. Assumes Quarry keeps the existing versioned browser contract; no shared golden is added by WS05.

## 7. Regression guards
| Audit section 8 strength | How WS05 touches it | Proof it is not regressed |
|---|---|---|
| Signed, fail-closed capability gate | Evidence gains claims; MCP resolves via registry | S3 tests: unknown/forged/unbound `mcp__` name denied; read decision replayed for another tool rejected; tamper/expiry tests kept (`capability_policy.rs:921-937`); `bindings_are_exact_and_unknown_or_dynamic_tools_fail_closed` kept for built-ins |
| Ask/Do switch and autonomy ladder | Rung input becomes the class | `autonomy_ladder_contract.rs` re-pointed not deleted (per-call arguments, unstated rung unconstrained, loop-only placement); S0 oracle parity for every built-in |
| Two loops separate | Inline loop untouched | `tool_loop.rs` tests 8929, 8966, 9430 unchanged; new source-text test that `tool_loop.rs` does not reference `tool_class`/`ResolveToolBindings` |
| ZDR by construction | MCP never offered/executed under ZDR; policy writes refused | golden-matrix cases in S2/S4/S5; `bash scripts/tests/zdr-contract-sync.test.sh` |
| Honest outcomes | Drift, withheld and changed-class paths return typed failures | tests assert error text names the cause; no success fabricated; `mcp_tools_withheld_total` is counted |
| Credential isolation | No tokens, descriptions or schemas persisted; admin from signed scopes only | `mcp_capability_payload` no-token test (`runtime_registries.rs` writethrough tests) extended to the observation payload; HMAC wrong-org rejection test |
| Model proposes, Quarry executes | Browser rows are declarations | no Quarry code or contract change |

## 8. Effort and risks
**Total:** L, upper end (about 31-44 dev-days: S0 1-2, S1 4-6, S2 6-8, S3 6-8, S4 7-10, S5 5-7, S6 2-3). Turns XL only if D3(c) (resumable MCP approvals) or Ask-mode MCP is pulled in.
**Critical path:** S0 -> S1 -> S3 -> S4 -> S6 (about 25 d with S2 and S5 in parallel by a second engineer; Go and Rust halves split cleanly after S1).
**Top risks**
1. Mis-declared class is silent over-permission. Mitigation: untrusted-by-default annotations (D2), TOFU plus digest pin with drift quarantine, derivation only tightens, class never loosens a seeded row, user relaxation bounded and digest-pinned.
2. Hot-file conflict and live uncommitted work (`agent.rs`, `runtime_loop/mod.rs`, gateway `grpc.rs`). Mitigation: land after WS03, hunk-minimal edits, rebase gates in section 2.2.
3. Behaviour drift when the heuristic is replaced. Mitigation: S0 oracle, `shadow` mode with divergence metric, two-week soak before S6; keep `off` path for one release.
4. Approved MCP writes dead-end (no cold resume). Mitigation: D3(b) withhold writes behind `MCP_DO_TOOLS=read`.
5. Gateway cache cold or per-replica, so MCP "works on one replica". Mitigation: D4(a) forwarded bearer, then WS04; offer filter requires registry approval so a cold cache fails to offer rather than to authorise.
