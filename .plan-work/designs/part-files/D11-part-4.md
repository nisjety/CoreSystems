
### S4. Widened action set and the risk gate that covers it (F-t2-010 (b), third part; effort L, 7 engineer-days)

**Goal.** The planner can `press`, `select`, `back` and `wait_for`, each mapped onto Quarry's governed `AgentAction` with execution-core-side admission that only tightens (D6), and the deterministic risk gate sees the new verbs (D7, F5). Quarry still validates every action and may refuse; the Model proposes.

| Path | Symbol and change | Tag |
|---|---|---|
| `EC/browser_agent.rs` | `ActionType` (91-113) gains `Press`, `Select`, `Back`, `WaitFor` with `as_str` `press`, `select`, `back`, `wait_for` (the only exhaustive match outside tests is `quarry_agent::action_to_wire`; the risk code uses `matches!`); `AgentPlan` gains `history: UrlHistory` (observed URLs from successful observations, bounded 20) and `focus_hint: Option<BrowserSnapshotTarget>` (target of the last `type` or `select` on the current page, cleared when the URL changes); `decide_next_action` calls `admit_proposal` after the planner reply and before `gate_risky_action`; a rejected proposal becomes a recoverable `ActionFailure` fed back to the model (counts toward the failure budget and the step count, never reaches Quarry) | [C] |
| `EC/browser_agent/admission.rs` | `KEY_ALLOWLIST` (`Enter`, `Tab`, `Escape`, `Backspace`, `Delete`, `Space`, `ArrowUp/Down/Left/Right`, `PageUp`, `PageDown`, `Home`, `End`); `admit_press` (name in list, no `+` chord, no single printable character), `admit_select` (bound ref required, value at most 256 bytes), `admit_back` (previous observed URL exists and `is_domain_allowed`), `admit_wait_for` (bound ref required, `clamp_wait(250..=10_000)`); each returns `Result<(), RejectKind>` with fixed vocabulary | [NEW] |
| `EC/quarry_agent.rs` | `action_to_wire` (346-402): `Press` to `AgentAction::Press { key }`; `Select` to `SelectRef` or `FrameSelectRef` per `target.frame_id`; `Back` to `AgentAction::Back`; `WaitFor` to `WaitForRef` or `FrameWaitForRef` with `timeout_ms`; the legacy CSS `Select` and `WaitFor` variants are never produced | [C] |
| `EC/llm_planner.rs` | `ACTION_SCHEMA` (53-68) and `ACTION_SCHEMA_STRICT` (85-101): action enum gains the four verbs; new properties `key` and `timeout_ms` (strict twin: `""` and `-1` sentinels, `required` list extended); `NextAction` gains `key` and `timeout_ms`; `into_browser_action` maps `press` to `value = key`, `select` to `value`, `wait_for` to `max_wait_ms`; the vocabulary-drift test (699-875) extended; `procedure.rs` rung text mentions the verbs | [C] |
| `EC/browser_agent/browser_risk.rs` | `bound_target` (354-366) also resolves `Select`; for `Press` with `Enter` or `Space` the target comes from the focus hint and the posting markers apply (line 293 widened from `Click` to `Click` or an Enter or Space `Press`), and the evidence includes the observed page URL when there is no focus hint; `Evidence::of` (324-340) reads the target `text` for `Select` as for `Click`; unit tests alongside the existing 558-1156 block | [C] |
| `ECT/support/fake_quarry.rs` | pages with a select, a search form, history, async content, a login form | [NEW] |
| `ECT/browser_planner_hermetic.rs` | scenarios below | [NEW] |

Companion, **not in this slice and owned by Ingestion**: validate `Press.key` at the edge against the same allowlist (`Q/crates/quarry-edge/src/agent_routes.rs` step handler; unvalidated today, F13), and add the destination check for `Back` before the 403 (belongs with [D2]/[D3] edge work). Until then execution-core is the only check for those two verbs.

