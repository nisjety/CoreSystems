# Wave 1 register triage, 2026-10-27

Every open Wave 1 (`D1-*`) and cross-wave (`D0-*`) row of `docs/plans/model-plane-depth-register.md` was read against the tree at `model-plane` HEAD by a Sonnet classifier (21 batches of about 30 rows), and every proposed closure was then re-checked by an Opus verifier told to refute it and to keep the row open when unsure. Only confirmed closures were written to the register; a rejected closure keeps its row open under the verifier's category. Read-only agents: no code, stack or env file was touched. Wave 2 (`D2-*`) rows were not triaged.

Rows triaged: 608 (one row, D1-542, came back unclassified and stays open). Closed: 64 fixed, 113 duplicates, 5 stale (182 in all). Proposed closures the verifier rejected: 24.

| Still open | Rows | Of which block Wave 1 |
|---|---|---|
| live-proof-needed | 153 | 73 |
| code-fix | 78 | 5 |
| owner-decision | 61 | 4 |
| still-open | 54 | 1 |
| pointer | 80 | 0 |

`pointer` rows only point to more notes in `.plan-work/execution/results/*.json`; they were not opened.

## Owner decisions (the question each row needs answered)

| Row | Slice | P | Blocks W1 | Effort | Next step |
|---|---|---|---|---|---|
| D1-003 | WS12-S1 | P1 | yes | S | Owner: may the integration branch be pushed so CI runs are recorded? |
| D1-011 | WS05-S2 | P1 | yes | S | Owner and security: approve per-server annotation_trust and the HMAC observation-write route before the switch goes beyond dev? |
| D1-042 | WS01-S2 | P1 | yes | S | Do you accept that every org member can read the 4,992 replayed tool_action audit rows via GET /api/v1/audit, or should the replay wait for the Space role gate (F-t1-031)? |
| D1-482 | WS04-S7 | P1 | yes | M | Before the idempotency switch: buffer `connected` before the claim returns, or require MODEL_GATEWAY_RUN_CONTROL_BUS for MODEL_GATEWAY_SHARED_IDEMPOTENCY? |
| D1-077 | WS10-S0 | P1 |  | S | Keep DEC-13 'unknown = enforce' (and document the rollback trap), or switch to a parser that ignores unknown values and keeps the default off? |
| D1-079 | WS10-S0 | P1 |  | S | Do you ratify DEC-13 (a): org-core stores, inference-core enforces? |
| D1-097 | WS12-S10 | P1 |  | M | Widen the log-content denylist (question, query, turn_question first) at the cost of reviewing about 50 more sites, or keep the current names? |
| D1-099 | WS12-S10 | P1 |  | S | Accept the path-plus-macro-hash baseline key in place of a file hash? |
| D1-112 | WS01-S4 | P1 |  | M | Accept loss of pre-cutover application audit events (dev stack) or hand-replay the up-to-72h backlog into the new stream? |
| D1-121 | WS10-S2 | P1 |  | S | Confirm: with no org policy, should a non-member resolve to state none (unrestricted) rather than nothing, before enforce is switched on? |
| D1-157 | WS12-S11 | P1 |  | S | Is there a reviewed exception for raising a file-size ceiling, and how is it recorded? |
| D1-163 | WS12-S11 | P1 |  | S | Turn on MODEL_PLANE_CLIPPY_RATCHET and set MODEL_PLANE_CLIPPY_WARNING_CEILING from a measured run, or leave it off? |
| D1-177 | WS05-S5 | P1 |  | M | Should ordinary members get a self-scope write (grant capability:write, or a distinct personal-policy scope) so a person can set a personal tool policy? |
| D1-186 | WS06-S7 | P1 |  | S | On org erasure, must organization_siem_audit_outbox rows (org_id, actor id, destination host) be purged or kept as content-free audit? |
| D1-212 | WS12-S9 | P1 |  | M | For each of the 14 plane-boundary violations: fix it, or record a reviewed exception with a reason, so the gate can block? |
| D1-265 | WS06-S1 | P1 |  | S | Which final ADR number does the integrator assign to the audit-integrity ADR (keep 0013 or rename file and title)? |
| D1-268 | WS06-S1 | P1 |  | S | Which ADR number does the integrator assign to the audit integrity ADR (keep 0013 or renumber)? |
| D1-292 | WS06-S2 | P1 |  | M | Before AUDIT_CHAIN_ENABLED goes on in production: stop-the-world image swap or an activation grace window? |
| D1-307 | WS01-S12.code | P1 |  | M | Who runs the signing key ceremony and which keys (dev or production) are used for the S12 evidence? |
| D1-327 | WS01-S13.code | P1 |  | L | Owner: will you obtain the written Microsoft or Anthropic ZDR contract and name a reviewer so Z-40 can be attested? |
| D1-366 | WS02-S8.code | P1 |  | S | Which staging provider (openai or azure-openai) and model alias should the pinned route serve for the stable-prefix A/B? |
| D1-374 | WS01-S14.code | P1 |  | S | Has counsel approved PendingItem d adoption and the wording for trust rows 36 to 41? |
| D1-399 | WS06-S8 | P1 |  | M | Owner: enforce the details guard (reject content keys), or run the golden content-free check on live rows, before AUDIT_SIEM_EXPORT_ENABLED is turned on? |
| D1-422 | WS04-S3 | P1 |  | M | Owner/integrator: what shape should an operation-level matrix row take, so the ZDR-token queue-input refusal can be pinned in the matrix and vendored copy? |
| D1-426 | WS06-S9 | P1 |  | M | Owner: what is the policy and repair procedure for a head marked broken (manual reset, re-anchor, or tombstone) before the chain is switched on? |
| D1-429 | WS06-S9 | P1 |  | M | Owner: ratify DEC-35 / Q-7a1-15 so SIEM export is an enterprise entitlement, and in which plan? |
| D1-431 | WS06-S9 | P1 |  | S | Counsel: confirm the SIEM destination class (user-authorised, customer-directed, EXCLUDED) before subprocessors and trust-recipients rows are applied. |
| D1-435 | WS08-S1 | P1 |  | S | Owner: gate only image attachments on subscription/Do/ZDR routes (inlined document text allowed), or refuse all attachments? |
| D1-464 | WS04-S6 | P1 |  | M | When the queue refuses (429/503) after persist, record the row as withdrawn (needs WS17 marker) or accept the stored undelivered row? |
| D1-475 | WS10-S3 | P1 |  | M | Should model entitlement also cover vision/image models and non-chat RPCs used for chat-like analysis? |
| D1-498 | WS10-S4 | P1 |  | S | Should enforce mode fail closed at the gateway on any GetModelAccess error, accepting more 503s, to avoid thread/message/run rows for refused turns? |
| D1-508 | WS08-S2.code | P1 |  | S | Should URL-only (no bytes) image attachments stay allowed, and if so behind which host allowlist? |
| D1-528 | WS07-S1 | P1 |  | S | Confirm MEMORY_CONTROLS_ENABLED stays default ON (opt-out kill switch) despite the general default-off switch rule? |
| D1-545 | WS03-S8 | P1 |  | S | Set SESSION_CORE_PARK_TTL_SECONDS shorter or add a fixed thread notice before flip (4)? |
| D1-546 | WS03-S8 | P1 |  | S | Approve S8-B crash-park design and go-ahead, or order an independent review first? |
| D1-558 | WS07-S2 | P1 |  | S | Record in the ledger that ResetMemory needs no zdr-posture-matrix row (loaders reject unknown fields)? |
| D1-571 | WS07-S6 | P1 |  | M | Which ThreadSummary tag numbers may hold memory_mode, given WS17 reserves 16-19? |
| D1-589 | WS10-S7.code | P1 |  | L | Do you approve the shadow soak and the enforce flip, and on which date? |
| D1-591 | WS10-S7.code | P1 |  | S | none |
| D1-598 | WS07-S4 | P1 |  | S | Should Model Plane session-core get its own org-core principal and token, or reuse the Control Plane session-core principal? |
| D1-601 | WS07-S4 | P1 |  | M | Before enabling ORG_POLICY_FAIL_CLOSED, accept the poll-lag Unavailable window or add a gate pull-through? |
| D1-639 | WS08-S4.code | P1 |  | S | Accept that zdr_feature_table.rs pinning image/ZDR refusal is enough, or require Ask-image rows in zdr_posture_matrix.rs and zdr-posture-matrix.json? |
| D1-678 | WS02-S12 | P1 |  | L | Which producer (WS14, WS24 or a WS03 todo source) should supply plan todos, or should the recitation stay parked? |
| D1-687 | WS04-S8 | P1 |  | M | Do you record the ledger reversal of persist-before-deliver and approve the WS17 withdrawn marker so retract and edit can be switched on? |
| D1-691 | WS04-S8 | P1 |  | S | On queued-message edit: retract then re-queue under a new client message id, or move text back into the composer? |
| D1-696 | WS09-S4 | P1 |  | S | Sign D5 numeric trigger for VITE_CHAT_DELTA_FRAME_BATCH after a bursty-fixture onMessage-per-frame count is measured? |
| D1-705 | WS09-S7 | P1 |  | S | Sign the C8 numeric trigger (p95 gap <=50ms, DOM <=30000, mount <=1500ms, heap <=150MB) once the three-run S1 baseline exists? |
| D1-714 | WS09-S10 | P1 |  | S | Sign D5 numeric triggers for turning VITE_CHAT_VIRTUALIZE on, after S1 numbers on 300/1000-turn fixtures show budget still fails? |
| D1-716 | WS09-S10 | P1 |  | M | Is an in-app transcript find required before VITE_CHAT_VIRTUALIZE may be turned on? |
| D1-727 | WS11-S9 | P1 |  | S | May one probe call per Codex model with xhigh be run on the owner's subscription to fix the per-model ladder? |
| D1-742 | WS09-S6 | P1 |  | S | Does the owner sign D5 (build/enable S6 parallel load) before the S1 three-run baseline exists, or wait for the measurement? |
| D1-758 | WS09-S9 | P1 |  | S | Does the owner want S8/S9 paging enabled before the WS09-S1 long-thread measurement exists, or only after it? |
| D1-764 | WS09-S9 | P1 |  | S | Does the owner accept the S1 long-thread measurement against D5 (plus Q1/Q7) so the S8/S9 flag can be switched on? |
| D1-773 | WS09-S8 | P1 |  | S | Does the owner sign off DEC-29 (cursor = message id resolved under the audience ceiling) and DEC-39 (proto tags 6, 7, 2)? |
| D1-783 | WS02-S10.code | P1 |  | S | Keep Azure and UnifiedAzureAi off until staging confirms they accept prompt_cache_key; confirm that stance as the D7 decision? |
| D1-109 | WS12-S8 | P2 |  | S | Keep strict (WS02/WS15 add the dispatchEvent case in the same change) or allow a reviewed exception list for new golden events? |
| D1-154 | WS01-S10 | P2 |  | S | Should S12 sign the rollback baseline in release mode, or may the rehearsal skip compose config for a non-release baseline? |
| D1-271 | WS06-S1 | P2 |  | S | Confirm the integration branch names the workflow push trigger should list before pushing. |
| D1-458 | WS10-S1 | P2 |  | M | Should the entitlement error frame carry reason and model id so the composer can show a per-reason sentence? |
| D1-801 | WS04-S3 | P2 |  | S | Must the plan-grant event be written to session-core's events table or the audit chain, given runs.mode and threads.granted_rung already record the grant durably? |
| D1-792 | WS11-S1 | P3 |  | S | Grant the finetune-poller credential in auth-core's registry, or turn the poller off where fine-tuning is not deployed: which? |

