# progress-A (lane A, execution-core + mp-content-safety)

Env for every cargo run: `CARGO_TARGET_DIR=C:/dev/mp-w2w3-target-A CARGO_BUILD_JOBS=2`, cwd `apps/Model Plane/rust`.
Note: the Model Plane crates are not clean under default `cargo fmt --check` (existing style differs), so no crate-wide fmt was run; edits follow neighbouring style by hand.

## F12 trust fix
- Built: `crates/mp-content-safety/src/trust.rs` `TOOL_TRUST` += `browser.observe`, `browser.act`, `browser.console` as `BrowserScraped`; new test `trust_class_browser_mcp_tools_are_browser_scraped`.
- Tests: `cargo test -p mp-content-safety` -> lib 21 passed, corpus 10 passed, doc 0.
- Dependent expectations: grep found no test in execution-core/model-gateway/session-core pinning these tools as OrgInternal; `tool_class_registry_parity.rs` is about permission classes, not trust (no change).
- Flags: none (hygiene always on). Not verified: live render of `[source: browser-scraped]` for parent-loop browser tools (covered by WS11.S0 provenance tests below).

## WS11.S0 planner seam, view hygiene, ZDR, hermetic harness
- Built (all under `apps/Model Plane/rust/services/execution-core`):
  - `src/llm_planner.rs`: `ActionPlanner` trait (`plan_next(&PlannerContext) -> Result<PlannerReply, PlannerError>`), `PlannerContext{config, view}`, `PlannerReply{action, done, usage}`, `PlannerUsage`, `PlannerError{kind: Unauthenticated|Transient|Refused|Parse|Other, message}`; `impl ActionPlanner for LlmPlanner` (sets `x-org-id` metadata, usage from `InferResponse`, parse errors no longer echo the model reply); `planner_infer_request` keeps its signature and delegates to new `planner_request_from_view`; `InferRequest.zdr = config.zdr`.
  - `src/browser_agent/planner_view.rs` NEW (`pub mod planner_view`): `PlannerView` (page text cap 12,000 chars, 60 targets, 120-char labels, value text dropped for password/input-like roles, hidden chars removed, fragment-stripped URL, delimiters `<<<BEGIN/END UNTRUSTED PAGE CONTENT>>>` neutralised in page text, `[source: browser-scraped]` header + SCREENING line via `provenance::assess/render`, content-free `tracing::debug!`), two scrub seams `scrub_page_text` / `scrub_page_url` (WS12.S4 re-points at mp-scrub).
  - `src/browser_agent.rs`: `run_browser_agent_loop` / `decide_next_action` take `Option<&dyn ActionPlanner>`; the planner gets only the view; the risk gate and bot-wall check still read the raw observation; `Err` still falls back to `Observe`.
  - `src/tool_bridge/mod.rs` (coercion at the call site), `tests/browser_agent_e2e.rs` (coercion), `src/provenance.rs` (test `browser_mcp_tools_render_as_browser_scraped`).
  - Harness: `tests/support/{mod,scripted_planner,fake_quarry}.rs`, `tests/browser_planner_hermetic.rs` (FakeQuarry = axum on 127.0.0.1:0 driving the real `QuarryAgentClient::new`).
- Tests (cwd `apps/Model Plane/rust`, env `CARGO_TARGET_DIR=C:/dev/mp-w2w3-target-A CARGO_BUILD_JOBS=2`):
  - `cargo test -p execution-core --lib -- planner_view llm_planner provenance browser_agent` -> 139 passed, 0 failed (includes the 5 named planner_view tests, `planner_request_sets_zdr_from_config`, new provenance test).
  - `cargo test -p execution-core --test browser_planner_hermetic` -> 2 passed (`scripted_planner_drives_loop_against_fake_quarry`, `planner_sees_scrubbed_observation_only`).
  - `cargo check -p execution-core --tests` -> exit 0 (before the hermetic files were added; they then compiled in the test run above).
- Flags: none (hygiene always on). Rollback = revert.
- Assumed: PlannerContext carries the view only (not the raw observation); password/input roles keep their label, lose their text; the screening pass also covers page text past the cap.
- Not verified: live LlmPlanner against inference-core (x-org-id metadata, usage counts); `cargo fmt` not run (crate not fmt-clean, hand-formatted); live `[source: browser-scraped]` render of browser.observe/act in the parent loop.

## NOT DONE (stopped on the lead's usage-limit order)
- C0 + WS11.S1 typed-error seam (quarry_agent.rs contract fields, params structs, `parse_typed_error`, `StepFailureClass`, `classify_quarry_error`).
- WS11.S1 execution-core half (budgets, ceilings env, `max_planner_tokens`, U21 402 `BUDGET_EXHAUSTED` -> `BrowserResourceLimit::QuarryBudget(dimension)` / `resource_exhausted`, 429 backpressure, compose env lines).
- Resume notes: Quarry 402 body is `error.code="BUDGET_EXHAUSTED"`, `error.details={dimension: steps|attempts|runtime|cost|org_allowance, limit, spent, unit, estimated, limit_source: grant|run_budget, resumable:false}`, `retryable:false` (Quarry `docs/CONTRACTS.md` "Agent run budgets", `docs/ERROR_REASONS.md`); 429 details carry `reason` and `retry_after_seconds`. `RequestAsk` (quarry-core web_policy) has `deny_unknown_fields`, fields `disable, robots_max, proxy_max, deny_domains, allow_domains: Option<Vec>`; trace body is `{run_id?, agent?:{id?,role?,parent_id?}, origin?}`. Tests pinning non-2xx as `AgentClientError::Status` (quarry_agent.rs ~1195, 1201, 1368) must be updated when `Typed` lands; call sites of start_run/step: browser_agent.rs (864, 916), tool_bridge/mod.rs (333), quarry_agent tests.
- Cold lib-test build in the lane-A target takes ~12 min; incremental is much shorter.