**API / data model.** No proto, no SQL, no Quarry change: `Press`, `SelectRef`, `FrameSelectRef`, `WaitForRef`, `FrameWaitForRef`, `Back` already exist in `quarry-core` `AgentAction` (`Q/crates/quarry-core/src/contracts.rs:640-827`) and in the execution-core wire mirror (`EC/quarry_agent.rs:44-166`). Run events carry the verb through `BrowserEventSink::action_dispatched` (`action_type.as_str()`), additive.

**Flags and rollout.** No new flag: the verbs exist only when a planner exists (S2). Rollback: revert; or `QUARRY_BROWSER_AGENT_PROCEDURE=legacy/0` stops the prompt from mentioning them (the schema would still allow them), so a full revert is the real rollback.

**Tests.**
- `cargo test -p execution-core --lib -- admission llm_planner quarry_agent browser_risk browser_agent`: `press_allowlist_accepts_named_keys_only`, `press_rejects_modifier_chords_and_printable_characters`, `select_requires_a_bound_ref_and_never_emits_the_legacy_css_variant`, `select_in_child_frame_maps_to_frame_select_ref`, `back_rejected_without_a_previous_allowed_url`, `back_allowed_when_previous_url_is_allowed`, `wait_for_clamps_timeout`, `wait_for_requires_a_bound_ref`, `action_to_wire_maps_each_new_verb`, `strict_and_plain_schemas_agree_after_widening`, `press_enter_in_password_field_is_login_risk`, `press_enter_in_field_named_send_is_posting_risk`, `press_enter_with_no_focus_hint_on_checkout_url_is_checkout_risk`, `select_on_destructive_labelled_control_is_destructive_risk`, `press_enter_in_search_box_is_not_risky`.
- Hermetic (`--test browser_planner_hermetic`): `search_box_type_then_press_enter_reaches_results`, `select_option_then_confirm_by_reobserve`, `back_returns_to_previous_page`, `wait_for_async_content_then_extract`, `modifier_chord_is_rejected_locally_without_a_quarry_call`, `forbidden_off_domain_after_back_is_terminal`, `risky_press_pauses_for_approval`.

**Definition of done.** Each verb works end to end against the fake Quarry and against a live dev Quarry; the four admission rules and the extended gate are covered by tests; no code path emits a CSS-selector `Select` or `WaitFor`.

**Deployment evidence.** Rebuild execution-core (and Quarry edge only if the companion lands); dev stack, mode `allowlist`, real bearer. Flows on a static fixture site: (1) search form: the run types a query and presses Enter, Quarry's receipts list `type_ref` then `press` then `get_content`, final summary names the first result; (2) a `<select>` shipping-method control: receipts show `select_ref` with the chosen value and the run confirms by re-observing; (3) back: navigate to a second page, `back` returns, receipts show `back`; (4) async content behind a 2 s timer: `wait_for_ref` receipt then the answer; (5) gate: the fixture login form (password field), the run types and then proposes `press Enter`: the run pauses (`RunPausedForApproval` in `/v1/runs/<id>/events`) and approving or denying behaves as for a click; (6) a model-proposed `Control+A` never produces a Quarry receipt (`GET .../receipts` count unchanged) and the planner receives the fixed rejection text.

