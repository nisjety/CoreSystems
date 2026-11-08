
## 5. Dependencies and ordering

**Order inside this workstream.** S0 first (seam, harness, F12). Then S1; S2 can run in parallel with S1 (it needs only S1's `fetch_org_quota_keys`, which can land with S2 if S2 goes first). S3 needs S0 and S1. S4 needs S3. S5 needs S2 and, for stage 3, everything in D10. S6 and S7 are independent of S1 to S5 (S7 needs S0's F12 arm, or carries it itself). One engineer, critical path S0, S1, S3, S4, S5: 20 working days; with a second engineer taking S2, S6 and S7 in parallel, about 15 to 16 working days.

| Dependency | Kind | On what | What it blocks |
|---|---|---|---|
| F-t2-001 production grants ([D1]) | **hard** for any production exposure | production Quarry refuses runs without a grant (`Q/crates/quarry-edge/src/main.rs:182-200`); the delegated-bearer path is what gives execution-core the inference and browser bearers | D10 stages 2 and 3; S0 to S5 can be built and evidenced on the dev configuration |
| F-t2-008 budgets ([D6] S1 `on`, S4) | **hard** for stage 3, soft for stage 2 | Quarry-side enforcement and the budget claim in the grant | the audit gate "budgets before default-on"; S1 here is the execution-core half |
| F-t2-021 capture-time redaction ([D12] S1 to S4) | **hard** for stage 3 | masking at Quarry and `mp-scrub` in the planner prompt | the audit gate "raw observation text enters the prompt"; S0 here reduces, does not close it |
| F-t2-015 actionability and trusted input | soft | success rate, not safety | none |
| F-t2-022 claim channel ([D7]) and ADR topic 1 | soft | final carrier of the org flag | replacement of the interim quota row; nothing here waits for it |
| [D6] S1 and S4 | soft (evidence) | the 402 and the budget claim | S1 evidence step 3 |
| [D8] S8.6, [D3] S9, [D12] S4, [D7] S10 and S11, [D10] | **ordering**, same files | see hotspots | merge order, not behaviour |
| session-core migrations 0044 and 0045 committed (Z-17) | **hard** for S7 | `SC/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` are `[U]`; `dreaming.rs` hunks 216-222 | S7 merge |
| Owner decision OD13 | **hard** for stage 3 | planner route and default | S5 stage 3 |
| Control Plane sign-off on the quota-key convention | **hard** for production use of the org flag and the org token ceiling | section 9 | stage 2 in production orgs (allowlist works without it) |

**Merge-conflict hotspots.**

