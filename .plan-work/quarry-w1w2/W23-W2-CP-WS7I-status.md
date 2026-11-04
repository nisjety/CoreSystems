# W2-CP-WS7I status: auth-core web_policy claim minting (lane d, branch w23/lane-d)

## Built
All of the scope, CODE ONLY. Checked first: nothing of WS7.I existed in auth-core (no `web-policy.ts`, no `web_policy` claim, no flag). Already present and reused: the registry fields and `webPolicyPostureFor` (W2-CP-R13-1), org-core's internal route and its contract golden (W2-CP-WS7H), and the edge's claim reader (`Claims.web_policy`, WS7.J side).

- `apps/Control Plane/auth-core/src/auth/web-policy.ts` [NEW]: `resolveWebPolicy(orgId)` calls org-core `GET /internal/orgs/:orgId/web-policy` with `x-service-id: auth-core` and `ORG_CORE_SERVICE_TOKEN` via `requiredScopedServiceToken`; 45 s fresh cache, 15 min stale-if-error measured from the last read, no failure caching, concurrent reads shared, bounded memory; `WebPolicyUnavailableError` 503 `web_policy_unavailable` with nothing remembered; a 404 is an error (`org_core_not_found`), never an empty policy (W2-B3); `webPolicyClaimFromOrgCore` builds `{v:1, rev, caps?, robots_max?, proxy_max?, domains?}` (sorted caps, empty containers dropped, malformed answer = 503); 2 KiB cap `WebPolicyTooLargeError` 503 `web_policy_too_large` (checked before caching); `webPolicyClaimFor(audience, orgId)` (undefined off quarry/browser-broker, `{v:1,rev:"0"}` with the flag off).
- `convex-token.service.ts` [C]: `PlaneJwtClaims.webPolicy?`; `issuePlaneToken` signs `web_policy` right after `zdr` for `quarry` and `browser-broker` only (user tokens always: baseline when none given; service tokens only when passed); refuses the claim on any other audience; size cap enforced again at signing; optional `WEB_POLICY_TOKEN_TTL_CAP_SECONDS` (unset = audience TTL unchanged). `zdr` logic untouched.
- `plane-token.controller.ts` [C]: `getToken` resolves the claim after the retention posture; `issueInternalToken` resolves it after authorization, binding and retention when `webPolicyPostureFor(registry, serviceId, audience) === 'tenant'` (the registry is loaded once per request now); audit `plane_service_token_issued` gains `details.web_policy_rev` only on tokens that carry the claim (so the existing exact-details test stays valid).
- `web-policy.spec.ts` [NEW]: resolver (request shape, org-core contract golden replay incl. its 404, no-policy rev "0", field whitelist, 6 malformed answers, fresh 45 s, shared read, 503 down, 500/401/403/404 as errors, bad JSON, missing credential, bad org id, `stale_policy_served_for_15_minutes_then_503`, stale on 404, `failure_is_never_cached`, `claim_over_2KiB_fails_closed`, exact 2 KiB), flag (`0`/empty/false/off/absent = baseline, no fetch), real controller + real signer (`user_token_carries_the_claim_for_quarry_and_browser_broker_only`, `claim_is_always_present_on_those_audiences` incl. the onboarding token, `service_token_carries_the_claim_only_for_tenant_rows` + audit rev, tenant row with flag off, `zdr_claim_is_byte_identical_before_and_after`, `org_core_down_with_nothing_remembered_is_503_and_no_token`, 404 = no token, `service_token_ttl_unchanged_without_the_cap`, the TTL cap, signer refusals), catalogue copy pin and byte-compare.
- `fixtures/web-policy-catalogue.v1.json` + `.sha256` [NEW]: byte copy of `quarry-core/contracts/web-policy-catalogue.v1.json` (sha 998c8034...).
- `quarry-token-zdr-claims.golden.spec.ts` [C]: mints with `WEB_POLICY_CLAIM_ENABLED=1` and an org-core double that also answers the web-policy route (one org with the org-core golden's policy, all others rev "0"); four new cases; two new tests (claim beside zdr, zdr unchanged; every golden case carries v1).
- `fixtures/quarry-token-claims.golden.json` + `.sha256` and the Quarry edge copy `crates/quarry-edge/tests/fixtures/auth-core/` [C]: regenerated, sha256 `76cc4af7f2a2a4f2730265d19f8ec4b26dd181b048e9135b66e8369b3ae8c266`. Existing cases gain `{"v":1,"rev":"0"}`; new cases `user_in_org_with_web_policy`, `user_in_org_without_policy`, `service_execution_core_with_web_policy`, `service_model_gateway_with_web_policy`.
- `scripts/tests/zdr-contract-sync.test.sh` [M, outside the listed files but required by the regenerated golden]: GOLDENS digest updated; VENDORED rows for the auth-core catalogue copy and its pin.

Not touched: `plane-service-principals-policy.contract.spec.ts` (R13-1 already covers `registry_refuses_tenant_for_any_other_audience` in "refuses the whole registry for a web policy on any other audience..."), anything under apps/Model Plane.

## Commits (lane d)
- f345b781a feat(auth-core): web policy resolver for the web_policy token claim (WS7.I)
- 3f993789d feat(auth-core): mint web_policy beside zdr on quarry and browser-broker tokens (WS7.I)
- 752badfd5 test(auth-core): regenerate the quarry token claims golden with web_policy (WS7.I)
- (this file, MP ask and depth rows: the docs commit that adds this file)

## Code only, not compiled
No jest, tsc, cargo or bash test was run (owner order 2026-11-03). The golden bytes were produced by a Python reproduction of the spec's mint (format parity checked against the committed golden: `json.dumps(indent=2)` re-serializes the old file byte-identically), not by `UPDATE_GOLDENS=1 npx jest`.

## Flag and default
`WEB_POLICY_CLAIM_ENABLED` (auth-core), default off: every user token for quarry/browser-broker and every tenant-row service token carries `{"v":1,"rev":"0"}` and org-core is never asked. Turn on after org-core WS7.H is deployed; then Quarry WS7.J observe, then enforce. Optional `WEB_POLICY_TOKEN_TTL_CAP_SECONDS`, unset.

## Assumed decisions
- Service tokens for quarry/browser-broker carry the claim only for `webPolicyByAudience` tenant rows (absent otherwise); user tokens always carry it, including the pre-org onboarding token (baseline). Depth row on the edge's require-claim flag.
- A tenant row with the flag off carries the baseline `{"v":1,"rev":"0"}` (the flag "yields {v:1,rev:0} for everyone").
- 404 from the web-policy route: lookup failure `org_core_not_found`; stale-if-error applies like any failure (a remembered org gets its stale policy for 15 min), otherwise 503.
- `web_policy_too_large` is a 503 (fails closed, no token), not cached.
- The claim carries only `caps`, `robots_max`, `proxy_max`, `domains` from org-core's EFFECTIVE `policy`, as org-core sends them (org-core already omits baseline entries); caps sorted by key; empty containers dropped; `rev` must be "0" or 12 lower-case hex.
- Lookup timeout 3 s (org-core's internal handler timeout); credential and base URL exactly as the retention resolver.
- Audit `web_policy_rev` only when the token carries the claim (no key otherwise).
- `WEB_POLICY_TOKEN_TTL_CAP_SECONDS` applies to any token that carries the claim, floor 60 s.
- Golden fixture orgs: `org-web-policy-golden` (org-core golden's answer: rev `0123456789ab`, upload disabled, robots respect), `org-without-web-policy-golden`.

## MP asks
Appended to `.plan-work/quarry-w1w2/MP-ASKS.md` (section W2-CP-WS7I): re-vendor the model-gateway copy of the golden and its pin (exact digest and the byte-level change), confirm model-gateway's auth test tolerates `web_policy`, and the broker verifier must tolerate the claim. Until then `scripts/tests/zdr-contract-sync.test.sh` fails on the two model-gateway VENDORED rows (expected).

## Not verified
- Nothing compiled or run: `npx jest quarry-token-zdr-claims web-policy plane-service-principals-policy`, `bash scripts/tests/zdr-contract-sync.test.sh` (expected red on the MP rows until the ask lands), `qtest -p quarry-edge --test zdr_claim_floor`.
- That the real mint reproduces the golden bytes exactly (see Code only).
- Whether other auth-core specs assert the full payload or audit details of a quarry/browser-broker token (none found by grep; the exact-details test is data-plane).
- No live mint, no deploy, no org-core round trip.
