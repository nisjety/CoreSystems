# W2-CP-U35 status: WS7.J claim intake, enforce-ready (lane d, branch w23/lane-d)

## Built
All of the scope, CODE ONLY. Checked first: most of WS7.J already existed from W2-U35 (c5cb15995, b97c3e240): `Claims::web_policy_claim` (version 1 reader, unknown keys counted in `web_policy_claim_unknown_key_total`, unreadable counted and fail closed), `PolicyBaseline::org_input`/`decide_with_org`, `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM`, empty `domains.allow` read as no allow rule (auth.rs), and the plan's ten tests. Built only the gap:

- `src/policy_baseline.rs`: new `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` (`CLAIM_ENFORCEMENT_ENV`, off/observe/enforce, default observe, strict parse that fails the boot), `claim_mode()`. Under the web-policy mode `enforce` with the claim observed, the run resolves without the org layer and the full resolution (with the claim) is the shadow; an unreadable or missing required claim is the shadow denial (counted, never refused). Claim mode `off` drops the org layer and the requirement. An org input with no effect (absent, or the `{"v":1,"rev":"0"}` empty layer) gets no shadow. Module docs state D2-053 and D2-054. Two unit tests.
- `src/web_policy_edge.rs`: `policy_rev_hex(u64)` (`"0"` or 12 lowercase hex, zero-padded) and `quarry_error` rewrites `details.policy_rev` to it (D2-054: the envelope's rev equals the stored org-core rev as a string). `gate_url`/`gate_urls` count a would-be denial only for a URL the run itself lets through (otherwise the observed-claim shadow double-counts every baseline denial).
- `tests/web_policy_claim.rs`: the ten plan tests now pin claim mode `enforce` (the after-flip state); `claim_rev_is_in_the_denial_details` also asserts the wire form `"a1b2c3d4e5f6"`. Eleven new tests: `claim_is_observed_by_default`, `enforce_is_one_flip_of_the_claim_mode`, `claim_mode_parses_strictly`, `observed_claim_counts_an_unreadable_or_missing_required_claim_and_never_refuses`, `claim_mode_off_ignores_the_claim_and_the_requirement`, `baseline_claim_needs_no_shadow`, `empty_allow_list_is_no_allow_restriction`, `non_empty_allow_list_binds_autonomous_runs`, `observed_claim_never_refuses_a_url`, `policy_rev_is_the_twelve_hex_digest_on_the_wire`, `unknown_top_level_and_domain_keys_are_counted`.

`tests/zdr_claim_floor.rs` untouched (DoD: green and unchanged).

## Commits (lane d)
- d1ae10523 feat(quarry-edge): claim mode observe by default, policy_rev as 12 hex on the wire (W2-CP-U35)
- 3c80a6e6c test(quarry-edge): claim observe default, off mode, D2-053 and D2-054 cases (W2-CP-U35)
- (this file and the depth rows: the docs commit that adds this file)

## Code only, not compiled
No cargo check or test was run (owner order 2026-11-03). Integration step: `qtest -p quarry-edge --lib policy_baseline` and `qtest -p quarry-edge --test web_policy_claim --test zdr_claim_floor`; also `--test policy_baseline --test web_policy_agent --test web_policy_backstop --test web_policy_routes` (they share `Decision` and `gate_url`).

## Flag and default
`QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT`, default `observe`: the claim never binds a run; would-be denials count as `web_policy_would_deny_total{decision_source="org_policy"}`. Enforce = set it to `enforce` after WS7.H and WS7.I are deployed (one env flip). `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` keeps its U13 default `enforce` for the baseline and the ask (unchanged); under its `observe`/`off` the claim is shadow-only/ignored as before.

## Assumed decisions
- "Observe stays the default" is a separate claim mode, not a change of the global `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` default (that would also relax the operator baseline and the request ask, and break the U13 tests).
- D2-053 (queue decision 25): empty `domains.allow` = no allow-list restriction; an absent `domains` reads the same. Behaviour was already so; now documented and tested.
- D2-054 (queue decision 25): wire form is `"0"` or 12 lowercase hex, zero-padded, from the core's u64 (no quarry-core change; the core and its tests keep the number internally).
- Claim mode `off` also ignores `REQUIRE_CLAIM`.
- Under claim observe a missing required claim is counted, not refused (matches the REQUIRE_CLAIM doc: "under observe the refusal is only counted").

## MP asks
None.

## Not verified
- Nothing compiled or run.
- That `PolicyLayer::new` with nothing in it equals `PolicyLayer::default()` (the empty-allow and unknown-key tests assert it; the core doc says equal meaning compares equal).
- Interplay with the durable-job snapshot under `QUARRY_EDGE__WEB_POLICY_SNAPSHOT=enforce` (depth row: an observed claim would be stamped and bind pages).
- No HTTP-level test with real tokens (plan's evidence step needs WS7.H/WS7.I deployed).
- Docs (SELF_HOST.md, RUNBOOKS.md) not updated: outside the unit's files (depth row).