**Effort.** L, 7 engineer-days (schema and strict twin 1, admission and history 1.5, wire mapping 0.5, risk gate 1.5, fake Quarry pages 1, scenarios and live evidence 1.5). **Risks.** The risk gate is heuristic (the file's own doc says so); a `press Enter` false positive costs an approval prompt, a false negative is a missed gate, so the focus-hint rule is conservative and the domain fence remains the hard guard; Quarry does not validate `press` (F13) until the companion lands; `browser_risk.rs` and `llm_planner.rs` are shared with [D3] and [D12] (section 5).

### S5. The staged rollout switch, boot guard and metrics (F-t2-010 (c), second half; effort S, 2 engineer-days)

**Goal.** Make stage 2 and stage 3 of D10 an operations change with a visible guard and the numbers needed to watch it, and retire the legacy env gate. No new product behaviour.

| Path | Symbol and change | Tag |
|---|---|---|
| `EC/planner_policy.rs` | `default_on` requires `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` to contain `grants`, `budgets`, `redaction`, `evals`; otherwise log an error and use `org_opt_in`; a one-line boot banner states the effective mode, allowlist size, procedure id, ceilings | [NEW in S2, changed here] |
| `EC/tool_bridge/mod.rs`, `EC/browser_agent.rs` | metrics with the `metrics` crate already in `Cargo.toml`: `execution_browser_runs_total{planner,outcome}`, `execution_browser_planner_tokens_total{direction}`, `execution_browser_failures_total{kind}`, `execution_browser_deterministic_total{reason}` (low-cardinality labels only; never org id, URL or text) | [C] |
| `EC/llm_planner.rs` | remove the `QUARRY_BROWSER_AGENT_LLM` read and `from_env` (the legacy path), at stage 4 in a separate PR | [C] |
| `ECT/browser_agent_e2e.rs` | live test uses the mode variables only | [C] |
| `MP/docs/BROWSER_PLANNER_ROLLOUT.md` | runbook: stage table, prerequisites, kill switch, dashboards, the OD13 decision | [NEW] |
| `MP/docs/decisions/ledger.md` | entry: "durable browser planner selection is a deployment mode plus an org flag; interim carrier is an org-core quota row until F-t2-022" | [C] |
| `MP/deploy/docker-compose.yml` | no default change (`off` stays); staging overlay sets the mode | [C] |

**API / data model.** None. **Flags.** `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` (guard only). **Rollback.** Mode `off`, restart.

**Tests.** `cargo test -p execution-core --lib -- planner_policy`: `default_on_without_ack_downgrades_to_org_opt_in`, `default_on_with_full_ack_stays_default_on`, `banner_never_logs_org_ids_or_secrets`; metric label tests (`metrics_labels_are_low_cardinality`).

**Definition of done.** The prerequisites of stage 3 in D10 are all recorded green in the runbook with the dates and commit ids; staging has run at stage 2 for one release.

**Deployment evidence.** Staging (rebuilt images). Mode `default_on` without the acknowledgement: the boot log shows the error and the effective mode `org_opt_in`; with the acknowledgement: `default_on`. An org with no flag starts a browser run: `planner:llm`, `execution_browser_runs_total{planner="llm",outcome="completed"}` increases on the metrics endpoint; the same org flipped to `2` in org-core: `planner:deterministic`; `execution_browser_deterministic_total{reason="org_off"}` increases. Kill switch drill: mode `off` and restart, a new run is deterministic, an in-flight run (started before) completes under its own limits.

**Effort.** S, 2 engineer-days. **Risks.** Stage 3 is blocked by decisions and slices outside this design (D10); the acknowledgement is a ritual, not proof.

### S6. Connector-first routing guidance (F-t2-010 (d); effort M, 3 engineer-days)

**Goal.** The parent chat loop tells the model, per the tools actually offered, to prefer provider actions, then public search and fetch, and to use a browser only on explicit request or when the cheaper routes cannot answer or failed; compliance is measured, not assumed (D12).

| Path | Symbol and change | Tag |
|---|---|---|
| `EC/runtime_loop/agent.rs` | `const SNIPPET_ROUTING: &str`; `compose_system_prompt` (208-248): emit it when the offered set holds a connector tool (`list_provider_actions`, `execute_provider_action`) and at least one web or browser tool (`web_search`, `web_fetch`, `web.search`, `web.read`, `browser.observe`, `browser.act`, `browser_agent`); append the browser-open sentence only when `browser_agent` is offered; a per-run local that records the first non-discovery tool class into `execution_routing_first_tool_total{class}` (`connector`, `search`, `browser`, `other`) in phase 3 (around 2051-2075) | [M] |
| `EC/runtime_loop/agent.rs` tests | `routing_snippet_present_only_when_connector_and_web_tools_are_offered`, `routing_snippet_names_browser_agent_only_when_offered`, `prompt_stays_a_pure_function_of_the_offered_set` (existing test extended), `routing_counter_records_first_non_discovery_tool_class` | [M] |
| `ECT/routing_probe.rs` | `#[ignore]` live probe: a fixed list of prompts with the real tool definitions against inference-core (connector-solvable, public lookup, explicit browser request), asserts the first non-discovery tool class and prints a table; run notes in the header like `planner_on_terra_returns_valid_action_json` | [NEW] |
| `MP/docs/BROWSER_PLANNER_ROLLOUT.md` | the measured compliance numbers once the probe has run | [NEW in S5] |

**API / data model.** None. **Flags.** None (a prompt snippet; the offered set is the switch). **Rollback.** Revert the snippet.

**Tests.** `cargo test -p execution-core --lib -- runtime_loop::agent routing` (names above). Live probe: `cargo test -p execution-core --test routing_probe -- --ignored --nocapture` with the same environment as the existing live probes.

**Definition of done.** The snippet appears exactly when it should and never names a tool that is not offered; the counter exists; the live probe has been run once and its table is in the rollout runbook (a compliance floor of 90 percent on the connector-solvable prompts is the proposed gate before stage 3, `[placeholder]`).

**Deployment evidence.** Rebuild execution-core; chat stack with an org that has a connected provider. Flow 1: "list the latest posts on my connected Facebook page": the run's tool-step sequence (session events or the audit rows' `tool=` names) shows `list_provider_actions` before any `web_*` or `browser.*` call; flow 2: "what does the public page at example.com say about pricing": `web_fetch` without a provider call; flow 3 (explicit browser): "use the browser, not search": no provider call is forced and the model proceeds with the browser tools it has. The metrics endpoint shows `execution_routing_first_tool_total` increasing for the classes seen.

