# W2-U39 progress (WS8.B, human-browse semantics, dark)

Status 2026-10-23: BUILT to the queue row's scope (the human branch, flags off, no broker side). Compile-checked on the shared working tree, NOT run (build-first rule). Local commits only, nothing pushed. Owned paths are committed; other units' uncommitted hunks in the same files are left out (see "Left out").

## Committed (hashes at the time of writing; `git log` is authoritative)
- `8603f03c3` feat(browser): open-web egress policy and broker navigation policy. `EgressMode`, the denied list, `open_web`, `canonical_denied_domains`, `BrokerNavPolicy::from_validation` (fail closed), open-web host rules. Hunk-only commit (one other unit's hunk left out).
- `2e3566f54` test(browser): `tests/open_web_policy.rs` (new).
- `d09936203` feat(core): `redaction/action.rs` (new), `redacted_for_record`, plus one `pub mod action;` line in `redaction/mod.rs`.
- `04f5f63e1` feat(runtime): `GrantValidation` gets `policy_mode`, `denied_domains`, `absolute_expires_at`, `resource_id`, and `nav_policy()`. Vendored proto fields 50-53. Broker mapping in `grpc/browser_broker.rs`.
- `2ee29f147` feat(runtime): `AgentRunCheckpoint.policy_mode`, `ReceiptBuilder::complete_metadata_only` and `complete_maybe_metadata`/`fail_maybe_metadata`, `decode_timeline_event_body` and the tolerant Postgres read. Hunk-only commit (one other unit's `segment` hunk left out).
- `62f928a86` feat(edge): `run_limits` switches (`QUARRY_EDGE__HUMAN_BROWSE`, `QUARRY_EDGE__HUMAN_RECEIPT_MODE`, validated in `RunLimits::from_lookup`), `agent_routes/human_browse.rs` (new, pure decisions and 13 unit tests named `human_browse_*` and others), `RunEntry.nav_policy`, admission, step, tabs, control, and the test literals.
- `51101f4a6` test(edge): `tests/human_browse.rs` (new), 5 tests through the agent harness.
- `600671bbf` docs(quarry): openapi 403 text (kept) and a RUNBOOKS section. The RUNBOOKS part was wrong (see below).
- `2f6b026f` docs(quarry): corrects the RUNBOOKS.md damage from 600671bb. The blob is `51101f4a6`'s text plus the WS8.B section only; `git diff --numstat 51101f4a6 HEAD -- RUNBOOKS.md` is `14 0`.
- lifecycle.rs human-browse hunks are in `62f928a86`; the other unit's usage-surface hunks in the same file are not.

## Verified (what was actually run)
- `cargo check -p quarry-core -p quarry-browser -p quarry-runtime -p quarry-edge --tests` through qtest: no errors in the four crates' lib and test targets. The only error was in the bin target (`routes.rs:127`, `crate::schedule_preflight`, another unit's uncommitted `main.rs` hunk), which is outside this unit.
- `cargo test -p quarry-core --lib --no-run`: rc 0.
- `cargo test -p quarry-runtime --lib --no-run`: rc 0.
- `cargo test -p quarry-browser --test open_web_policy --no-run`: rc 0.
- `cargo test -p quarry-edge --lib --no-run`: rc 0.
- `cargo check -p quarry-edge --features browser-agent --test human_browse --test start_run_grants --test zdr_refusal_modes --test grant_budget_claim --test run_sweeper_integration`: rc 0.
- Each commit went through qcommit / qcommit-patch and the gitleaks pre-commit hook: "no leaks found".
- NOT run: every test named in the queue row and the plan (see not_done). Compile checks were on the shared working tree, which holds other units' uncommitted edits; the committed tree was not compiled on its own.

## Not done (in scope; each reason is in the depth file)
- Deployment evidence (plan WS8.B DoD and "Deployment evidence"): not run.
- The plan's test commands (`open_web`, `--test ssrf`, `browser_egress_boundary`), and the `open_web_policy_still_blocks_private_and_metadata_hosts` addition to the two existing files: not run / not added.
- Integration tests for resume with a mismatched mode, receipt modes end to end, the budget exemption, and changed denied lists: not written (the pure functions are unit-tested).
- Human run lifetime (idle 900 s, takeover 1800 s, absolute 4 h): not implemented; the run uses the default TTL. Needs WS8.C and the WS2.D Interactive class.
- The broker side (WS8.C): not built. Quarry's vendored proto numbers are the plan's; Model Plane must match.

## Left out (other units' uncommitted hunks, not committed by this unit)
- quarry-browser/src/lib.rs: the cfg line before `pub mod capture_plan`.
- quarry-runtime/src/step_receipts.rs: `segment` on `BudgetCheckpoint` and `is_first_segment`.
- quarry-edge/src/agent_routes/lifecycle.rs: `headers`, `surface` and `with_surface` (WS6.C-e usage surface).
- quarry-edge/src/agent_routes/close.rs, quarry-runtime/src/lib.rs, quarry-runtime/src/usage.rs, quarry-core/src/policy.rs, quarry-core/tests/contracts.rs, the routes.rs, main.rs and lib.rs edits, and the untracked files usage_attrs.rs, otel_zdr.rs, schedule_preflight.rs, internal_routes.rs.

## For the next batch
- WS8.F (attach) builds on this: it needs the tolerant decode's known-kind list updated with its kinds, the `max_steps = 0` reader check, and `agent_step_refused_when_tab_holder_is_human`. Its migration is 0018 per the queue ruling.
- `RUNBOOKS.md` in the working tree still lacks five committed sections (see the depth file). Whoever next commits that file must re-sync it first.
- Commit attribution: the commits end with `Co-Authored-By: Claude Haiku 5.5`, per this session's instruction. The workflow text asked for Sonnet 5.5. Confirm which the owner wants.
