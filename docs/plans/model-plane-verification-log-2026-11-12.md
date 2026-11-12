# Model Plane verification log (branch mp-w345, from 2026-11-12)

Owner order 2026-11-12: verify the code phase in the order of `model-plane-verification-plan-2026-11-11.md`.
Loop per bug: test (Opus 5.5, medium; scripts and code reading) -> fix (Haiku 5.5, max) -> verify (Opus 5.5, high;
re-run, then look for related bugs: sibling call sites, mirrored Go/Rust/TS/proto copies, same pattern in the batch,
ZDR/authz paths) -> continue testing. Only verified fixes are committed (`fix(scope): ...`). No merge, no push.
Pre-existing failures outside the Model Plane are recorded with the base diff (base `031e7965d`), not chased.

## Bugs

| # | Found by (script / code read) | Where | Bug | Fix commit | Verifier verdict | Related bugs |
|---|---|---|---|---|---|---|
| T1 | script: cargo test --workspace --lib | execution-core runtime_loop/agent.rs ~17274 | a_child_loop_stop_does_not_stop_the_parent overflows the 2 MB debug test stack (deeper nested future after the branch grew agent.rs); passes with RUST_MIN_STACK=16M | open | - | - |
| T2 | script | execution-core fold.rs ~2806 (test) | new input-pause test builds FoldWriter with no bearer, so the writer is Off (test bug, W2-WS14-S5) | open | - | - |
| T3 | script | model-gateway sse.rs ~23718 (test) | stale source-text guard after new approval clients in state.rs (wiring correct) | open | - | - |
| T4 | script | model-gateway sse.rs ~27262 (test) | assertion expects comment lines that code_of() strips (test bug, F-f06-19) | open | - | - |
| T5 | script | model-gateway http_routes.rs ~363; space-surface.v1.json | goal-judge route registered with a const path (scanner sees a computed route) and has no space-surface ZDR class row | 80193870b | PASS (Opus high): literal judge path + equality test; space-surface rows for the judge (refuse), approval-grants DELETE (n/a, like sibling narrowing deletes), rule-proposal confirm/dismiss (refuse); re-pin byte-exact, zdr-contract-sync 197/0, space vectors 5/0 | - |
| T6 | script | model-gateway zdr_posture.rs CALL_SITES | thread_goal.rs judge_goal_turn infer call site not listed (ZDR path checked: no leak) | 80193870b | PASS (Opus high): CALL_SITES row matches the real routes | - |
| T7 | script | mp-contracts tests/loop_fold_golden.rs ~356 | pinned park key set lacks connector_taint (always serialised now) | open | - | - |
| T8 | script | session-core routing_policy_grpc.rs ~305 | ZDR refusal built as untyped Status, not from the zdr_refusal catalogue | 80193870b | PASS (Opus high): typed catalogue refusal; zdr_refusal_inventory 3/0, routing_policy bin 11/0 | - |
| T9 | script | inference-core metrics.rs (class label), session-core write_policy.rs ~156 (claimed label) | new metric labels outside the closed label list (other offences pre-existing) | open | - | - |
| T10 | script | execution-core tests/approval_gate_contract.rs | stale guard: cause now filtered on durable_forbidden (stricter) | open | - | - |
| T11a | script + code read | execution-core runtime_loop/resume.rs ~2223 | same-claim QUESTION continuation replay accepted without replay_is_fenced (no effect-ledger fence) | (this commit) | PASS (Opus high): same fence predicate as consume_for_resume; durable_loop_contract 34/0, lib resume 132/0; in practice defence in depth (the fold claim is the real fence) | R1-R5 below |
| T11b | script | model-gateway sse.rs CONTINUE_FRAME_REFUSALS / CONTINUE_EXECUTOR_MESSAGE_TOKENS | claim was: question_answer_released and question_expired not listed | no change needed | PASS: question refusals route through CONTINUE_QUESTION_EXECUTOR_TOKENS (sse.rs ~22339), already listed; frames reach the SPA typed | - |
| T11c | script | execution-core tests/durable_loop_contract.rs | stale text guards after WS14-S5 / FU-01 / FU-25 / D2-1117 | (this commit) | PASS: relaxations count exactly and are scoped to named functions; mutations still go red | - |
| R1 | verifier (T11) | execution-core resume.rs ~1066/~1920 (select_stage, question_stage_decision), fold.rs ~185 | a ZDR or waived run with resume, checkpoint or ledger switch off is classified as that switch off and its question answer is RELEASED (client resends) instead of refused durable_mutation_forbidden | (this commit) | PASS (Opus high): catalogue durable_mutation_forbidden before any release/claim/spend/fold write; old code released in 18/24 grid cases, fixed refuses 24/24, non-ZDR unchanged; lib resume 133/0, durable_loop_contract 34/0. Defence in depth: ZDR/waived runs are never offered ask_user and session-core refuses ZDR callers | R6 |
| R2 | verifier (T11) | execution-core browser_session.rs ~775 skip_site, agent.rs ~12069 | skipped-site continuation has no atomic single-use spend: is_held_for_person then end_hold take the lock separately, so two concurrent skips can both continue | open | - | low-medium |
| R3 | verifier (T11) | execution-core agent.rs ~1886 + SPA QUESTION_GONE_CODES | lost question_answer_released frame + same-claim retry answers continuation_already_consumed, the SPA reads "gone", answer lost silently | open | - | low (pre-existing design) |
| R4 | verifier (T11) | execution-core resume.rs ~2199 | no unit table for classify_question_spend, no integration tests for begin_question_continuation / release_question_answer | open | - | test gap |
| R5 | verifier (T11) | model-gateway sse.rs ~22339 CONTINUE_QUESTION_EXECUTOR_TOKENS | authority_widened / authority_space_mismatch / authority_route_mismatch / continue_space_scope_unsupported not listed (degrade to continue_refused); run_ended listed but never sent | open | - | low |
| R6 | verifier (R1) | execution-core park.rs ~620 / agent.rs ~1326, ~2556 | a waived caller takes the park claim lease before run_agent_with_offered refuses it (no data written; parked row stays claimed until the lease lapses); also label-only switch-first orders in begin_continuation, WriterGate::decide and the gateway question form (no data effect) | open | - | low |
| R7 | verifier (T13-T6) | model-gateway sse.rs ~15318 judge_route | attended goal judge omits model_connection_id; unreachable today (normalize refuses agentic turns on customer-gateway connections) but a rule-17 / W4-BYO-6 leak if that changes; pin with a test | open | - | low (latent) |
| R8 | verifier (T13-T6) | model-gateway side_chat_routes.rs ~245 (also reasoning_translate_routes.rs ~197, council, review_run) | side chat sends a customer-gateway or subscription thread transcript to the platform chain; dark while MODEL_GATEWAY_BYO_GATEWAY is off; owner policy: inherit the thread connection or refuse | open | - | medium-low |
| T12 | script | execution-core tests/tool_class_registry_parity.rs ~98 | migration list misses capability-core 0041 (seed exists) | open | - | - |
| T13 | script | model-gateway http_routes.rs ~12183; ZDR field spec | request field model_connection_id has no ZDR field-table row (spec gap); knowledge_receipt order check is a text false positive | 80193870b | PASS (Opus high): model_connection_id row matches runtime (connection_named_by), speed inert under ZDR, receipt guard allows only agentic_knowledge_frame; zdr_feature_table 17/0, lib zdr 184/0 | - |
| T14 | script | model-gateway tests/agent_mode_resolution_test.rs | stale guard: resolve_turn moved into resolve_turn_with_agent_cap (order still correct) | open | - | - |
| T15 | script | model-gateway tests/infer_purpose_inventory.rs; sse.rs ~15318 | judge-route InferRequest without purpose needs an exemption (real request sets Validation) | open | - | - |