**Effort.** M, 3 engineer-days (snippet and tests 1, counter 0.5, probe and cases 1, evidence and runbook 0.5). **Risks.** A model can ignore prompt text (the audit says to cite Codex's version only as prompt text); eager routing can override an explicit browser request, so the snippet names that exception first and the probe has an explicit-browser case; `agent.rs` is [M] with in-flight hunks nearby (section 2.4, no overlap, rebase).

### S7. Web-taint memory guard, skip-only (F-t2-045 skip-only slice; effort M, 3.5 engineer-days)

**Goal.** A run that read web, browser or MCP content cannot write a model-initiated memory, and Dreaming does not extract memories from assistant text produced in such a run. Skip only: no pending-review queue, no promotion RPC.

| Path | Symbol and change | Tag |
|---|---|---|
| `EC/external_content.rs` | `ExternalContentLatch` (`Arc<AtomicBool>`, `set()`, `is_set()`, `Clone`, `Default`, same shape as `RefusalLatch`, `EC/subscription_refusal.rs:185`); `is_external_content_tool(name)` (`TrustClass::classify(name).is_external()` with F12 fixed); `transcript_has_external_tag(&[message])` (looks for the three `[source: ...]` tags) | [NEW] |
| `EC/lib.rs` | `pub mod external_content;` | [C] |
| `EC/runtime_loop/agent.rs` | `LoopContext` (362-430) gains `external: ExternalContentLatch`, created with the root context near `refusal` (917) and cloned into delegated children (about 3114); at cold resume the latch is initialised from the restored transcript; the sequential pre-pass (1801-1826) rejects a `save_memory` call, with `CallSlot::Rejected` and the message "save_memory refused: this run read web, browser or third-party content, which may contain instructions; ask the user to confirm the exact text and save it in a later turn that does not read external content", when the latch is set or the same batch holds an external-content call (F15); phase 3 (2051-2075) sets the latch after a successful external-content result (a failed fetch does not taint); `record_tool_step` is unchanged (the prefix already carries `tool=<name>`) | [M] |
| `SC/src/external_content.rs` | `is_external_content_tool(name) -> bool` (`web_search`, `web_fetch`, `fetch_url`, `web.search`, `web.read`, `browser_agent`, `browser.observe`, `browser.act`, `mcp__*`: the same set as `EC/provenance.rs` `TrustClass` after F12, mirrored by hand and pinned by a unit test in each crate) and `mark_external_content(tx, run_id)`: `UPDATE runs SET external_content_at = COALESCE(external_content_at, now()) WHERE id = $1 OR id IN (SELECT parent_run_id FROM subagent_edges WHERE child_run_id = $1)` (the parent's window must carry the marker because the answer is written under the parent) | [NEW] |
| `SC/src/main.rs` | `mod external_content;` (session-core has no `lib.rs`; one line, in-flight waiver hunks are elsewhere) | [M] |
| `SC/src/grpc.rs` | two call sites inside existing transactions: `enqueue_tool_action_audit` (about 3664-3709, the `CompleteStep` transaction, tool name from `detail.tool` or `resolve_tool_name`) for execution-core runs, and `reserve_tool_action_inner` (3792-3860, after the `session_tool_audit_intents` insert, `req.tool`) for model-gateway's inline loop; both call `mark_external_content` when `is_external_content_tool` | [M] |
| `SC/migrations/0047_run_external_content.sql` | `ALTER TABLE runs ADD COLUMN IF NOT EXISTS external_content_at TIMESTAMPTZ;` plus `CREATE INDEX IF NOT EXISTS idx_runs_thread_external ON runs (thread_id) WHERE external_content_at IS NOT NULL;`. Number 0047 follows [D10]'s 0046; the runs receipt trigger of 0044 guards only the retention columns, `[INF]` to confirm | [NEW] |
| `SC/migrations/revert/0047_run_external_content.down.sql` | drops the index and the column | [NEW] |
| `SC/src/dreaming.rs` | `load_pending_messages` (1263-1300) selects an `external_content` flag per assistant message (an `EXISTS` over `runs` in the same thread whose `external_content_at` is set and whose `[created_at, COALESCE(ended_at, now()) + 60 s]` window holds `m.created_at`); `dream_once` (195-300) records `skipped_external_content` through `record_dream_run` for such messages, the same pattern as `skipped_conversation_scope` (223-231); `load_preceding_context` and `extraction_window` (390-413) drop those assistant rows so the extractor never reads them; user messages stay eligible | [M] |
| `.github/workflows/model-plane-rust-ci.yml` | the Postgres job (127-169) gains `external_content` in its filter (or a sibling job copying it) with the same minimum-count guard | [M] |

**API / data model.** One new nullable column `runs.external_content_at` (migration 0047, down file included), no proto change, no RPC. No wire change: the marker is derived from the tool name both seams already carry. No Data Plane change.

**Flags and rollout.** The execution-core refusal is always on (it only removes a write that is unsafe by construction). The Dreaming filter is always on; with `DREAMING_LLM_EXTRACTION` off it is nearly inert today (section 2.3). Rollback: revert both services; the column is harmless if left.

**Tests.**
- execution-core, `cargo test -p execution-core --lib -- external_content runtime_loop::agent save_memory`: `save_memory_refused_after_web_fetch_in_the_same_run`, `save_memory_refused_when_the_same_batch_has_web_search`, `save_memory_refused_after_browser_observe`, `save_memory_refused_after_mcp_tool`, `save_memory_allowed_after_org_internal_tools_only`, `failed_web_fetch_does_not_taint`, `delegated_child_taint_blocks_parent_save_memory`, `latch_is_rebuilt_from_a_resumed_transcript`, `recall_memory_is_unaffected`, `external_content_tool_list_is_pinned`, `zdr_run_still_refuses_with_the_zdr_message_first`.
- session-core unit, `cargo test -p session-core --bin session-core -- external_content`: `external_content_tool_list_is_pinned` (the same literal list as the execution-core test `external_content_tool_list_is_pinned`), `mcp_prefixed_tools_are_external`, `org_internal_tools_are_not_external`.
- session-core DB tests (`#[ignore]`, run by CI): `DATABASE_URL=postgres://session_core:session_core@localhost:5432/session_core_test REQUIRE_CONTRACT=1 cargo test -p session-core --bin session-core external_content -- --ignored --test-threads=1`: `external_content_marker_is_set_once_by_complete_step`, `external_content_marker_is_set_by_inline_reserve_tool_action`, `external_content_marker_reaches_the_parent_run`, `external_content_dreaming_skips_assistant_rows_in_the_window_and_ledgers_them`, `external_content_dreaming_keeps_user_rows_eligible`, `external_content_extractor_window_excludes_tainted_assistant_rows`, `external_content_zdr_run_stores_only_a_timestamp`, `external_content_thread_without_external_runs_is_unchanged`.

**Definition of done.** All of the above green; migration 0047 applies to an empty and to a populated database and reverts cleanly; the verified claims of section 2.3 are re-checked against the commit (the audit's `session_tool_audit_intents` derivation is replaced by the `CompleteStep` marker, deliberately).

**Deployment evidence.** Rebuild execution-core and session-core (migration 0047 runs at session-core start; `docker logs session-core` shows it applied). Flow 1: a chat run "search the web for <fixture topic> and remember the first result": the model attempts `save_memory`; the tool step shows the refusal text; `psql` on the session-core database: `SELECT external_content_at IS NOT NULL FROM runs WHERE id = '<run>'` is true and `SELECT count(*) FROM agent_memory WHERE ...` for that thread is unchanged. Flow 2: a new run in the same thread without web tools, "remember that I prefer short answers": `save_memory` succeeds and the row exists (user-authored memory is not collateral damage). Flow 3: compose override `DREAMING_LLM_EXTRACTION=1`; an assistant message paraphrasing the fixture page inside the tainted window: `dream_runs` holds `skipped_external_content` and `agent_memory` holds no row; baseline recorded first (before the slice the same setup writes an `accepted` row). Flow 4: a delegated child that calls `web_fetch` then the parent calls `save_memory`: refused. Flow 5 (inline loop): a chat turn answered by model-gateway's inline loop after `web_search` (no execution-core run) sets `external_content_at` on its run through `ReserveToolAction`, and with extraction on Dreaming ledgers its assistant message as `skipped_external_content`.

**Effort.** M, 3.5 engineer-days (execution-core latch and pre-pass 1.25, session-core marker at two seams, migration and Dreaming filter 1.5, CI and evidence 0.75); the audit's S understates it because the marker needs a migration, a second service and a CI job. **Risks.** In-flight diffs in `dreaming.rs` (216-222, 1278-1282) and `agent.rs`: land after Z-17 (0044, 0045) is committed or rebase; over-blocking on research-heavy threads (mitigation: the message tells the model what to do, user rows stay eligible, quarantine with promotion is the later step); a 60 s window grace is a placeholder that DB tests pin; the latch cannot protect a run whose transcript at resume lacks tags (documented limit).

### 4.8 Eval and test strategy, with the F-t2-044 epic outline

**Principle.** Three tiers, each with a different cost and job. Tier A gates every PR and ships inside S0 to S7; Tiers B and C are the F-t2-044 epic and the default-on evidence.

**The hermetic planner fixture (Tier A).** Two test-support pieces under `ECT/support/`, both in S0 and extended in S3 and S4:
- `ScriptedPlanner` implements `ActionPlanner`: a queue of replies or errors, plus a recording of every `PlannerContext` it received, so a test can assert what the model would have seen (the action log, the failure line, the remaining budget) without calling a model.
- `FakeQuarry` is a stateful axum server speaking the real `/v1/agent/runs` wire: pages with targets, a `generation` that advances on mutation, `409 TARGET_REPAIR_REQUIRED` for stale refs, `403` off-domain, scripted `402`, `429`, `504`, history for `back`, async content for `wait_for`. It is driven by the real `QuarryAgentClient::new(base_url, token)`, so the loop, the client, the typed-error parsing and the wire mapping are all the production code.
- Recorded replies (`ECT/fixtures/planner/replies/*.json`) cover the model-output side: well-formed, fenced, prose-wrapped, truncated, wrong-enum. Prompt goldens (`ECT/fixtures/planner/prompts/*.txt`) pin the rendered prompt, are content-free by construction and fail on any unreviewed change (regenerate with `UPDATE_GOLDENS=1`).
- It needs no network, no Chromium and no credentials, runs inside `cargo test --workspace`, and is the same harness the later slices append scenarios to. Corpus v0 is listed in S3; S4 adds seven scenarios.

**Tier B (F-t2-044 phase 1 to 3, labelled or scheduled workflow, Chromium).** The same scenarios against a real `quarry-edge` with the real driver and the `TestSite` fixtures (`Q/crates/quarry-runtime/src/test_site.rs`, no consumer today), a stub grant validator, and recorded planner replies replayed at the inference boundary; scores task success, steps and planner tokens; adds prompt-injection pages asserting on the screening verdict and on "no off-policy action reached Quarry", and ZDR no-write assertions. This is the audit's "make `browser_agent_e2e` hermetic and scored".

**Tier C (live, scheduled and on demand, spend-capped).** The existing live probes plus corpus runs with real inference (`verevon-balance`, and the Terra route if OD13 pins it): task success, median steps, planner tokens, policy violations (must be zero), injection followed (must be zero), routing compliance (S6 probe). Results are written per release with the commit id.

**Default-on evidence gate (proposed thresholds, `[placeholder, ratify]`).** Tier A green; Tier B green on the release candidate; Tier C on a 12-case corpus: at least 80 percent task success, median at most 12 steps, zero policy violations, zero followed injections, routing compliance at least 90 percent on connector-solvable prompts. The corpus categories: navigate and extract, form fill with approval, stale ref, cookie banner overlay, select, press Enter search, back, async wait, injection page, login wall (stop `blocked`), challenge page (stop `blocked`), off-allowlist link, token budget exhaustion with partial answer.

**F-t2-044 epic outline (W3, L; about 12 engineer-days beyond Tier A; owners Ingestion lab/evals and Model Plane harness).**

| Phase | Content | Effort | Depends on |
|---|---|---|---|
| P0 | a CI job running the existing `lab/evals` binaries (`quarry-eval`, `bakeoff`, `release_matrix`) with the commit id (`.github/workflows/quarry-v2-ci.yml` runs `cargo test` only today) | S, 1 d | none |
| P1 | Tier B loop against `quarry-edge` plus `TestSite`, stub grant validator, record and replay of planner replies; scoring of success, steps, tokens | M, 5 d | S0 (the seam), F-t2-001 or the dev Quarry configuration, F-t2-008 for cost scoring |
| P2 | injection pages and ZDR no-write assertions in the fixtures | S, 2 d | P1 |
| P3 | per-release scoreboard publication, label and scheduled triggers, Chromium job isolation (the audit notes documented CI instability) | S, 2 d | P1 |
| P4 | scheduled live Tier C with a spend cap and the 12-case corpus | S, 2 d | S2 (flag), a CI inference credential |
| dropped | mirror-and-diff on canary traffic (the card says drop it) | | |
