# W2-U67 progress (WS8.F, Quarry attach core, dark)

Status: built to scope for the attach core; the test suites are written and compile-checked, not run (build-first rule). Flag `QUARRY_EDGE__ATTACH` off in every environment.

## Committed (branch quarryv2-next, local only)

- `3b1861652` feat(quarry-runtime): tab and attach fields on step receipts and timeline. Migration `0018_step_receipt_tab_attach.sql` (queue ruling 0.1), `StepReceipt.tab_id/attach_id`, `BrowserTimelineEventKind::{TabControl, Attach}`, `BrowserTabHolder`, `BrowserAttachState`, `EventType::{AgentAttached, AgentDetached}`, NATS tokens, postgres columns.
- `80a346116` feat(quarry-edge): tab ledger, attach registry, attach switch. `agent_routes/tab_ledger.rs` (new: TabLedger, hand_to_agent, is_passive, AttachBudget, AttachState, AttachRegistry, delegate_matches; unit tests), `RunLive.tabs/attaches`, `QUARRY_EDGE__ATTACH` and `QUARRY_EDGE__ATTACH_IDLE_TTL_S` in `run_limits.rs`.
- `10ddd22c6` feat(quarry-edge): attach routes, delegate steps, tab hand-over. `agent_routes/attach.rs` (new: attach_run, release_attach, control_tab, delegate_step, attach_grant_refusal, finish_attach; unit tests), router entries, `step` dispatch to the delegate lane, `StepBody.grant_id`, `tests/attach_cobrowse.rs` (new, four refusal-path tests), postgres round-trip test (ignored).
- `4c76cb5ef` docs(quarry-v2): OpenAPI paths for the three routes and the RUNBOOKS section (patch-based, HEAD base).
- `492b87295` feat(quarry-edge): a person's action takes an agent tab over (`person_tab_gate` on REST and WebSocket paths).

## Verified (what was run)

- `cargo check -p quarry-core -p quarry-runtime --tests` (qtest): rc 0.
- `cargo check -p quarry-edge --tests` (qtest): rc 0, after each code commit (last run after `492b87295`'s edit).
- `cargo check -p quarry-runtime -p quarry-edge --tests` (qtest): rc 0 (includes the integration test target).
- `git apply --check --cached` on the OpenAPI and RUNBOOKS patch: ok.

## Not run

- Every test named in the plan (unit, integration, postgres, live lane). Written, compile-checked only.
- The deployment evidence (plan WS8.F steps 1 to 10): not run; needs the stack, real tokens and the sibling (b) execution-core and model-gateway attach mode (WS8.G).

## Not built (in scope, see depth/W2-U67.md)

- CONTRACTS.md attach text; OpenAPI step-body `grant_id`.
- `list_tabs` filter and holder view for the delegate; projection fields `tabControl`, `attachAvailable`, `attach`.
- Sweeper release of an idle attachment; `finish_run` SessionEnded; `step_seq` lane counter (WS8.H).
- Tab receipt variant `opened_by` / `activated`.
- ZDR feature-matrix rows for the three routes (WS4.G scanner reads `router.rs`; add the rows before the matrix test runs).
- Enabled-path integration test and the I5 allow-list table test (need a driver double with `open_tab`).

## Hand-over traps (for the next batch)

- A `MutexGuard` from `handle.live.attaches()` or `.tabs()` must never live across an `.await`: an `if let` scrutinee keeps it for the whole block (fixed in `delegate_step`). Take the value out first.
- Working-tree files `step_receipts.rs` (tests module), `RUNBOOKS.md` and `openapi.yaml` carry other units' uncommitted hunks. Commits used `qcommit-patch.sh` with hand-filtered hunks; the remaining foreign hunks are still uncommitted and not ours.
- `tests/attach_cobrowse.rs` loads the harness with `#[path = "support/agent_harness.rs"]`, as `human_browse.rs` does.
- The tab gate for a person reads the driver's active tab (`active_tab_id`); with no live attach it returns before calling the driver, so existing runs see no call.
- Commit trailer: the task asked for `Co-Authored-By: Claude Sonnet 5.5`; the commits carry that line as the task text instructs.