| File | Lines | Shared with | Proposed handling |
|---|---|---|---|
| `EC/browser_agent.rs` [C] | `PlanConfig` 243-292, `BrowserObservation` 171-186, loop 621-786, `decide_next_action` 809-848 | [D8] S8.6 (attach mode, `USER_TOOK_OVER` pause), [D3] S9 (`challenge` field), [D6] (cost limit), [D2] follow-ups | one owner or strictly sequential PRs: D11 S0, S1, S3 first (they restructure the loop and add `StepFailureClass`), then [D3] and [D8] rebase; [D8]'s `PausedByUser` becomes one more arm of the classification |
| `EC/quarry_agent.rs` [C] | `AgentClientError` 480-490, `action_to_wire` 346-402, `observation_from_wire` 407-474, `StepRequest` 612-630 | [D8] (typed variant, `attach_run`, `initiated_by`), [D3] (`challenge` parse), [D12] S4 (redaction stamp), [D10] (trace fields) | this design adds helpers (`parse_typed_error`, `classify_quarry_error`) and no enum change; the others stay additive |
| `EC/llm_planner.rs` [C] | schemas 53-101, `planner_infer_request` 200-267, `from_env` 129-154 | [D12] S4 (scrub at the prompt boundary), [D3] (challenge line and rule), [D7] S10 (allowed-capabilities list) | S0 creates `planner_view` and `procedure` modules so the others add lines there, not in the function body |
| `EC/tool_bridge/mod.rs` [C] | `execute_browser_agent` 165-232, `BrowserAgentInput` and `browser_plan_config` 860-892 | [D6] S4 (effective budget values), [D1] (grant), [D8] | additive input fields; one `BrowserAgentSettings` struct |
| `MG/browser_run.rs` [M] | `browser_run_start` 297-428 | [D1] (derive-grant call near 337-348), [D6] S4 (`mint_budget`), [D8] (attach field), the uncommitted ZDR hunks (307-313, 628-632, 663-669, 995, 1328) | one helper that mints every run parameter (`max_steps`, `max_runtime_s`, `max_cost_usd`, `max_planner_tokens`, `planner_flag`); one editor at a time; rebase on the ZDR hunks first |
| `MG/org_quota.rs` [C] | new functions only | [D6] S4 uses `fetch_org_limits` | additive |
| `EC/runtime_loop/agent.rs` [M] | prompt consts and `compose_system_prompt` 208-248, `LoopContext` 362-430, 917, pre-pass 1801-1826, phase 3 2051-2075, 3114, `record_tool_step` 4368-4400 | in-flight hunks at 571, 599, 749, 769, 1937, 4566 and the test hunks from 7185; sibling (b) chat work | S6 and S7 are small and local; rebase, no textual overlap found |
| `SC/src/dreaming.rs` [M] | `dream_once` 195-300, `extraction_window` 390-413, `load_pending_messages` 1263-1300 | in-flight hunks 216-222 (`thread_has_waived_run`) and 1278-1282 | land after Z-17, or rebase onto it |
| `SC/src/grpc.rs` [M] | `enqueue_tool_action_audit` about 3664-3709 | [D10] S6 (trace events), in-flight hunks 3218-3229 and 4101-4160 | rebase |
| `SC/migrations/` | `0047_run_external_content.sql` | [D10] `0046_run_trace_events.sql` | numbers are assigned at merge; renumber if [D10] slips |
| `MP/deploy/docker-compose.yml` [C] | execution-core env about 1236-1262, line 1189 | every Model Plane slice | append-only env block, one editor at a time (the CHAT_GAP plan names this file an additive seam) |
| `EC/provenance.rs` [C] | `TrustClass::classify` 53-67 | F-t2-021, F-t2-049 | one match arm |
| `.github/workflows/model-plane-rust-ci.yml` [M] | Postgres job 127-169 | the uncommitted receipts job | rebase |
| `ECT/browser_agent_e2e.rs` [C] | loop call about 76, literals 57-75 and 138-156 | [D8] (attach scenario) | additive |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- **Control Plane owns identity, orgs, quotas, entitlements.** The org flag and the planner token ceiling are org-core quota rows written by an org admin through org-core's existing route; the Model Plane only reads them with a service credential (the pattern of `fetch_org_limits`). No Control Plane code or schema change. At risk: a flag stored as a quota row blurs "quota" and "entitlement"; mitigated by the interim label and its removal when F-t2-022's carrier lands (D9).
- **Data Plane owns documents, chunks, embeddings, retrieval, graph, wiki.** Untouched. No agent-derived graph write path is added or guarded (the card's Data Plane clause is dropped because none exists).
- **Ingestion Plane (Quarry) captures evidence and persists through Data Plane contracts.** No Quarry change in any slice. Two companion requests for the Ingestion owner (key validation for `press`, destination check for `back`) are not dependencies.
- **Model Plane owns reasoning, runs, inference, execution loops, capabilities, browser grants, cost.** Satisfied: the planner, the meter, the procedure, the failure classification, the org-flag consumption and the taint latch are execution-core and session-core. cost-core is not written by this design (D3); the token meter is an in-process cap, not a ledger.
- **Application and Frontend Planes.** Untouched. An optional request field `max_planner_tokens` exists on the gateway's browser-run endpoint but the SPA does not send it.
- **Channel Plane.** No runtime is built or assumed.
- **No direct database crossing between planes.** Satisfied: execution-core reaches session-core through gRPC (the existing `CompleteStep` step prefix, no new field), model-gateway reaches org-core over HTTP with a service credential, session-core's new column is in its own database.
- **No independent embeddings or reranking.** None added.
- **The Model proposes browser actions; Quarry executes or rejects.** Satisfied for every verb: the planner emits a proposal, execution-core's admission and risk gate only tighten, Quarry validates grant, domains, refs and budgets and may refuse, and its refusal is terminal and non-negotiable in the loop (`policy_denied` and 403 are never retried or rephrased). **Two verbs are at risk until the Ingestion companion lands**: Quarry does not validate `press` keys and checks the destination of `back` only after it ran (F13). The execution-core allowlist and history guard cover them; if either is bypassed the domain fence and the post-action 403 still hold, at the cost of the run. Excluded verbs (`evaluate`, semantic, coordinates, upload, download, dialog) stay unreachable from the planner.
- **Zero Data Retention.** (1) The org flag cannot apply to ZDR: `PlannerPolicy::resolve` never selects the planner for a ZDR run, and model-gateway already refuses durable browser runs under ZDR (`MG/browser_run.rs:307-315`). (2) Defence in depth: `InferRequest.zdr` follows the plan's flag, and the planner forwards the org metadata its doc promised (F7). (3) The usage line, metrics labels and planner-view log line are counts and codes only; no URL, text, title or org id reaches a log, a metric label or a durable store. The action log lives in memory for the life of the run and is never persisted or checkpointed. (4) S7's marker is a timestamp on the run row, derived from a tool name both seams already carry; it names no content. (5) A ZDR run never reaches the memory tools anyway (`memory_tool_preflight` refuses first, and a test pins that order). Remaining risk: a future caller that builds a `browser_agent` `tool_input` outside model-gateway; it inherits the same policy because the decision is made inside execution-core from verified inputs.
- **Strengths protected (audit section 8).** Risk gates stay in code and are extended rather than moved to the prompt; provenance screening is reused, not duplicated; one-shot sensitive approvals are untouched; the domain fence and the broker-owned domain policy are unchanged; ZDR refusals are not loosened.

## 7. Cross-session interfaces

**(a) Verevon Space.**
- *Shared piece: the browser run parameters.* A Space agent's browser run goes through the same `browser_agent` tool and therefore the same `PlannerPolicy`, ceilings and token budget. Proposed owner: model-gateway's single run-parameter helper (S1, S2, with [D6] S4's `mint_budget`); Space must not build its own `tool_input`.
- *Shared piece: content-free run telemetry.* The usage line and the metric labels are safe for ZDR-aware Space telemetry; Space may display the usage line as is.
- *Shared piece: memory taint on Space threads.* Messages carry a Space audience (`SC/migrations/0025_message_space_audience_context.sql`), and Dreaming reads the same `messages`. S7's window filter applies to Space threads unchanged. The user must reconcile who may later promote a quarantined memory in a Space (the owner of the Space or the author), because skip-only defers that question.
- *Shared piece: the `done` answer.* Whether a browsing run's answer is written into the Space thread or kept in a panel is part of OD13; S3 returns it as the tool result only.

**(b) Model Plane and chat/agents.**
- *Shared files.* `EC/browser_agent.rs`, `EC/quarry_agent.rs`, `EC/llm_planner.rs`, `EC/tool_bridge/mod.rs` and `MG/browser_run.rs` are edited by this design, [D3], [D6], [D7], [D8] and F-t2-001. Proposed owner: one engineer from sibling (b) as the single editor of the execution-core browser files, taking S0, S1 and S3 first.
- *Shared piece: Quarry error classification.* `parse_typed_error` and `StepFailureClass` serve [D6] (402), [D7] (`policy_denied`), [D3] (`CHALLENGE_DETECTED`), [D8] (`USER_TOOK_OVER`). Proposed owner: this design's S1 and S3; the others add arms.
- *Shared piece: the run-parameter helper in model-gateway,* as above, and the combined cost cap: [D6] DEC-9 assumed a combined USD cap in execution-core; this design defers it (D3) because planner spend is not recorded per run in cost-core (F8). The user must reconcile this with [D6].
- *Delegated bearers.* The effective run length follows the inference bearer lifetime (D2). Longer runs need a refresh path owned by [D1]/sibling (b); the `ValidatedBrowserGrant` expiry accessor is `[UNVERIFIED]`.
- *Approvals (F-t2-033).* A risky `press` or `select` uses the existing approval machinery (`gate_risky_action`); no new approval UX is designed here.
- *Chat loop.* S6 touches `compose_system_prompt` and phase 3 of `run_rounds`; any other change to those regions (subagent snippets, memory snippets) needs the same rebase.
- *What the user must reconcile:* the single owner of the execution-core browser files; the 402 and 429 handling split between this design and [D6]/[D8]; the owner decision OD13; the quota-key convention with Control Plane.

## 8. What can proceed WITHOUT an ADR or decision, and what is blocked

**Proceed now (no ADR, no owner decision):** S0 (seam, hygiene, F12, harness); S1 (budgets; the numbers are placeholders, adjustable by env); S3 (failure semantics, procedure, answer); S4 (verbs and gate); S6 (routing guidance); S7 (memory guard; sequence after Z-17 is committed); S2 as code and for dev and staging through the env allowlist.

**Blocked, and by what:**

| Work | Blocked by |
|---|---|
| S2 org flag in production orgs, and the org token ceiling | Control Plane agreement on the quota-key convention (section 9 question 2); the final carrier is ADR topic 1 but does not block the interim |
| D10 stage 2 in production | F-t2-001 production grants; [D6] S1 `on`; S3 and S4 merged; hermetic corpus green |
| D10 stage 3 (`default_on`) | F-t2-001, F-t2-008 ([D6] S1 and S4), F-t2-021 ([D12] S1 to S4), F-t2-044 Tier B, live Tier C floor, owner decision OD13 |
| Raising `max_runtime_s` beyond about 240 s | delegated-bearer refresh in execution-core ([D1]/sibling (b)) |
| Pending-review quarantine for memory (the later half of F-t2-045) | a new accept/reject RPC and UI; the memory-owner promotion rule (not designed here) |
| Any semantic, coordinate or vision rung of the ladder | F-t2-015, F-t2-034 and screenshot-to-model policy |

## 9. Open questions for the user (only real blockers)

1. **OD13: the planner route and the default.** Should the durable planner stay on the default Balance route, or pin the ChatGPT subscription with a route guard (the CHAT_GAP plan says E5 must pin Terra before the default flips), and may the answer be written into the thread? Blocks stage 3 only. Recommendation: keep Balance until the Tier C numbers exist.
2. **Control Plane: may org-core quota rows `browser_agent_llm_planner` (1 on, 2 off) and `browser_agent_max_tokens_per_run` carry the interim org flag and token ceiling?** The alternative is the env allowlist only (no production per-org control). Blocks production use of S2.
3. **Run length.** Is a browser run of at most about 4 minutes acceptable until delegated-bearer refresh exists, and who owns that refresh? Blocks raising the defaults, nothing in S0 to S5.
4. **Single editor.** Who is the single editor of the execution-core browser files across this design, [D3], [D6], [D7], [D8]? Blocks scheduling parallel work.

## 10. INTERFACE SUMMARY (for the integrating architect)

**Files touched.** *Model Plane Rust, execution-core:* `browser_agent.rs`, `quarry_agent.rs`, `llm_planner.rs`, `tool_bridge/mod.rs`, `run_budget.rs`, `provenance.rs`, `runtime_loop/agent.rs`, `browser_agent/browser_risk.rs`; new `planner_policy.rs`, `external_content.rs`, `browser_agent/{planner_view,procedure,failure,admission}.rs`; tests `tests/browser_planner_hermetic.rs`, `tests/support/*`, `tests/routing_probe.rs`, fixtures. *model-gateway:* `browser_run.rs`, `org_quota.rs`. *session-core:* `grpc.rs`, `main.rs`, `dreaming.rs`, new `external_content.rs`, migration `0047_run_external_content.sql` (+ revert). *Deploy and CI:* `deploy/docker-compose.yml` (env), `model-plane-rust-ci.yml`, new rollout runbook, ledger entry. No Quarry, Control Plane, Data Plane, Application or Frontend change.

**New contracts.** `tool_input.max_planner_tokens` and `planner_flag`; optional request field `max_planner_tokens`; org-core quota keys `browser_agent_llm_planner` (1 on, 2 off) and `browser_agent_max_tokens_per_run`; `runs.external_content_at`; usage line (counts only); metrics `execution_browser_*`, `execution_routing_first_tool_total`. Flags: `QUARRY_BROWSER_AGENT_PLANNER_MODE` (`off` default), `_LLM_ORGS`, `_PROCEDURE` (`browse/1`, `legacy/0`), ceilings, failure limits, `_DEFAULT_ON_ACK`; the legacy `QUARRY_BROWSER_AGENT_LLM` is retired at stage 4. Planner schema gains `press`, `select`, `back`, `wait_for`, `key`, `timeout_ms`, `answer`, `outcome`.

**Dependencies.** Hard: F-t2-001 (production), F-t2-008 and F-t2-021 (stage 3), session-core 0044 and 0045 committed (S7), OD13 (stage 3), Control Plane sign-off on quota keys (production flag). Soft: F-t2-015, F-t2-022, ADR topic 1.

**Effort.** S0 2 d, S1 4 d, S2 3 d, S3 5 d, S4 7 d, S5 2 d, S6 3 d, S7 3.5 d: F-t2-010 about 26 d (audit M understated; L), F-t2-045 skip-only 3.5 d. F-t2-044 epic about 12 d beyond Tier A.

**ADR gates.** Topic 1 (final carrier of the flag; interim not blocked). No other audit topic; OD13 is an owner decision.

**Parallelisable with.** S1 and S2 after S0; S6 and S7 with anything; S4 after S3.

**Conflicts with.** [D8] and [D3] and [D6] in `browser_agent.rs`, `quarry_agent.rs`, `browser_run.rs`; [D12] S4 in `llm_planner.rs`; [D10] in session-core `grpc.rs` and migration numbering; in-flight uncommitted `dreaming.rs`, `agent.rs`, `browser_run.rs`.

**Key findings.** Step and time budgets already exist, the token budget does not; one failed Quarry action ends the whole run; planner errors silently degrade to an observe loop; `done` drops the answer; `browser.observe`/`browser.act` are never screened; the audit's `session_tool_audit_intents` derivation of taint is wrong for execution-core runs.

<!-- COMPLETE -->
