# ADR 7: External contract (OpenAPI, MCP, CLI, n8n, SDK waiters)

- **Status:** proposed at the plan recommendation. Not ratified. The owner (the user) decides, and no code is gated on this note.
- **Date:** 2026-11-11
- **Decider:** the user (plan decision ADR-7 / OQ-6)
- **Source:** `docs/plans/quarry-browser-build-plan-2026-10-14.md`: section E4.7 (F-t2-064 and F-t2-065), the stale-document row for `docs/OSS_PARITY_BACKLOG.md` (section 3.5), and the decision table row ADR-7 / OQ-6.
- **Related:** ADR 6 (script execution model), ratified 2026-10-28 in the plan's decision table: closed, declarative, capped action lists; `Evaluate` stays denied.

## Context

Plan item F-t2-064 asks how external callers reach Quarry. The options were REST only, or an audience-bound MCP front outside Quarry with customer API keys. The plan's E4.7 recommendation splits the item: the OpenAPI slice is built now, SDK wait helpers and MCP are parked, and the CLI and n8n scope is deleted. F-t2-065 (cached extraction profiles) is parked. This note records that recommendation. It changes no code, route, SDK, CI step or switch.

## Decision

1. **OpenAPI is the external contract.** `docs/openapi.yaml` is maintained by hand and checked by tests. The TypeScript and Python SDKs are generated from it.
2. **MCP server: parked.** No MCP front is built. Reopening it needs an owner decision. If the owner says go, the front is audience-bound and lives outside Quarry, customer keys are held in Control Plane, connectors are read-only, and tool-call audit goes through audit-core, never as a third audit producer.
3. **CLI and n8n node: deleted from scope.** Neither is planned.
4. **SDK wait helpers (`scrapeAndWait`, `crawlAndWait`): built only on demand.** Build them only after WS15 and only if an external SDK consumer exists.
5. **Cached extraction profiles (F-t2-065): parked.** They wait for ADR 6, the WS16 model grant and cumulative cost enforcement.

## Evidence and reopen triggers

### 1. OpenAPI is the external contract

Evidence:
- `sdks/generate.sh:8` and `:25-40` generate the Python (`quarry_client`) and TypeScript (`@quarry/client`) SDKs from `docs/openapi.yaml`.
- `.github/workflows/quarry-v2-ci.yml:270-274` runs the scrape-options golden, which is generated from the Rust type (`crates/quarry-core/tests/scrape_schema.rs`). The comment at `:270-272` says the OpenAPI file is checked against it and against the routes the edge registers.
- `.github/workflows/quarry-v2-ci.yml:283-284` runs the route-coverage and scrape-parity tests in CI. `scripts/tests/openapi-route-coverage.test.mjs:1-3` says every registered edge route is documented in `docs/openapi.yaml`, documented in `docs/openapi-internal.yaml`, or listed on `scripts/openapi-route-allowlist.json` with a reason.
- The two internal-spec guards, `scripts/tests/openapi-internal-guard.test.mjs` and `scripts/tests/openapi-no-bare-null.test.mjs`, are not in the route-coverage step. Q4-E4.7-B8b runs them as their own CI step (`.github/workflows/quarry-v2-ci.yml:293` in the working tree, uncommitted when this note was written).

Open work, not a reopening: extend the coverage check to crawl, jobs, schedules and agent routes as WS14, WS15, WS2 and WS8 add them (plan E4.7).

### 2. MCP server: parked

Evidence:
- `docs/OSS_PARITY_BACKLOG.md:30` and `:112-114` record the deferral: Verevon is the client, Quarry serves it over REST, and the item is revisited "only if third-party agent access becomes a goal."
- The plan's E4.7 entry says an MCP server "needs customer authentication that does not exist." That holds for an MCP front, but it is not the whole picture. Quarry already has an `api_key` principal for agent runs (INNH-10). It is dark behind `QUARRY_EDGE__API_KEY_AGENT_RUNS` (`crates/quarry-edge/src/run_limits.rs:47`), off by default (`docs/ERROR_REASONS.md:304`). It is limited to a nine-route allow-list and the `browser:agent` scope (`docs/CONTRACTS.md`, section "API-key agent runs", from line 1120). Auth Core mints the key's scope (`docs/ERROR_REASONS.md:305`). It is not an MCP front. Re-check the premise against it before any MCP design.

