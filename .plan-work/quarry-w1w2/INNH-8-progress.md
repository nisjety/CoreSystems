# INNH-8 progress (8a only)

Run of 2026-10-31, lane D, worktree innh-lane-d. Continue from the tree: every step leaves a checkable change. The Edit tool cannot reach this worktree from the verification session, so edits were made with a CRLF-preserving script (`scratchpad/inh8/ed.py`).

- [x] Read INNH-8-status.md, brief, answer_routes.rs, search_routes.rs include_answer block, main.rs pipeline wiring, state.rs, mp_client decode, service_tokens mint errors, zdr_matrix rows `search` and `answer`.
- [x] Decision: ZDR on include_answer = SKIP (status `skipped_zdr`, no pipeline, no model call), because the model hop carries no ZDR posture (ADR-4) and the lead's rule says no content-bearing text goes to a model under ZDR. `/v1/answer` under ZDR is left as it was (its tests pin it); recorded as an owner decision.
- [x] Code: `quarry-edge/src/answer_status.rs` (AnswerPipelineStatus gate order, AnswerReason closed set + exact hints, AnswerStatus, `gate(&AppState)`, `classify_failure`), `AppState.answer_status`, `EdgeConfig.answer_synthesis` (QUARRY_EDGE__ANSWER_SYNTHESIS, default off), main.rs wiring (first closed gate recorded and logged), runtime `mark_model_hop` stamped on the model call of `AnswerPipeline::answer`.
- [x] answer_routes.rs: 501 with `reason` + exact `hint`; 503 `UNAVAILABLE` / `model_plane_auth_refused`; other model-hop failures keep status and gain `reason`; stream error frame carries `reason`.
- [x] search_routes.rs: `answer_status` / `answer_reason` on the response (only when include_answer), ZDR skip, no cache write for a request that asked for an answer and has none (`cacheable`), an answerless cached entry is a miss (`usable_hit`), ANSWER_SYNTH metered once per model call (not on hit / refusal / failure / no_sources / ZDR), `answer_status` in the SEARCH_QUERY metadata.
- [x] Docs: openapi.yaml (/v1/answer, /v1/answer/stream, AnswerRequest/AnswerCitation/AnswerResponse/AnswerErrorBody, SearchResponse fields), route allowlist (two entries removed), CONTRACTS.md, ERROR_REASONS.md, RUNBOOKS.md ("Answer synthesis: enable, disable and the reasons"), SELF_HOST.md, TAVILY_PARITY.md, BILLING.md, both composes + deploy README, docs-contract.test.mjs (3 new tests, ANSWER_SYNTHESIS out of the QB-T17 pending list).
- [x] ZDR matrix rows `search` and `answer` (zdr_matrix.rs), goldens regenerated with QUARRY_UPDATE_GOLDEN=1, CRLF restored, rerun without it.
- [x] Rust tests green in the lane container (counts in INNH-8-status.md); node tests green (51 pass, 12 todo that were already todo).
- [x] Status file, depth file, local commit.