## Code or test fixes (files and a fix sketch)

| Row | Slice | P | Blocks W1 | Effort | Next step |
|---|---|---|---|---|---|
| D1-007 | WS05-S2, S4 | P1 | yes | M | Add both goldens to zdr-posture-matrix.json, regenerate .sha256 and vendored copies, update pin in zdr-contract-sync.test.sh, run it. |
| D1-115 | WS01-S4 | P1 | yes | S | Remove ,"optional":true from the application entry at line 804 and update the comment above audit-extra-nats-provisioner. |
| D1-134 | WS10-S2 | P1 | yes | M | Add a go test ./internal/modelentitlement step with the disposable DB url to apps/Control Plane/scripts/run-isolated-control-lifecycle-e2e.sh, then run it. |
| D1-229 | WS04-S2 | P1 | yes | M | sse.rs agentic_run_stream: cancels.register_run(request,org,user,run_id) + state.run_control.attach(...), finish() on end; add a cross-replica Stop test for a Do run. |
| D1-386 | WS06-S5 | P1 | yes | M | Add a DB-harness test in audit-core internal/store running events, manifest and keys against seeded chain rows (range, chain_seq paging, filters). |
| D0-010 | WS20-S5 | P1 |  | L | Add digest recompute and expiry check to scheduled_step_decision.rs, session-core ResolveScheduledStepAuthority contract fields, and user-core step decision; add the two rejects tests. |
| D0-011 | WS21-S8 | P1 |  | L | Wire ApplySpaceFloor into getBrowserSitesInternal (space_id, blank 400), add browser_entitled to user-core EffectPolicy, matrix golden space_floor rows, gateway spaces.rs, and space_id in the broker cache key. |
| D0-014 | WS20-S6 | P1 |  | L | Add zdr/waiver tags to ScheduledStepContext, serve them from session-core (and contract_json as exact text), build the caller from them in grpc.rs, add the scheduled_do_run matrix row and pin; decide Complete vs FailRunActivity. Flag stays off. |
| D0-019 | WS23-S3.code | P1 |  | M | Add CauseCode::TaintedEgress ('tainted_egress') to approval_cause.rs and api/contracts/approval-causes.json, update golden and the unknown-token test, switch agent.rs to it with a held-outcome test. |
| D0-022 | WS13-S3 | P1 |  | L | Project allowlisted cause_code, rule_id, scope, decision_source, step_up_verified in sse.rs and refresh the golden; add Approval.decision tag 17 with ApprovalDecisionRecord; build WS13-D10 envelope; then run DB tests live. |
| D1-034 | WS11-S7 | P1 |  | S | Add one line: per-role/per-org effort cap is Control-owned (WS10); inference-core enforces via effort::clamp_to_cap(level, cap), cap None until WS10. |
| D1-064 | WS12-S7.code | P1 |  | S | Add a sse-event-payloads.json row to api/contracts/README.md with the MP_UPDATE_GOLDEN regen command and the 3 follow-up pins. |
| D1-078 | WS10-S0 | P1 |  | S | Apply the 3 shared doc edits (master matrix rule 19 and inference row, capability matrix row and 4.6, ledger top entry plus the ADR-0005 draft), keeping CRLF; rerun test for 50/50. |
| D1-100 | WS12-S10 | P1 |  | S | Review the two sites (they log counts and fact fields, not turn text) and run log-content.py --update with review reasons, or rename the fields. |
| D1-127 | WS10-S2 | P1 |  | S | Add the README row and a model-entitlement-matrix.json.sha256 pin in apps/Model Plane/api/contracts. |
| D1-136 | WS10-S2 | P1 |  | S | Add the README row and a .sha256 for model-entitlement-matrix.json in apps/Model Plane/api/contracts and check it in model-entitlement-contract-sync.test.sh. |
| D1-162 | WS12-S11 | P1 |  | S | Create .github/CODEOWNERS with entries for scripts/gates/ and .github/workflows/repo-gates.yml per DEC-39 D2. |
| D1-176 | WS05-S5 | P1 |  | S | Mint an admin-matching scope (e.g. capability:admin) for owner/admin in plane-token-scopes.ts and update its spec, or change verifiedIsAdmin; then live-check an org write. |
| D1-219 | WS01-S7.code | P1 |  | M | Add org-filtered counter queries to zdr-live-proof.sh once the org columns are confirmed, or accept the quiet-stack rule. |
| D1-234 | WS04-S2 | P1 |  | S | apps/Model Plane/docs/decisions/ledger.md: add the WS04-S2 entry and correct the S1 'Not done' note on /readyz and the ignored NATS tests. |
| D1-238 | WS01-S11 | P1 |  | L | Add a script that queries Prometheus for the TSV metrics over a 7-day window, compares to targets, and wires into coresystem-conformance.sh; needs approved targets. |
| D1-242 | WS01-S11 | P1 |  | S | Optional: pass --profile runtime to coresystem-conformance.sh in coresystem-conformance-artifact.sh; otherwise set CORESYSTEM_CONFORMANCE_PROFILE=runtime. |
| D1-310 | WS06-S3 | P1 |  | M | Add a retention step in internal/store (keep newest attestation per day beyond N days), tested in purge_chain_test.go, or record a decision to defer. |
| D1-332 | WS01-S13.code | P1 |  | S | Add a pass-2 preflight (attested route listed in /api/v1/models) to scripts/tests/zdr-live-proof.sh. |
| D1-370 | WS02-S8.code | P1 |  | S | Add tests/test_stable_prefix_ab.py to the eval-lab selftest line in the Makefile and apply the ledger WS02-S8 entry. |
| D1-378 | WS01-S14.code | P1 |  | S | Apply the lead_todo lines to MODEL_PLANE_STATUS.md, ledger.md, evidence README.md and trust-claims-pending-reverification.md. |
| D1-427 | WS06-S9 | P1 |  | M | Add a rotate-audit-chain-keys script/runbook step in scripts/ that replaces the pair atomically and refuses a half-present ledger. |
| D1-522 | WS10-S6 | P1 |  | M | Fix the three type errors in apps/Frontend Plane/verevonv3 (or baseline them), then pnpm typecheck and pnpm build. |
| D1-539 | WS08-S3.code | P1 |  | S | Add E::VisionUnsupported to status_contract_cases in inference-core grpc.rs tests, regenerate with MP_UPDATE_GOLDEN=1, re-pin digest in scripts/tests/zdr-contract-sync.test.sh, run it. |
| D1-566 | WS07-S5 | P1 |  | S | none |
| D1-582 | WS08-S7 | P1 |  | S | Add CHAT_EXTRACT_MAX_BYTES passthrough to imports-core in the compose file and confirm the cap vars reach the BFF gateway compose. |
| D1-640 | WS08-S4.code | P1 |  | S | Add a short error-code note for attachment_do_unsupported to the sse-event-taxonomy doc (or its generator) and a test pinning it. |
| D1-643 | WS07-S8 | P1 |  | M | Add a per-user erase path in session-core gdpr.rs deleting agent_skills then skill_revisions WHERE owner_user_id=$1, with a DB test. |
| D1-680 | WS02-S12 | P1 |  | S | orchestration_store.rs: cap todos in render_todo_recitation by count or token budget (copy the WS02-S6 history budget) with a test. |
| D1-685 | WS02-S12 | P1 |  | L | Add a session-core RPC (proto tag per section 4.3), an orchestration_grpc handler calling recite_plan_todos, and the prompt_context.rs Volatile block. Do this once a producer exists (D1-678). |
| D1-689 | WS04-S8 | P1 |  | M | When the owner retract op is built (after D1-687 decision): session-core marks row withdrawn first, ack, then gateway releases queue entry; add crash-ordering test. |
| D1-690 | WS04-S8 | P1 |  | S | With retract wiring: remove entry from state.queuedInputs/queuedSendContext on retract; add controller test in use-chat-controller.send-receipts.test.ts. |
| D1-698 | WS09-S4 | P1 |  | M | Run file with VITE_CHAT_DELTA_FRAME_BATCH=true, find stale controller/timer leak across cases (dispose coalescer and snapshotWriter in afterEach); fix test isolation or controller. |
| D1-699 | WS09-S4 | P1 |  | M | Add flushAll to delta coalescer in chat-client.ts, call from lifecycle triggers and at start of onCleanup before snapshotWriter.flush(). |
| D1-700 | WS09-S4 | P1 |  | S | In use-chat-controller.resume-replay.test.ts (~227) and snapshot-throttle cases, advance a frame (fake rAF) before assertions when the switch is on. |
| D1-712 | WS09-S7 | P1 |  | S | Add a WS09-S7 Build ledger entry to the build plan (switch VITE_CHAT_CONTENT_CONTAINMENT, unset=off, live proof pending, D5 open). |
| D1-718 | WS09-S10 | P1 |  | M | Extend tests/e2e/chat-transcript-performance.spec.ts with the five cases under VITE_CHAT_VIRTUALIZE=true. |
| D1-719 | WS09-S10 | P1 |  | M | Add ChatPage.virtualize.test.tsx with VITE_CHAT_VIRTUALIZE on: stub ResizeObserver, assert spacers, role=feed/article with aria-setsize/posinset, focusin tracking and arrow-key focusTurn. |
| D1-726 | WS11-S9 | P1 |  | S | When INFERENCE_CORE_CODEX_WIDE_EFFORT is on, return the wide ladder (Low..XHigh) from list_models; add a test; gated on D1-727 knowledge. |
| D1-733 | WS07-S10 | P1 |  | M | Add a Redis-backed ReviewBudget (or a Postgres counter) in sessionreview and pass it in WithCostControls from cmd/main.go; cover with test. |
| D1-759 | WS09-S9 | P1 |  | M | Carry citations/artifacts/grounding in the messages tail read, or fall back to the transcript read for adoption when the flag is on. |
| D1-760 | WS09-S9 | P1 |  | M | Page the /transcript BFF route (S8 scope) or drop the metadata dependency so a cold paged open reads one page. |
| D1-761 | WS09-S9 | P1 |  | M | Mark turns older than the server window as non-actionable in the controller and disable their action buttons until loaded; guard edit against unknown ids. |
| D1-765 | WS09-S9 | P1 |  | M | use-chat-controller.ts edit/rewind/branch handlers: recompute leadingTurnCount (or clear earlierCursor) when s.turns is replaced/truncated, or disable those actions on turns outside the server window. |
| D1-776 | WS09-S8 | P1 |  | M | Add a read-route row for GET thread messages with limit/before to the canonical matrix in zdr_posture_matrix.rs and assert the org join and audience ceiling. |
| D1-781 | WS02-S10.code | P1 |  | S | In openai.rs sends_prompt_cache_key add a shape check (32 lowercase hex, optional ':'+suffix) and a test with non-derived text asserting no prompt_cache_key. |
| D1-006 | WS05-S2 | P2 |  | S | Add GET /api/v1/mcp/{id}/tools, POST approve and annotation_trusted schema to apps/Model Plane/api/openapi.yaml. |
| D1-013 | WS05-S3 | P2 |  | S | Edit plan WS05 section and .plan-work/designs/WS05-declared-tool-classes.md to execution-core first, capability-core second. |
| D1-016 | WS05-S4 | P2 |  | S | none |
| D1-017 | WS05-S4 | P2 |  | M | Add x-capability-authorization via verify_delegated_bearer(audience capability-core) in grpc_auth.rs intercept and drop the is_service rule. |
| D1-020 | WS04-S4 | P2 |  | M | Retry via set_run_mode_retryable in raw ExitPlanMode and add the e_zdr_token_plan_grant_refused matrix row. |
| D0-020 | WS23-S3.code | P2 |  | S | Pass a space_scoped flag into dispatch and replace the sandbox.is_some() check; fix the source-text guard in tests/provider_read_contract.rs; confirm integration-corev2 connection id charset. |
| D0-021 | WS07-S7 | P2 |  | S | Write the DEC-30 ledger entry and the 0058 contract note in the docs, then run the six session-core DB tests and verify-durable-layer.sh on the live stack. |
| D1-023 | WS12-S5 | P2 |  | M | Remove RECEIPT_*_LEAKS/SPARSE_RECEIPT_LEAKS and artifact pins as translations land; mutation-test PlanApprovalControl label; run pnpm test. |
| D1-027 | WS04-S4 | P2 |  | M | Publish after SetRunMode outcome or add persisted field to ExitPlanModeResponse and condition the event on it. |
| D1-028 | WS04-S4 | P2 |  | S | Make retryable call set_run_mode_failure_is_transient (one predicate, decide on Unknown). |
| D1-039 | WS11-S7 | P2 |  | S | Extract the clamp block of resolve_intent into a pure fn taking (enabled: bool, req, decision) in fallback.rs/effort.rs and unit-test it with an Exhausted decision. |
| D1-066 | WS12-S7.code | P2 |  | S | Generate VARIANTS from the same macro/list as covered(), or assert VARIANTS.len() equals a count derived from the ChatEvent enum. |
| D1-105 | WS12-S8 | P2 |  | S | Add those helpers to READING_HELPERS in chat-event-contract.test.ts (and extend aliases/golden mapping) so their key reads are checked. |
| D1-106 | WS12-S8 | P2 |  | S | Make the AST visitor treat ObjectBindingPattern on the payload as key reads, or add a lint rule forbidding destructuring of payload. |
| D1-287 | WS04-S5 | P2 |  | M | Add a test in http_routes.rs with a test-injected mirror switch (not process env) driving ingest_feedback and the sse.rs call-in with the flag on. |
| D1-288 | WS04-S5 | P2 |  | M | Add a stream_buffer.rs test: in-memory kv_set/kv_get no-op without REDIS_URL, plus an ignored real-Redis TTL round trip. |
| D1-361 | WS02-S11 | P2 |  | M | Add a source-read or mock-AppState test in model-gateway sse.rs tests asserting summarize_head ends in abort(timeout/provider) or commit. |
| D1-396 | WS06-S6 | P2 |  | S | Add a Rust test in audit_compliance.rs that an org-core 402 plan_upgrade_required body on SIEM create passes through with status and code unchanged. |
| D1-460 | WS10-S1 | P2 |  | S | none |
| D1-513 | WS08-S2.code | P2 |  | M | Add model-gateway test driving the real /v1/invoke/stream handler with a subscription provider plus image, asserting error frame and zero run-create calls. |
| D1-514 | WS08-S2.code | P2 |  | S | Add the two golden rows to model-gateway tests/zdr_posture_matrix.rs and zdr_waiver_route_tests.rs. |
| D1-800 | WS11-S1 | P2 |  | M | Add the RequestRejected row to infer-statuses.json, regenerate with MP_UPDATE_GOLDEN=1, update the digest in scripts/tests/zdr-contract-sync.test.sh and the two Data Plane fixture copies; check parts (2),(3). |
| D0-009 | cross | P3 |  | S | Add a gen:chat-events:check script (node scripts/gen-chat-event-types.mjs --check) and call it from the Tier B pass. |
| D1-010 | WS05-S2 | P3 |  | S | Move module into lib.rs and export derive_org_scoped_service_token instead of duplicating. |
| D1-014 | WS05-S3 | P3 |  | S | Add a test asserting the counter increments on run eviction (metrics recorder snapshot). |
| D1-092 | WS01-S8 | P3 |  | S | Add ARG SOURCE_REVISION/BUILD_DATE and the org.opencontainers.image.revision/created LABELs to the nats-provisioner Dockerfile, as in bridge-core. |
| D1-799 | WS04-S2 | P3 |  | M | model-gateway sse.rs: move the gateway_active_streams guard into the spawned tasks of the five helper streams; update their signatures and test call sites. |