Reopened by: an owner decision that third-party agent access is a goal. The "go" conditions in decision 2 then apply, and a new plan unit owns the work.

### 3. CLI and n8n node: deleted from scope

Evidence:
- No Quarry CLI exists in the tree. The only `src/bin` target is `crates/quarry-edge/src/bin/profiles_backfill.rs`, the only `cmd/` entries are the control and orchestrator services, and no path named `cli` exists under `crates`, `services`, `apps`, `tools`, `sdks`, `scripts` or `docs` (the `cli*` matches are `client.go` files).
- No n8n node exists. The only mention is a client category in `docs/TRANSPORT.md:22` ("browsers, CLIs, Cursor, n8n"). This note does not change it.

Reopened by: an owner decision that puts a CLI or an n8n node in scope. That is a new plan unit and a new ADR section.

### 4. SDK wait helpers: on demand only

Evidence:
- The generated SDKs have no wait layer (`sdks/generate.sh:25-40`). `scrapeAndWait` and `crawlAndWait` appear nowhere in the repo outside plan documents.
- The one in-repo TypeScript caller, `apps/Frontend Plane/verevonv3/src/shared/api/quarry-client.ts` (a wrapper over `@quarry/client`), is imported only by its own test, `quarry-client.test.ts`.
- The verevonv3 gateway calls Quarry over HTTP with `reqwest` (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs:8`, `:327`).

Reopened by: WS15 (job lifecycle) landing and an external SDK consumer existing. Both are needed.

### 5. Cached extraction profiles (F-t2-065): parked

Evidence:
- `ExtractionProfile` is defined at `crates/quarry-core/src/contracts.rs:441`. The step request's profile is read at `crates/quarry-runtime/src/observation.rs:1047`.
- The executor is a bounded string search over the captured HTML, not a selector engine. `extract_profile` (`observation.rs:3097`) uses `extract_dom_field` (`:3163`) and `extract_json_ld_field` (`:3193`). Network-JSON and visual sources are skipped (`:3093-3096`).
- The invoke grant is closed. The `model-invoke`, `model-stream` and `model-audio` rows are `granted: false` (`crates/quarry-runtime/contracts/cross-plane-token-requests.json:45`, `:62`, `:73`). The two `model-extract` rows are granted for AI formats and requested by `mp_client.rs` (`granted: true` at `:95` and `:106`, `used_by` at `:96` and `:107`; no `pending` key).
- Cost enforcement: `AgentLoop::over_budget` (`crates/quarry-runtime/src/agent_loop.rs:156`) is called only at `:251`. The step admission runs `enforce_step_budget`, which checks `max_steps` only (`crates/quarry-edge/src/agent_routes/budget.rs:5`, called at `agent_routes/admission.rs:519`). It then runs `budget_gate` (`budget.rs:15`, called at `admission.rs:522`), which is mode-gated. The documented default mode is `shadow` (`crates/quarry-edge/src/budget.rs:58-59`), which records and refuses nothing. `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` refuses, and that mode waits for the scripted stack proof, `scripts/x12-budget-stack-proof.mjs` (`budget.rs:37-38`). The plan's "checks only max_steps" describes `enforce_step_budget` alone.
- No extraction-profile cache exists. `crates/quarry-runtime/src/cached_profile_store.rs` caches browser session snapshots in Redis, not extraction profiles.

Reopened by all four steps, in the plan's order:
1. rebuild the executor on a real selector engine with a versioned schema;
2. the WS16 model grant for profile generation (flipping the invoke row needs an owner decision to open those callers, per the row's notes);
3. a per-org, non-ZDR cache keyed by org, URL template, schema hash and model;
4. bounded escalation, only after cost enforcement is `on` and proven.

None of the four starts from this note.

## Risks if reopened

- An ADR 7 reversal that creates a third audit producer.
- Profile generation cost before budgets bind.
- The model-gateway grant has no owner (plan E4.7).

## ZDR

This note stores and forwards no content. If the profile cache is unparked, it stays per-org and non-ZDR only. An MCP front, if unparked, lives outside Quarry, and Control Plane authenticates its callers (plan E4.7 ZDR check).

## Not decided here

- The owner has not ratified this note.
- No code, route, SDK, CI step or switch changes.
- The CI wiring for the internal-spec guards belongs to Q4-E4.7-B8b (`.github/workflows/quarry-v2-ci.yml:293`, uncommitted when this note was written).
