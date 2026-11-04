# W2-U25-29 status: checkpoint trace_seq, caller policy ask, WS7.D tests (D2-061, D2-051, D2-052)

Lane a, branch `w23/lane-a` (fast-forwarded to `w23/integ` first). Plane: Ingestion (quarry-runtime/edge).

## Built

The checkpoint is JSON (`state` column), so no typed column and no migration were needed. Two additive fields on `AgentRunCheckpoint`, both `serde(default)` and skipped when empty, so old checkpoints load and old edges ignore them.

- D2-061, `trace_seq: u64`: the seq the run's event trace mints next. `EventSink::next_seq(run)` (new, additive) reads it; `RunLive::with_trace` hands the sink to every `checkpoint_for_entry`, so each step, control and budget save stores it. `start_run` opens the trace at `max(trace_seq, 1)` on a resume (1 on a start); the first save is written with `start_seq + 1` because AgentStarted takes the start seq after the save. `finish_run` (expiry and drain only) re-saves the checkpoint with the terminal `agent.expired` seq already spent, while the run is still registered, so a resume cannot read a stale one.
- D2-051, `policy_ask: Option<RequestAsk>`: the ask the run was started with. A resume merges the stored ask with the resume request ask (`agent_policy_gate::merge_asks`, tighten only: disables join, ceilings take the lower, deny lists join, allow lists keep the hosts that pass both, disjoint lists deny everything) and resolves the policy from the merge. The merged ask is what the next checkpoint stores. A resume with no ask keeps the stored one whole.
- D2-052: the in-process sub-resource deny for an unconstrained run already existed (`denied_hosts_are_refused_even_with_no_allow_list_and_under_an_allowed_parent`, U34). Built only the gap: the persisted ask reaching the browser egress (merged deny lists, test below).
- D2-271 (part): `resume_refused_when_a_persisted_profile_is_now_disabled`.

## Files outside the listed scope (compile or correctness needs)

`quarry-runtime/src/events.rs` (`next_seq`), `quarry-edge/src/agent_routes/run_live.rs` (`with_trace`, `with_policy_ask`), `agent_routes/control.rs` (`checkpoint_for_entry` fills the two fields), `agent_routes/close.rs` (the terminal re-save), `agent_routes/tests.rs` (one struct literal). None is claimed by another unit of W2-B2.

## Tests (qtest, lane a)

- `tests/agent_checkpoint_policy.rs` (new): 11 passed. Stored seq grows with the run and equals the open trace next seq; seq continues across expiry+resume (dense, no reuse, past the AgentExpired seq); seq continues after an edge restart (second edge over the same store); a resume does not lower the stored seq; a pre-field checkpoint resumes at seq 1; the checkpoint stores the start ask (and none when none was asked); a resume with no ask keeps the restriction (still denied, no receipt, run not poisoned, ask still stored for the next resume); a resume ask adds to and never replaces the start ask; a restriction survives an edge restart; resume refused when the persisted profile is now disabled; the persisted deny list refuses a sub-resource on a run with no allow list.
- `quarry-edge` lib (4 new `merge_asks` unit tests in agent_policy_gate): 860 passed.
- `quarry-runtime` lib: 1130 passed (checkpoint round-trip and legacy tests updated).
- Regression: agent_resume_step_keys 6, web_policy_agent 7, agent_run_control 35, agent_events_replay 7, run_events_scope 3, agent_landed_page 8, agent_step_timeout 2, agent_run_budget 11 (3 ignored), zdr_refusal_modes 8; all passed.
- Postgres: the round-trip test in `postgres_step_receipts.rs` now also asserts `trace_seq` and `policy_ask`; it is DSN-gated and was NOT run (no DATABASE_URL in the wrapper).

## Flag and default

None (bug and leak fix; no migration needed).

## Assumed decisions

- A resume ask is merged with the stored one (tighten only), not replacing it: the plan says a restriction is never dropped.
- Allow lists merge to the hosts that pass both (a subdomain narrows its parent), not a plain intersection of strings.
- An invalid stored ask (a capability later removed from the catalogue) fails the resume with the same 400 as a bad ask: fail closed.
- Expiry and drain re-save the checkpoint so the terminal event seq is spent; explicit close does not (the checkpoint is closed and not resumable).

## Model Plane asks

None.

## Not verified

- Postgres checkpoint round trip (DSN-gated, not run).
- Live Chromium sub-resource deny (`egress_deny_blocks_a_subresource_on_an_allowed_page`, needs Chromium).
- Not run against a live stack.

## Still unwritten from D2-271

Upload/download refusals (the harness driver lacks the capabilities), `org_allow_narrowing_does_not_break_grant_equality` (needs a grant validator double), the gate-8 `admission_order_is_pinned` (agent_routes/tests.rs). See depth rows.