## Live proofs still owed (what to run and observe)

| Row | Slice | P | Blocks W1 | Effort | Next step |
|---|---|---|---|---|---|
| D1-001 | WS01-S1 | P1 | yes | M | Rebuild Control Plane Prometheus with committed rules, force one audit-path failure, record the firing alert in an evidence file. |
| D1-005 | WS05-S2 | P1 | yes | M | Connect mcp-annotated-server.mjs via a minted gRPC token, see 3 rows, change a description, see tool_definition_changed after 60 s. |
| D1-012 | WS05-S3 | P1 | yes | L | Rebuild execution-core then capability-core with TOOL_CLASS_GATE=shadow, soak, read divergence counters, then enforce. |
| D1-015 | WS05-S4 | P1 | yes | M | On proof stack set MCP_DO_TOOLS=read TOOL_CLASS_GATE=enforce; check Do calls echo_read, create_item absent, plan mode refuses it, ZDR sees none. |
| D1-018 | WS04-S4 | P1 | yes | M | Run session-core DB harness (+3 tests) and scripts/verify-durable-layer.sh on the dev stack; migration itself is applied. |
| D1-021 | WS03-S9 | P1 | yes | M | Run durable-loop-live-proof.sh on the dev stack, fix names/decide body, write the evidence bundle into MODEL_PLANE_STATUS.md. |
| D1-024 | WS11-S5 | P1 | yes | M | On a dev route send quick then deep; observe effort_resolved Low/High, distinct cache keys, /v1/models effort_levels; flags adapter first, then MODEL_GATEWAY_EFFORT_LADDER. |
| D1-030 | cross | P1 | yes | L | Run cargo check -p model-gateway, browser-broker go test, Postgres store tests and the e2e harness; merge conflicts are a separate task. |
| D1-035 | WS11-S7 | P1 | yes | M | Rebuild inference-core with the clamp flag and S2 effort switch; invoke a verevon-* mode deep as an exhausted-budget org; expect Low and the clamp log line. |
| D1-044 | WS01-S2 | P1 | yes | L | Clean detached worktree at WS01-S0 commit, rebuild-service.sh --require-clean session-core, then audit-delivery-proof.sh stages baseline, deploy, verify, canary (10 rows), gate, replay, final. |
| D1-045 | WS01-S2 | P1 | yes | S | See D1-044. |
| D1-051 | WS01-S3 | P1 | yes | M | run-control-plane.sh up -d --no-deps --build auth-core; record before/after organization_membership_audit_outbox counts (3 stuck, 3 dead-lettered, attempts stable) and audit_events role_change rows. |
| D1-057 | WS01-S3 | P1 | yes | M | cd 'apps/Control Plane/auth-core' && pnpm test -- membership-audit with TEST_DATABASE_URL and CONTROL_LIFECYCLE_FIXTURE_ID set on a lifecycle fixture. |
| D1-059 | WS12-S7.code | P1 | yes | M | Rebuild model-gateway, curl -N one non-ZDR turn (reasoning, steps, tools, citations, artifacts, usage) via the BFF, run node scripts/verify-sse-frames.mjs on it, repeat with a ZDR turn for the refusal frame. |
| D1-069 | WS01-S9 | P1 | yes | L | Run scripts/rollback-rehearsal.sh on a stack with baseline and candidate artifacts: older session-core must boot over newer schema with the switch true and fail with it false. |
| D1-073 | WS01-S9 | P1 | yes | M | On the DB harness run: cargo test -q -p session-core --all-targets with DATABASE_URL set; record pass counts in an evidence file. |
| D1-075 | WS01-S9 | P1 | yes | S | On the DB harness: DATABASE_URL=... cargo test -p session-core --bin session-core older_migrator_boots_only_with_ignore_missing -- --ignored; expect pass; bump harness count by 1. |
| D1-086 | WS01-S8 | P1 | yes | L | On Linux/CI run release-artifact.sh build in release mode; confirm the nats-provisioner image builds and restore/compose config accept NATS_PROVISIONER_RELEASE_IMAGE. |
| D1-088 | WS01-S8 | P1 | yes | M | On Linux/WSL run the three tests under apps/Model Plane/scripts/tests (release-contract-test.sh first: count 22, new overlay assertion, 12 app images) or cite a green ops-gates run. |
| D1-104 | WS12-S8 | P1 | yes | M | Stack up: drive one BFF chat turn with reasoning, citations, usage (plus a ZDR turn), save curl -N output, run node scripts/verify-sse-frames.mjs <transcript> in verevonv3, and confirm the SPA renders it. |
| D1-113 | WS01-S4 | P1 | yes | M | Run Application Plane jetstream-provisioner, then audit-extra-nats-provisioner on the live stack; confirm stream, two durables (ensureConsumer accepts), 0 pending, and a leads-core event in audit_events with plane=application. |
| D1-117 | WS01-S4 | P1 | yes | S | Close into D1-113; add 0 pending and /readyz application bus healthy to its proof. |
| D1-125 | WS10-S2 | P1 | yes | M | Run TestControlLifecycleModelEntitlementRepositoryUnderRLS via apps/Control Plane/scripts/run-isolated-control-lifecycle-e2e.sh (add a step there); check gdpr_hard_delete_organization still succeeds with the new tables. |
| D1-126 | WS10-S2 | P1 | yes | M | On live org-core: curl internal route for a no-policy org (state none, revision 0, no-store), PUT a signed delegation, GET per role, capture the org_model_policy_history row. |
| D1-135 | WS10-S2 | P1 | yes | M | On the dev stack curl the org-core internal entitlement route: no-policy org (state none, revision 0, no-store), signed PUT, GET per role; capture the history row. |
| D1-138 | WS01-S5 | P1 | yes | S | Run the model-plane-ops-gates release job on CI (or Linux) and record the three tests passing. |
| D1-140 | WS01-S5 | P1 | yes | M | Dev stack: boot inference-core with past review-by and recomputed digest; show ERROR in docker logs, /api/v1/models without zdr, typed zdr_unavailable, gauge on :8082/metrics. |
| D1-147 | WS01-S10 | P1 | yes | L | Run rollback-rehearsal.sh on a stopped stack with real S12 artifacts; confirm rollback session-core logs 'migrations applied' with JWKS reachable on the bridge network. |
| D1-149 | WS01-S10 | P1 | yes | L | Run the S12 real rehearsal (docker, signing/verify key, stopped stack) and commit rollback-rehearsal-<date>.txt. |
| D1-169 | WS09-S5 | P1 | yes | M | Build remote-core, pnpm exec vite build, vite preview --port 5176 --strictPort, pnpm exec playwright test --project=chunk-recovery; confirm AuthPage-*.js name and /login reaches lazy AuthPage with no backend. |
| D1-170 | WS09-S5 | P1 | yes | M | docker buildx build --target production, run it, open a tab, deploy a new build, navigate to a lazy route; observe one reload and notice (dirty composer blocks the reload). |
| D1-220 | WS01-S7.code | P1 | yes | M | Run tests/e2e/zdr-live-turn.spec.ts against a ZDR-TEST org on the live stack; confirm the refusal frame shape, code field and 'not sent to a model' text. |
| D1-221 | WS01-S7.code | P1 | yes | L | Ordered deploy (rebuild-service.sh x4, SPA pinning, FE gateway, frontend restart), then zdr-live-proof.sh preflight, before, after on the dev stack. |
| D1-222 | WS01-S7.code | P1 | yes | S | Run the spec and zdr-live-proof.sh preflight against a non-ZDR org; both must refuse. |
| D1-223 | WS01-S7.code | P1 | yes | S | Run the session-core zdr_receipt DB tests on the DB harness; expect at least 8 pass. |
| D1-224 | WS01-S7.code | P1 | yes | M | zdr-seed-test-org.mjs --apply, run zdr-org-switch-gate.spec.ts with ZDR_GATE_LIVE_PATCH=1 (expect 409 zdr_no_attested_provider), then switch the test org back off. |
| D1-225 | WS01-S7.code | P1 | yes | S | After the D1-221 run passes, write the evidence file, ZDR.md Z-60 rows, ledger flip and runbooks/README.md link. |
| D1-261 | WS06-S0 | P1 | yes | M | bash scripts/run-control-plane.sh up -d controlplane-prometheus; capture /api/v1/rules (group control-audit-core, 5 rules); publish a forged-subject audit event on the Control bus; capture /api/v1/alerts firing. |
| D1-318 | WS02-S2 | P1 | yes | M | Rebuild cost-core then model-gateway; run 6-turn Anthropic and pinned-openai threads; capture GET /api/v1/cost/analytics?scope=organization (cache_breakdown) plus the SPA tile; compare idle windows to the 300 s ttl. |
| D1-324 | WS02-S2 | P1 | yes | M | bash apps/Model Plane/scripts/compose.sh up -d --no-deps --build cost-core model-gateway (cost-core first); confirm boot log applies 0011 and gateway healthy. |
| D1-325 | WS02-S2 | P1 | yes | S | Set COST_CORE_TEST_DATABASE_URL to a dev Postgres and run go test -tags integration -run TestPostgresLedger_CacheMissReasonRoundTrip ./internal/postgres/ in cost-core. |
| D1-330 | WS01-S13.code | P1 | yes | M | After Z-40: deploy with ZDR_RETENTION_PATH=zdr-provider-route-attested, check /api/v1/models lists zdr, run pass 2 on ZDR-TEST org with zero deltas. |
| D1-341 | WS06-S3 | P1 | yes | M | Run audit-core store DB tests with TEST_DATABASE_URL, then a dev-stack purge with AUDIT_CHAIN_PRUNE_DRYRUN first; confirm verification stays green and capture evidence. |
| D1-356 | WS02-S11 | P1 | yes | M | On rebuilt model-gateway scrape /metrics for mp_gateway_compaction_total{phase,outcome}; force a summariser timeout and observe abort/timeout plus the DROPPED_HISTORY_NOTICE marker. |
| D1-360 | WS02-S11 | P1 | yes | M | On a rebuilt model-gateway, force a summariser timeout, scrape /metrics for mp_gateway_compaction_total{phase="abort",outcome="timeout"} and capture the dropped-history marker in the prompt. |
| D1-363 | WS02-S8.code | P1 | yes | L | Run the runbook on staging: drive, capture, three eval reruns per arm, compare; write the A/B table (hit rate, write share, eval pass rate). |
| D1-364 | WS02-S8.code | P1 | yes | S | Before the A/B run, mint the drive's token and call cost-core capture; if refused, issue a cost-core token with cost:org:read for the eval org. |
| D1-373 | WS01-S14.code | P1 | yes | S | Read the running session-core image revision and SELECT max(version) from _sqlx_migrations (expect 43) on the dev stack before the S2 deploy step. |
| D1-379 | WS01-S14.code | P1 | yes | S | Run promtool check rules and promtool test rules audit-delivery.rules.test.yml via the prom/prometheus image (docker) and record the output. |
| D1-388 | WS06-S5 | P1 | yes | M | Deploy audit-core image first, then compose via scripts/run-control-plane.sh; confirm audit-core boots and the gateway principal holds the scope. |
| D1-391 | WS06-S6 | P1 | yes | M | Rebuild gateway and audit-core; as owner/admin/member call GET /api/v1/audit/compliance/events, /manifest, /keys and POST /verify; expect 200/200/403 plus the two self-audit rows. |
| D1-476 | WS10-S3 | P1 | yes | M | Stack with S3 shadow: soak and read would_refuse/service_exempt; stop org-core, observe stale service 900 s/120 s then 503 model_entitlement_unavailable; capture x-mp-provider-error and x-mp-entitlement-reason under enforce. |
| D1-484 | WS04-S7 | P1 | yes | M | Two-replica overlay with SHARED_IDEMPOTENCY=true and REDIS_URL: same key to :8080 and :28080 gives one runs row, same frames; add and run an #[ignore] RedisClaimStore SET NX PX/settle test against Dragonfly. |
| D1-492 | WS10-S5 | P1 | yes | M | Rebuild verevon-gateway-rs, run real org-core with S2 migration, make a real-cookie PUT/GET on the model-entitlement routes and confirm org-core accepts the delegation signature. |
| D1-494 | WS10-S5 | P1 | yes | M | Test stack only: rebuild gateway, MODEL_ENTITLEMENT_ADMIN_ENABLED=true, curl as owner and member with real cookies: PUT/GET round trip, member 403, explain, ZDR-TEST 409 model_policy_leaves_no_zdr_model, org-core stopped fail closed. |
| D1-503 | WS10-S4 | P1 | yes | M | Rebuild model-gateway, inference-core, execution-core; with S3 enforce on, restricted member refused on /v1/invoke, /v1/invoke/stream and gRPC via the BFF, owner not, zero new thread/run rows. |
| D1-510 | WS08-S2.code | P1 | yes | M | Rebuild model-gateway and BFF; curl /v1/invoke/stream with openai-codex-subscription plus image: expect error frame attachment_route_unsupported and no session-core run row; 5 MiB body 200; capture /api/v1/models with attachment_policy and modes.vision=true. |
| D1-521 | WS10-S6 | P1 | yes | M | Second Vite on 5174 (restart for host edits), signed in: E2E_BASE_URL=http://localhost:5174 pnpm test:e2e -- model-entitlement; keep trace and screenshots, check the 403 model_not_entitled Norwegian refusal. |
| D1-523 | WS10-S6 | P1 | yes | L | WS10-S7 flow: rebuilt frontend and gateway, switches on, admin edits role policy, member picker drops a model, pinned denied send shows the Norwegian refusal. |
| D1-532 | WS07-S1 | P1 | yes | M | DB harness vs a disposable DATABASE_URL: cargo test -p session-core -- --ignored for the six named tests plus the pre-existing forgotten_memory, delete_memory, retention_receipt and learning_events DB tests. |
| D1-564 | WS07-S5 | P1 | yes | M | Rebuild session-core and capability-core on a stack; as a paused user POST /v1/memory (403), org_disabled GET (403), GET /api/v1/settings/ai, boot migration 030 twice on real Postgres. |
| D1-567 | WS07-S5 | P1 | yes | M | Reload auth-core registry via run-control-plane.sh with the new scope, then rebuild capability-core and confirm its session-core token mint succeeds. |
| D1-568 | WS07-S5 | P1 | yes | M | After D1-567: run-control-plane.sh, rebuild session-core and capability-core, curl POST /v1/memory as paused user (403 memory_not_enabled), check settings/ai, migration 030 twice. |
| D1-612 | WS08-S5 | P1 | yes | M | In rust:1 container run scripts/verify-sandbox-isolation.sh and the two Linux-gated bwrap tests in execution-core/sandbox; capture EROFS on attachments/ writes, --ro-bind argv, no verevon-code-* temp dir left. |
| D1-618 | WS07-S7 | P1 | yes | M | On the live stack, as users A and B: A learns a skill (proposal row), check B's /api/v1/skills and chat turn do not see it, paused user writes nothing; then run session-core-db-tests.sh for the six tests. |
| D1-622 | WS07-S7 | P1 | yes | M | Run scripts/tests/session-core-db-tests.sh without org-core credentials; confirm the six S7 ignored tests pass and update the harness count. |
| D1-624 | WS07-S3 | P1 | yes | M | On the live stack drive PUT /api/v1/memory/controls, reset and remember via the signed-in page; confirm session-core rows change and run tests/e2e/product-memory.spec.ts. |
| D1-630 | WS07-S3 | P1 | yes | M | Rebuild gateways, run second Vite server, run product-memory.spec.ts 'paused user: no recall, no new memory, resume works', screenshot three states, capture PUT controls network. |
| D1-637 | WS08-S4.code | P1 | yes | M | Rebuild with both CHAT_VISION flags true; 2-image Ask on verevon-balance and Anthropic with tool follow-up (SSE chunk/usage/done, text-only persisted message), Do-with-image gets attachment_do_unsupported, ZDR chat refused; then flip default. |
| D1-646 | WS07-S8 | P1 | yes | M | Run the four plan PG tests via scripts/tests/session-core-db-tests.sh against real Postgres; confirm trigger, jsonb_to_record restore and ledger UNION work. |
| D1-665 | WS07-S9 | P1 | yes | M | On the dockerized stack with a signed-in account, run playwright test tests/e2e/product-learning.spec.ts (accept, undo, reject, admin promotion). All must pass. |
| D1-666 | WS07-S9 | P1 | yes | M | Rebuild session-core, capability-core, model-gateway and the gateway from HEAD, drive accept, reject, undo and the admin copy on the S7 rig, and capture learning-inbox.png. |
| D1-763 | WS09-S9 | P1 | yes | M | On a rebuilt stack with the S9 flag on, open a 400-turn thread in the browser: check cold-open request shape, load-earlier has_more round trip, scroll anchor, saved-answer adoption. |
| D1-004 | WS04-S1 | P1 |  | S | Run the five #[ignore] live NATS tests against the dev NATS and record the result; the replica half is done. |
| D1-022 | WS09-S3 | P1 |  | M | Run Playwright before/after (<=1 transcript write/s), kill tab mid-stream, add a test pinning one write per terminal. |
| D1-036 | WS11-S7 | P1 |  | M | Run inference-core with the clamp env true and the effort ladder on, invoke verevon-* with effort deep for a cost-core exhausted-budget org, observe the log line requested=deep applied=low. |
| D1-047 | WS01-S2 | P1 |  | S | After audit-delivery-proof.sh --stage final passes, update postmortem 0002 Status and trust-claims section 6. |
| D1-048 | WS01-S2 | P1 |  | M | Run cargo test -p session-core audit_publisher -- --ignored with DATABASE_URL, NATS_URL, USER, PASSWORD set, and go test in audit-core with TEST_DATABASE_URL. |
| D1-055 | WS01-S3 | P1 |  | M | cd 'apps/Control Plane/auth-core'; pnpm test on HEAD and on the tree, diff the failing spec sets (not exit codes). |
| D1-062 | WS12-S7.code | P1 |  | S | On the main checkout with all planes present run bash scripts/tests/zdr-contract-sync.test.sh and its --self-test; record exit codes. |
| D1-082 | WS10-S0 | P1 |  | S | Tracked under D1-078. |
| D1-089 | WS01-S8 | P1 |  | S | On Linux run scripts/release-artifact.sh dry-run and confirm 12 docker build lines including nats-provisioner. |
| D1-102 | WS12-S8 | P1 |  | S | Close into D1-104. |
| D1-118 | WS01-S4 | P1 |  | S | Close into D1-113. |
| D1-124 | WS10-S2 | P1 |  | S | none |
| D1-133 | WS10-S2 | P1 |  | S | none |
| D1-145 | WS01-S5 | P1 |  | S | Run bash scripts/tests/zdr-contract-sync.test.sh on Linux/CI at HEAD and at the base commit; diff the failing sets. |
| D1-148 | WS01-S10 | P1 |  | S | On first live run check env values with quotes; if wrong, render env via release-artifact.sh compose. |
| D1-152 | WS01-S10 | P1 |  | S | Once S12 artifacts exist run: bash 'apps/Model Plane/scripts/rollback-rehearsal.sh' --dry-run <candidate-dir> <rollback-dir>. |
| D1-159 | WS12-S11 | P1 |  | S | On a throwaway branch run repo-gates (green), then append one line to execution-core runtime_loop/agent.rs and show it red. |
| D1-232 | WS04-S2 | P1 |  | S | none |
| D1-235 | WS04-S2 | P1 |  | S | cargo test -p model-gateway --lib sse_event_payload_golden and cargo test -p execution-core --test zdr_refusal_inventory at HEAD; fix or re-pin the digest if red. |
| D1-239 | WS01-S11 | P1 |  | S | Push the branch/open a PR, confirm both jobs of Model Plane Ops Gates are green incl. session-core migrations on postgres:16; record the run URL. |
| D1-240 | WS01-S11 | P1 |  | S | Run the three release-*.sh tests on Linux/WSL (or via the ops-gates CI run) and record the pass. |
| D1-244 | WS01-S11 | P1 |  | M | Set MODEL_PLANE_CANDIDATE_ARTIFACT_DIR, ROLLBACK_ARTIFACT_DIR, ARTIFACT_VERIFY_KEY (path), run scripts/rollback-rehearsal.sh, commit evidence, run coresystem-conformance.sh --json --profile runtime on clean tree. |
| D1-248 | WS12-S12 | P1 |  | S | Run the spa-browser-fixtures job on GitHub (or docker locally): chunk-recovery project 3 tests green against vite preview; confirm AuthPage chunk name matches the build. |
| D1-249 | WS12-S12 | P1 |  | S | Throwaway branch: import ~200 KB into the entry, run spa-verify budget step with ceilings set, observe it red; record the run, delete branch. |
| D1-253 | WS12-S12 | P1 |  | S | Open a PR; confirm the 'git fetch --depth=1 origin <base sha>' step and check-bundle-budget --base-ref FETCH_HEAD succeed. |
| D1-264 | WS06-S1 | P1 |  | S | Push the branch or open a PR, confirm audit-core-ci.yml is green, record the run URL as S1 evidence. |
| D1-267 | WS06-S1 | P1 |  | S | Push model-plane or open a PR so audit-core-ci runs; record the green run URL in the ADR or evidence file. |
| D1-276 | WS05-S6.code | P1 |  | L | Run TOOL_CLASS_GATE=shadow on the proof stack for two weeks, require zero tool_class_divergence_total for built-ins, then flip. |
| D1-279 | WS05-S6.code | P1 |  | M | Run cargo test --workspace --no-fail-fast, cargo check --workspace --all-targets in rust:1 container (Linux); diff fmt failures against HEAD baseline. |
| D1-284 | WS04-S5 | P1 |  | M | Run cargo test -q -p model-gateway --all-targets in the rust:1 Linux container with cache volumes (bash -c); record result. |
| D1-302 | WS01-S12.code | P1 |  | L | On Linux, run the release pipeline with ephemeral keys (label DEV-KEYED): build, verify, restore, rehearsal, baseline/candidate deploys; commit evidence. |
| D1-305 | WS01-S12.code | P1 |  | S | Run release-contract-test.sh, release-runtime-partition-test.sh, release-input-security-test.sh on Linux or in ops-gates CI; record green. |
| D1-306 | WS01-S12.code | P1 |  | M | After D1-302, run scripts/coresystem-conformance.sh --profile runtime on a clean tree and expect ready. |
| D1-314 | WS06-S3 | P1 |  | M | Run the ignored purge_chain_test.go and verify_test.go DB tests via the DB harness, then a dev-stack purge with signing key; observe verification stays green before AUDIT_CHAIN_PRUNE_ENABLED=1. |
| D1-331 | WS01-S13.code | P1 |  | S | Edit ZDR.md and add live-reads-zdr-<date>.txt only after the real attestation and the live pass 2 run. |
| D1-333 | WS01-S13.code | P1 |  | S | Run release-runtime-partition-test.sh and the signed-policy gate path on Linux or macOS (or WSL/CI) and record the result. |
| D1-400 | WS06-S8 | P1 |  | M | Stack with a TLS NDJSON receiver; create a destination via gateway, enable AUDIT_SIEM_EXPORT_ENABLED for a test org, run audit-verify on received NDJSON plus head record, fire and clear the lag alert. |
| D1-417 | WS04-S3 | P1 |  | M | On the two-replica stack, queue an ~8 KiB message via B to an owner on A under the real NATS ACL, and probe a killed owner to observe no-responders and the 750 ms timeout. |
| D1-420 | WS04-S3 | P1 |  | S | Two-replica stack: start a run on A, keep the producer quiet over 120 s, resume the stream via B and confirm it still tails. |
| D1-430 | WS06-S9 | P1 |  | M | Stack: chain on, mutate an audit row via the GUC and confirm the verifier flags it; capture SIEM receiver output. |
| D1-436 | WS08-S1 | P1 |  | S | Run tests/e2e/chat-attachment-gates.spec.ts against the live stack with a signed-in test user and record pass/fail. |
| D1-438 | WS08-S1 | P1 |  | M | Second Vite dev server over the worktree, run pnpm test:e2e -- tests/e2e/chat-attachment-gates.spec.ts --project=chat; screenshot the six refusals, capture network showing no /api/v1/chat/stream request. |
| D1-444 | WS02-S9 | P1 |  | M | Before enabling, run several Anthropic Do turns with a changing Context tier and compare cache_creation vs cache_read tokens against the S2 baseline. |
| D1-450 | WS02-S9 | P1 |  | M | Deploy inference-core then execution-core, set switch true, run two consecutive Anthropic Do turns with changed volatile block, confirm cache_read_input_tokens covers Stable+Context. |
| D1-453 | WS10-S1 | P1 |  | S | See D1-456. |
| D1-456 | WS10-S1 | P1 |  | M | Rebuild model-gateway via recreate-with-live-env.sh with ALLOWED_MODELS=a, call /v1/invoke and /v1/invoke/stream with model b, capture the 403 body and SSE error frame, restore env, check the SPA Norwegian text. |
| D1-462 | WS04-S6 | P1 |  | M | On a live stack with normal chat load, read the registry's in-flight claim gauge per replica over a soak and confirm it stays well under 1,024. |
| D1-472 | WS10-S3 | P1 |  | L | Shadow soak reading would_refuse/service_exempt; verify org-core token+ORG_CORE_URL from inference-core; stop org-core to see stale within 900/120 s then 503 model_entitlement_unavailable; grpcurl enforce trailers. |
| D1-516 | WS10-S6 | P1 |  | M | Second Vite dev server on :5174 (restart Vite), run E2E_BASE_URL=http://localhost:5174 playwright on model-entitlement.spec.ts; confirm selectors and Norwegian text for 403 model_not_entitled. |
| D1-540 | WS08-S3.code | P1 |  | M | Rebuild inference-core, send two images to a live vision deployment, capture the provider-bound body shape via a debug test (no bytes in logs). |
| D1-547 | WS03-S8 | P1 |  | L | Run the five ignored DB tests on a disposable DB; live: kill execution-core past the deadline, observe park_not_due, resume by continue, TTL expiry (needs executor half). |
| D1-554 | WS07-S2 | P1 |  | M | Run the three ignored PG reset tests through the DB harness on a disposable DATABASE_URL and bump the harness test count by 3. |
| D1-557 | WS07-S2 | P1 |  | M | Rebuild session-core; seed 3 memories (inferred, stated, org); grpcurl ResetMemory with a user bearer; ListMemory empty, org row intact, mirror count, reset row with durable_deleted. |
| D1-563 | WS07-S5 | P1 |  | M | On a live stack: paused user POST /v1/memory via model-gateway expects 403; org_disabled read 403; user-core GET /api/v1/settings/ai has no memoryEnabled; boot twice, migration 030 idempotent. |
| D1-576 | WS07-S6 | P1 |  | S | Run scripts/tests/session-core-db-tests.sh with the ignored test thread_off_blocks_read_and_write_for_that_thread_only on the DB harness and update the harness count. |
| D1-583 | WS08-S7 | P1 |  | S | Run CARGO_INCREMENTAL=0 cargo test -q -p model-gateway --lib stream_body_limit now that protos compile. |
| D1-584 | WS08-S7 | P1 |  | S | Run pytest tests/test_chat_extract.py in the imports-core container; check importlib.reload fixture and zip-bomb peak-memory assertion. |
| D1-585 | WS08-S7 | P1 |  | M | On the live stack with raised cap env, send two 3 MB files from Home via BFF, model-gateway, inference-core; observe acceptance at each hop; run the Playwright spec. |
| D1-588 | WS10-S7.code | P1 |  | M | Run model-entitlement-live-proof.sh --phase preflight on the dev stack; fix CP_*/MP_* defaults and metrics parsing as it fails. |
| D1-594 | WS10-S7.code | P1 |  | S | Run bash apps/Model Plane/scripts/tests/model-entitlement-live-proof-test.sh (4-5 min) and confirm green. |
| D1-595 | WS10-S7.code | P1 |  | L | Run the runbook in order: preflight off, shadow dry run plus a one-day soak, admin on staging with the ZDR leg, enforce steps a-i, stop-org-core stale run, prod admin flag. |
| D1-599 | WS07-S4 | P1 |  | M | Run session-core DB tests plus a live org policy poll with the org-core scope granted; observe mirror upsert, watermark and memory_control::stored_inputs. |
| D1-649 | WS07-S8 | P1 |  | S | On a scratch copy of the live DB, apply migrations/revert/0064_skill_revisions.down.sql once, check skill_revisions is gone, then re-apply 0064 and confirm it is clean. |
| D1-675 | WS04-S9 | P1 |  | S | With NATS reachable, run cargo test -p model-gateway --test run_control_nats_live -- --ignored. Confirm all five pass, including the no-responders mapping on async-nats 0.49.1. |
| D1-684 | WS02-S12 | P1 |  | S | Against a scratch Postgres with the session-core DB harness, run cargo test -p session-core a_recitation_lists_the_owners_plan -- --ignored. It must pass. |
| D1-693 | WS04-S8 | P1 |  | M | After op lands: rebuilt model-gateway+session-core; queue on replica A, retract via B, psql shows messages row withdrawn and absent from next ListConversation; retract after delivery returns AlreadyDelivered. |
| D1-697 | WS09-S4 | P1 |  | M | Second dev server on rebuilt frontend: run chat-transcript-performance.spec.ts 3x with switch unset and 3x with VITE_CHAT_DELTA_FRAME_BATCH=true; capture Stop mid-stream; record JSON. |
| D1-707 | WS09-S7 | P1 |  | S | On second dev server with VITE_CHAT_CONTENT_CONTAINMENT=true, open .verevon-chat-menu__panel (incl. --down) on a non-latest turn near article edge; confirm not clipped. |
| D1-710 | WS09-S7 | P1 |  | M | Second dev server, switch true vs unset: chat-transcript-performance.spec.ts on 200-turn fixture, scroll-up capture, Ctrl+F across skipped turns, arrow-key walk, a11y snapshot, menu clipping check. |
| D1-720 | WS09-S10 | P1 |  | M | On a second dev server with VITE_CHAT_VIRTUALIZE=true, scroll a variable-height 1,000-turn thread up and down; record scrollTop jump per re-plan (expect 0 visible jumps). |
| D1-721 | WS09-S10 | P1 |  | M | Run chat-transcript-performance.spec.ts on 300/1000-turn fixtures, VITE_CHAT_VIRTUALIZE true vs off, second dev server :5174; attach JSON and an accessibility-tree capture. |
| D1-734 | WS07-S10 | P1 |  | M | After owner approves 0.50 USD: set LEARNING_REVIEW_MODEL, LEARNING_EVAL_USD_PER_MTOK, LEARNING_EVAL_SUITE_REVISION, new evidence dir; run go test -tags liveeval -run TestLiveSeededCases. |
| D1-743 | WS09-S6 | P1 |  | M | On a rebuilt stack open a 200-turn thread cold with VITE_CHAT_PARALLEL_THREAD_LOAD on and off; diff the resulting turn arrays (expect equal). |
| D1-747 | WS09-S6 | P1 |  | M | Run chat-transcript-performance.spec.ts section D cold and warm, flag off and on, on second dev server; record requests.total, duplicateGetCount, sequentialWaves. |
| D1-784 | WS02-S10.code | P1 |  | M | Enable INFERENCE_OPENAI_PROMPT_CACHE_KEY on staging for the first-party OpenAI route, run repeated same-thread turns, compare cache hit rate against the S8 best run (S2 view). |
| D1-785 | WS02-S10.code | P1 |  | S | From apps/Model Plane/rust: CARGO_INCREMENTAL=0 cargo test -p inference-core --lib openai; quote 'Compiling inference-core' and the 'test result:' line with both named tests passing. |
| D1-786 | WS02-S10.code | P1 |  | S | CARGO_INCREMENTAL=0 cargo check -p inference-core --all-targets from apps/Model Plane/rust; expect no missing-field errors in the five tests/ files. |
| D1-002 | WS09-S1 | P2 |  | S | Run the chat perf harness 3x on a second dev server and write results into chat-perf-budget.json. |
| D1-108 | WS12-S8 | P2 |  | S | After the D1-104 transcript, compare which alias spellings the gateway actually sends and delete the unused aliases (lowering the pin). |
| D1-442 | WS08-S1 | P2 |  | S | On a dev stack with a subscription thread whose history holds an image turn, press retry/regenerate and observe the attachments_not_sent message instead of a silent drop. |

