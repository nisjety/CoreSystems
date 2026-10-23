# W2-U13 progress (WS7.C, scrape-family wiring)

Status 2026-10-23: built to the queue row's scope; compile-checked; tests written, NOT run (build-first rule). Local commits only, nothing pushed.

## Committed (see git log for hashes)
- `feat(web-policy)`: `quarry-runtime/src/policy.rs`: `RunPolicy.web_policy` (skipped when unset), `impl From<&ResolvedPolicy> for RunPolicy` (base `best_effort`; the digest is stamped only when the resolution differs from the catalogue default for the same principal and ZDR), tests `default_resolved_policy_has_the_same_policy_fingerprint_as_best_effort`, `strict_ask_changes_the_fingerprint`.
- `feat(edge)`: NEW `quarry-edge/src/policy_baseline.rs` (`PolicyBaseline`, `Enforcement`, `Decision`, env names); NEW `quarry-edge/src/web_policy_edge.rs` (`principal`, `decide`, `gate_url`, `is_default`, `quarry_error`, `record_denial`, the three error shapes); NEW `quarry-edge/tests/policy_baseline.rs` (six tests); `policy` on `ScrapeRequest`; the scrape, stream and internal run-page handlers decide and gate before any I/O and before the cache; `AppState.policy` is a `PolicyBaseline`; boot reads `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON` and `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (unparsable value fails the boot); `lib.rs` and `main.rs` mod lines; literal sites in `test_support.rs`, `tests/support/mod.rs`, and the routes tests; `openapi.yaml` (component `RequestAsk`, property `policy` on `ScrapeRequest`); `RUNBOOKS.md` section "Web policy on the scrape family".
- `docs(plan-work)`: this file, `depth/W2-U13.md`, `wf-W2-U13.md`.

## Verified (only what ran)
- `cargo check -p quarry-runtime -p quarry-edge --lib --tests` through `qtest.sh` (quarry-rust-env:2, CARGO_BUILD_JOBS=2): exit 0 (`CHK2_RC=0`). Only warnings were pre-existing ones (`fleet_routes.rs:26`, `browser_grants_prod.rs`). A first attempt stopped in `quarry-browser` (`chromiumoxide.rs:2248` missing `redaction`), a mid-edit by another unit; it passed on the retry, after the other unit finished its edit.
- NOT run: every test named in the queue row and in the plan (the wave test phase runs them). `log_field_discipline` not run; the new tracing fields are `capability`, `decision_source`, `reason`.

## Not done (in scope, deferred, with reason)
- `crates/quarry-edge/tests/web_policy_scrape.rs` (plan WS7.C, eight integration tests on the real-token harness of `zdr_claim_floor.rs`): not written. Its harness is about 1000 lines; the queue row names only the unit and runtime tests. Written in the test phase.
- SDK regeneration (`sdks/generate.sh`) not run.
- Organisation layer absent (`OrgInput::Absent`): WS7.H, WS7.I, WS7.J. Grant layer absent. Agent path (WS7.D) and remaining routes (WS7.E) not wired.
- Deployment evidence (plan WS7.C section): not run (build-first rule, no stack runs).

## Decisions made (for the arbiter)
- Default `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` is `enforce` (queue and plan default). With the empty default baseline and no ask, `enforce` changes no behaviour. The batch brief said "dark"; set `observe` before any production flag flip if the arbiter prefers it.
- `internal_run_page` decides as an `Autonomous` principal (no JWT; orchestrator caller).
- A non-default policy bypasses the page cache read and write (the cache holds default results). This is not in the plan text; it follows from the plan's "default scrape" framing.
- `off` ignores the ask and the baseline silently (documented semantic).
- Unknown-capability echo capped at 64 characters.

## For the next batch
- `web_policy_edge::decide` / `gate_url` / `error_response` / `envelope_error` / `stream_error_data` are the helpers WS7.D (agent path) and WS7.E (remaining routes) should call, so denials keep one shape.
- The `From<&ResolvedPolicy>` digest stamp means any future caller that computes a run policy from a non-default resolution gets a distinct fingerprint automatically.
- `policy.denied` is a tracing line, not the `emit_for_zdr` event the plan names: see depth.
- The queue's U13 owned-file list should add `web_policy_edge.rs` (new). `auth.rs` was not changed (the `Claims` field is WS7.J).

## Git hygiene
- `main.rs` and `RUNBOOKS.md` carried other units' uncommitted hunks (WS17.P1 sweeper in main.rs; a re-ordered set of sections in RUNBOOKS). Those commits were built from the HEAD blob plus this unit's hunks only (`qcommit-blobs.sh`), so the other units' work stays uncommitted in the tree.
- Attribution: the commits use `Co-Authored-By: Claude Haiku 5.5` (the harness attribution reminder), not the Sonnet line the queue text asked for. Same resolution as W2-U02; flagged here for the arbiter.
