# W2-U29 progress (WS7.D, agent-path web policy)

Status 2026-10-23: BUILT to the queue row's scope and compile-checked (cargo check -p quarry-edge --lib --tests, rc=0, on the shared working tree). Tests written, NOT run (build-first rule). Local commits only, nothing pushed.

## Committed (hashes at the time of writing; `git log` is authoritative)
- `98faa937c` feat(edge): the two NEW files: `crates/quarry-edge/src/agent_policy_gate.rs` (resolve_for_start, egress_policy, gate_policy, gate_visit, check_action, gate_url; four unit tests) and `crates/quarry-edge/tests/web_policy_agent.rs` (seven integration tests). Committed first, on their own, so the wiring commit could reference them.
- `7a58c6e95` feat(edge): the wiring. `run_live.rs` (`policy` field on RunLive, `with_policy`, `policy()`), `types.rs` (`StartRunBody.policy: Option<RequestAsk>`), `lifecycle.rs` (resolve before the persistent-profile check; egress narrowed; recovery URL gated; `with_policy` on RunLive), `admission.rs` (gate 8 of R-7), `step.rs` (landed-URL check in `step` and `execute_ws_action`), `tabs.rs` (new_tab URL), `lib.rs` and `main.rs` (feature-gated `mod agent_policy_gate`).
- `1eb88a6ff` fix(edge): the same wiring stored as LF blobs. 7a58c6e95 had written the CRLF working-copy bytes of admission.rs, step.rs, tabs.rs and types.rs, so those four files were rewritten whole. The fix stores them as LF, with the repo convention (WS7.C precedent `b33379cb6`).
- `1935c2c8f` fix(edge): restores three WS10.S3 hunks that 1eb88a6ff dropped (the `trace_config` mod line, the ring bounds and `with_ring_limits` in main.rs, and `open_run` in lifecycle.rs). Those files are restored to their exact 0d05adc6c blobs. Net content against 0d05adc6c is unchanged. This was my mistake: my HEAD copies predated the other unit's commit 0d05adc6c. Every later commit was built from fresh HEAD copies.
- `8eabdd77b` docs(quarry): `docs/openapi.yaml` (additive: `policy` on the start-run request via allOf RequestAsk; 403 text names policy_denied) and `docs/RUNBOOKS.md` (append-only section for WS7.D).
- This progress file and `depth/W2-U29.md` (committed in the plan-work commit that follows).

## Not done (in scope; each reason is in the depth file)
- Upload and download refusal tests: the harness driver does not advertise `uploads_from_artifacts` / `downloads_to_artifacts`, so gate 5 answers first. Test phase: add a capability-enabled double, then write `denied_upload_returns_403_and_leaves_the_approval_grant_unused` and `denied_download_same`.
- `resume_refused_when_a_persisted_profile_is_now_disabled`: needs a durable checkpoint of a persisted profile.
- `org_allow_narrowing_does_not_break_grant_equality`: needs a grant validator double with `require_browser_grants=true`.
- `admission_order_is_pinned` extension for gate 8 (the existing test in `agent_routes/tests.rs` is unchanged; the integration tests cover the order).
- Deployment evidence (plan WS7.D section): not run.
- Gateway forwarding of the `policy` field (H10 owners) is unverified; see depth.

## Verified (only what ran)
- `cargo check -p quarry-edge --lib --tests` through qtest (quarry-rust-env:2, CARGO_BUILD_JOBS=2): rc=0 on the shared working tree (which also held other units' uncommitted hunks). Only pre-existing warnings (`fleet_routes.rs:26` unused import, `run_live.rs:406` deprecated `fetch_update` in pre-existing code, `browser_grants_prod.rs`, `profile_guard.rs`). None from the new code.
- After the commits: the CR-insensitive content diff of the three restored files against 0d05adc6c is empty; the docs commit is 28 insertions and 1 deletion.
- NOT re-run at HEAD: a `cargo check` of the committed tree (the queue was busy). The committed content of my files equals the checked working-tree content, apart from other units' hunks I did not commit.
- NOT run: every test named in the queue row and the plan.

## For the next batch
- Gate 8 entry point: `agent_policy_gate::gate_policy(state, &handle.live, zdr, action)`. Start-time: `agent_policy_gate::resolve_for_start(...)`. Egress: `agent_policy_gate::egress_policy(&decision, allowed)`. Host rules for pages and tabs: `agent_policy_gate::gate_visit`.
- The decision lives on `RunLive` (`with_policy` / `policy()`), not on `RunEntry`. A run built outside `start_run` is gated as the autonomous principal under the baseline.
- WS7.G (6a) will add the egress deny list and the pre-navigation final-URL preflight; WS7.E will reuse `web_policy_edge` helpers for the remaining routes.
- Resumed runs drop the caller's ask (high in the depth file). Fix needs a checkpoint field, owned by the WS2.C checkpoint code.

## Git hygiene
- Commit helpers used: `qcommit-blobs.sh` (hunk-only, from HEAD copies) and `qcommit.sh` (new files). `qcommit-blobs.sh` cannot add new paths (`update-index --cacheinfo` without `--add`); new files went in through `qcommit.sh`.
- Attribution: the commits end `Co-Authored-By: Claude Haiku 5.5`, as the system attribution reminder asks. The unit text asked for a Sonnet line. Same resolution as W2-U13 and W2-U02; flagged for the arbiter.
- A stale private index file from the failed first blob attempt (`.git/u8-index.<pid>`) may remain; it is not in any commit.