## Still open, other

| Row | Slice | P | Blocks W1 | Effort | Next step |
|---|---|---|---|---|---|
| D1-561 | WS07-S5 | P1 | yes | S | Reload auth-core principals with the scope before rebuilding capability-core; add to deploy runbook. |
| D1-068 | WS01-S9 | P1 |  | M | Review 0044/0048 constraint drops against the previous release's upserts and record the result in migration-policy.md before the switch is set true. |
| D1-084 | WS10-S0 | P1 |  | S | When D1-078 lands, the integrator confirms ADR-0005 and rule 19 as next free numbers (DEC-39). |
| D1-103 | WS12-S8 | P1 |  | S | Build the workspace packages (pnpm -r build) before typecheck, or add a pretypecheck script; environment issue, not a Model Plane defect. |
| D1-156 | WS12-S11 | P1 |  | L | Run size-ratchet.py --check on CI; extract the over-ceiling hunks or get the exception path (D1-157) before blocking phase. |
| D1-160 | WS12-S11 | P1 |  | S | Run size-ratchet.py --init --ref <integration branch> and diff the file set against the committed baseline. |
| D1-167 | WS09-S5 | P1 |  | S | Track under D1-169. |
| D1-172 | WS09-S5 | P1 |  | S | Add VITE_CHUNK_RECOVERY_DISABLED?: string to src/vite-env.d.ts (shared-owner file) or drop. |
| D1-247 | WS12-S12 | P1 |  | S | After first spa-verify CI run, record the measured gzip sizes plus margin in bundle-budget.json. |
| D1-278 | WS05-S6.code | P1 |  | S | See D1-276; apply ledger lead_todo lines once the soak passes. |
| D1-322 | WS02-S2 | P1 |  | S | Run the node action-surface-contract test, and if red hand it to its owning slice. |
| D1-323 | WS02-S2 | P1 |  | S | Add ledger entry for WS02-S2 and mark cost-core 0011 consumed in plan 4.4 (docs only). |
| D1-351 | WS06-S4 | P1 |  | S | none |
| D1-354 | WS02-S11 | P1 |  | S | Run cd apps/Model Plane/rust && cargo test -q -p model-gateway --lib compaction (use Docker rust target per memory) and confirm compile plus pass. |
| D1-365 | WS02-S8.code | P1 |  | S | When the A/B runs, report write share as n/a unless the pinned route reports a cache-creation count. |
| D1-372 | WS01-S14.code | P1 |  | S | See D1-041. |
| D1-376 | WS01-S14.code | P1 |  | S | See D1-041. |
| D1-377 | WS01-S14.code | P1 |  | S | See D1-041 and D1-373. |
| D1-394 | WS06-S6 | P1 |  | S | Run pnpm typecheck and pnpm lint in apps/Frontend Plane/verevonv3 and diff the failing sets against a clean HEAD baseline. |
| D1-465 | WS04-S6 | P1 |  | M | Reconcile on resume by checking the receipt ids against the transcript before resending (use-chat-controller / chat-client). |
| D1-504 | WS10-S4 | P1 |  | S | none |
| D1-556 | WS07-S2 | P1 |  | S | Run buf generate for Python if it is committed, and attach the append-only tag diff vs HEAD memory.proto (Appendix B N2). |
| D1-572 | WS07-S6 | P1 |  | L | Add the gateway body field, BFF route, shared/actions contract and chat-header toggle. |
| D1-593 | WS10-S7.code | P1 |  | S | After step 4 passes on both orgs and DEC-13 is ratified, append MODEL_PLANE_STATUS.md and move ADR-0005 to implemented. |
| D1-602 | WS07-S4 | P1 |  | S | Before flipping fail-closed true: confirm SESSION_ORG_CORE_SERVICE_TOKEN set and no agent-policy 403 in session-core logs; record the order in the rollout note. |
| D1-619 | WS07-S7 | P1 |  | S | none |
| D1-631 | WS07-S3 | P1 |  | S | Run pnpm lint in verevonv3, diff failing set against the pre-slice baseline. |
| D1-645 | WS07-S8 | P1 |  | S | When S10 governance script is written, fail on any unattributed skill_revisions row; confirm capability-core deployed with actor settings. |
| D1-648 | WS07-S8 | P1 |  | S | see D1-646 and D1-622 |
| D1-661 | WS07-S9 | P1 |  | M | Run the script in a scratch env, record the failing set, and fix or ledger-accept the 22 rows. |
| D1-679 | WS02-S12 | P1 |  | S | Make list_todos_by_plan pub(crate) or add org and user parameters so it cannot be called without the owner check. |
| D1-688 | WS04-S8 | P1 |  | M | Add the withdrawn marker (migration plus reader filters) before the retract switch is turned on; blocked on the D1-687 decision. |
| D1-692 | WS04-S8 | P1 |  | M | Requirement to carry into the retract op build: scope by request id, org, user; require_non_zdr_durable_mutation; add ZDR inventory and zdr-posture-matrix rows. |
| D1-724 | WS11-S9 | P1 |  | M | Before either switch goes on, add a capability check or a documented joint flip order with a contract test; hazard is dormant while both are off. |
| D1-735 | WS07-S10 | P1 |  | S | Add the lead's service credential path to the live harness (per-RPC token from env) before running against a guarded stack. |
| D1-736 | WS07-S10 | P1 |  | L | Keep review off for real orgs until WS26 supplies the PII screen; then replace Screen and flip the pii-in-a-proposal case. |
| D1-749 | WS09-S6 | P1 |  | S | Lead adds the WS09-S6 row (built default-off, proof owed) to the plan's Build ledger. |
| D1-762 | WS09-S9 | P1 |  | M | Keep as a known limit; decide in S9 flag-on review whether older pages need citation hydration. |
| D1-008 | WS05-S2 | P2 |  | M | Keep as risk; resolved with D1-017 or a minted token in the D1-005 proof. |
| D1-009 | WS05-S2 | P2 |  | M | Re-read the approve response after the write, or make first-sight insert honour disabled state atomically. |
| D0-017 | WS20-S3 | P2 |  | S | Track under D2-327 and D2-331. |
| D0-018 | cross | P2 |  | S | Optionally add one line to the Model Plane build recipe doc (clean mp-contracts after a proto change on a warm target). |
| D0-023 | cross | P2 |  | S | Close; track remaining WS03-S8 executor half and the Tier B pass in their own rows. |
| D1-029 | WS05-S4 | P2 |  | M | Run a review of the ZDR/waived class learning, merge-style hydrate and fail-closed transport paths. |
| D1-031 | WS05-S4 | P2 |  | L | Stays open until the soak and WS05-S6 land. |
| D1-037 | WS11-S7 | P2 |  | M | WS27/WS15: add an applied_effort field to the usage or step frame so the SPA can show a clamp. |
| D1-065 | WS12-S7.code | P2 |  | S | Locally rename delta to text in the ReasoningDelta payload arm, confirm sse_event_payload_golden and zdr-contract-sync.test.sh fail, revert. |
| D1-107 | WS12-S8 | P2 |  | M | Add golden payload rows for these events when the WS12-S6 orchestration extension lands, then remove them from TRANSPORT_EVENTS. |
| D1-289 | WS04-S5 | P2 |  | S | none |
| D1-397 | WS06-S6 | P2 |  | M | Stream or chunk the export download and show progress when large-org volume warrants it. |
| D1-440 | WS08-S1 | P2 |  | S | Optional: add attachment refusal cases to src/shared/api/zdr-refusals.test.ts; plan marks it live-only, so low value. |
| D1-441 | WS08-S1 | P2 |  | M | Handled in WS08-S2/S4 slices. |
| D1-794 | WS11-S1 | P2 |  | M | Residuals (1)-(3) remain; split if worked. |
| D0-006 | cross | P3 |  | S | Apply the leftover docs lines in a depth pass. |