## Pre-existing failures (base diff)

| Suite | Failure | Also fails at base 031e7965d? | Note |
|---|---|---|---|
| execution-core lib | sandbox an_egress_call_covers_the_socket_root... (Windows path separators) | yes (untouched) | P1 |
| execution-core | browser_planner_hermetic (4), browser_challenge_rule_contract | yes (untouched) | P2 |
| execution-core, model-gateway | metric_label_guard offences planner/class/editor/cause/trust/agrees | yes | P3 |
| model-gateway | offline_downstreams_contract (2) | yes (known, plan 2.5; client count now 38) | P4 |
| model-gateway | orchestration_http_test decide_approval_grant_* (2) | yes (code unchanged) | P5 |
| execution-core | tool_retry_contract both_loops_exclude... | yes | P6 |
| model-gateway | loop_guard_inline_test query normaliser | yes | P7 |
| scripts | action-surface-contract.test.mjs (8 dotted literals, C-13) | yes | P8 |
| scripts | ownership-docs-contract.test.mjs (21) | yes (docs untracked) | P9 |
| scripts | browser-grant-contract.test.mjs (5) | yes | P10 |

Passed: zdr-contract-sync 197/0, model-entitlement-contract-sync 5/0, space-contract-vectors-sync 5/0, control-train-repin; lib tests of inference-core (677), mp-contracts (457), mp-orchestration (66) and the small mp-* crates; model-gateway lib 3570 passed / 4 failed (T3-T6); execution-core lib 2189 passed / 2 failed with RUST_MIN_STACK=16M.
