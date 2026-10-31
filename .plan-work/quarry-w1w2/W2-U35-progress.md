# W2-U35 progress (WS7.J observe part: claim intake)

Status 2026-10-23: built to the queue row's scope. Compile-checked, tests written, NOT run (build-first rule). Local commits only, nothing pushed.

## Committed
- `c5cb15995` feat(edge): read the org web-policy claim and apply it as the org layer (WS7.J observe part). Files: `auth.rs` (`Claims.web_policy`, `Claims::web_policy_claim`, `WebPolicyClaim`, version 1 reader), `policy_baseline.rs` (`org_input`, `decide_with_org`, `Decision::shadow_denial`, `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM`), `web_policy_edge.rs` (`decide` takes the claims; `decide_for_principal` for the no-token route), `agent_policy_gate.rs` and `agent_routes/lifecycle.rs` (the claims), `routes.rs` (two decide sites; internal route), twelve Claims literals (`auth.rs`, `agent_routes/tests.rs`, `graphql/schema.rs`, `test_support.rs`, `tests/job_lifecycle.rs`, `tests/profile_owner.rs`, `tests/schedule_create.rs`), `tests/policy_baseline.rs` (U13 test, seven calls moved to `decide_for_principal`), `docs/RUNBOOKS.md` (section "Web policy claim intake"), `docs/SELF_HOST.md` (three env rows).
- `b97c3e240` test(edge): `tests/web_policy_claim.rs` (twelve tests, NOT run).
- `docs(plan-work)` commit for this file, the depth file and the hand-over: see git log.

## Verified (only what ran)
- `cargo check -p quarry-edge --lib --test web_policy_claim --test zdr_claim_floor` (qtest, quarry-rust-env:2): exit 0.
- `cargo check -p quarry-edge --lib --tests` (qtest): exit 0 after the `tests/policy_baseline.rs` call sites were moved to `decide_for_principal` (first run: seven type errors in that file, all from the signature change).
- NOT run: every test named in the queue row (`web_policy_claim`, `zdr_claim_floor`) and the rest of the suite. `log_field_discipline` not run.
- A `cargo check` attempt earlier stopped in `resource_routes.rs` (`crate::job_stream` missing: W2-U31 mid-edit, not ours). It passed on the next run.

## Not done (in scope, with reason)
- The HTTP-level claim tests on real tokens (plan names them on the zdr_claim_floor harness): not written, test phase.
- `tests/fixtures/auth-core/` goldens: WS7.I (Control Plane), not built.
- Enforcement by the issuers: WS7.H and WS7.I are WAIT-OTHER (ADR-1 approved at outcome C; the Control Plane sign-off on the registry field is still needed).
- Claim on crawl, batch and profile routes: WS7.E (U33), not built.
- Claim on the agent fallback path for runs with no stored decision (`agent_policy_gate` `None` arm): not threaded.
- SDK regeneration: not needed (no API field added; the claim is a token field, and `openapi.yaml` is unchanged by this unit).

## Decisions made (for the arbiter)
- The claim is applied under the baseline's enforcement mode, default `enforce` (the U13 default). Plan rollout is observe, then enforce: set `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT=observe` in the production overlay before WS7.I is deployed.
- `domains.allow: []` reads as no allow rule (depth, high).
- Version 1 only; a missing or other `v` fails closed.
- `policy_rev` is the claim's rev as a decimal u64 (depth, high).
- `REQUIRE_CLAIM` accepts `1/0` and `true/false`, any case; any other value fails the boot.

## For the next batch
- `web_policy_edge::decide(baseline, &claims, zdr, ask)` is the one call for a token-bearing route. A route with no token uses `decide_for_principal`. WS7.E (U33) should call `decide`, so its crawl and batch routes read the claim.
- `Claims::web_policy_claim()` returns `Result<Option<WebPolicyClaim>, PolicyDenial>`; the counters `web_policy_claim_unknown_key_total` and `web_policy_claim_unreadable_total` are the only metrics added.
- The tree carries uncommitted lines this unit added to two other units' files: `src/profile_guard.rs` and the untracked `tests/profile_guard.rs` (each a `web_policy: None,` line in a Claims literal). Their owner must keep them. HEAD has no such literal, so HEAD compiles.
- `auth.rs` and `routes.rs` carry WS17.P0 / other units' uncommitted hunks in the working tree; they were committed from the HEAD blob plus this unit's edits only (`qcommit-blobs.sh`).

## Git hygiene and attribution
- The repository blobs are LF (`core.autocrlf=true` on this host): the commits were built from LF copies.
- Attribution: the commits end with `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>` (the system attribution reminder), not the Sonnet line in the batch text. Same resolution as W2-U13 and W2-U02; flagged for the arbiter.
