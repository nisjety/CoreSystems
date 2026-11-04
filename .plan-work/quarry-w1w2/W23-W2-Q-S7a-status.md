# W2-Q-S7a status: WS14.S7a ignore-robots override branch and run-binding robots hash

Lane b, branch `w23/lane-b` (already synced to `w23/integ`; not merged or rebased). Plane: Ingestion (edge and orchestrator).

## Built: partly

Checked first: no override existed (`ignore_robots_txt: true` was always a 403 in `parse_body`); the run binding had the W2-U34 `RunBindingExtras` with a policy line only and a placeholder comment for the robots line. Built the gap:

- `crawl_request.rs`: `parse_body` records `ignore_robots_txt: true` as `ParsedCrawl.ignore_robots_txt` (the spelling sent) instead of refusing; 400s still come first. New `robots_override` / `decide_robots_override` (the plan's `require(RobotsIgnore, Human)`): allowed only when the caller is a person (`claims.is_person()` with a user id: never a service principal, the Model or an API key), `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` and `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` are both `enforce`, the org's signed `web_policy` claim is readable with `rev` != 0 and `robots_max = ignore` (`web.robots_override`, WS7.H/WS7.I), the operator baseline has no robots ceiling below `ignore`, and the org is on `QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS` (OQ-11). Any miss is the unchanged 403 `policy_denied` field; the reason is a closed label on `crawl_robots_override_total{outcome,reason}` and a log line. `RobotsOverride::params_value()` = `{mode:"ignore", authority:"web.robots_override", rev:<12 hex>}`.
- `routes.rs` `crawl_handoff`: calls `robots_override` right after the body check (where the old 403 came from); an allowed override sets the scope's robots to `ignore`, stamps `params.robots` as the object (`params.user_id` was already stamped), and the ack carries `effective.robots: "ignore"` plus `effective.robots_override: {authority, rev}`. `internal_run_page` verifies with `RunBindingExtras::for_page(policy_snapshot, crawl)`.
- `internal_auth.rs`: `RunBindingExtras.crawl_robots_sha256` and `for_page`; the canonical string gains `\nrobots=<sha256 of canonical crawl.robots>` after the policy line, only when `crawl.robots` is present, not null and not `"respect"` (respecting pages sign exactly what they did; built on W2-U34's `snapshot_sha256_hex`).
- `activities.go`: `runBindingSigPage` (same rule: `Crawl != nil && Robots != respect`), `runBindingSigExtras` delegates to it, `RunPage` signs with the crawl context, `crawlRobotsSHA256`, and `RobotsModeFromParam` (reads the string or the override object's `mode`).
- Tests written: `crawl_request.rs` unit tests (old 403 unchanged per spelling, allowed with policy and stamps params, refused without entitlement / no claim / unreadable / rev "0", refused for service and API key and empty user, refused unless both modes enforce, operator ceiling, grant list default empty and other org, grant list parsing); `internal_auth.rs` Rust half of the two-sided golden plus respect-unchanged and replay/strip cases; new `activities_robots_binding_test.go` (Go half of the golden, respect-unchanged incl. the 6b golden, ignore vs respect vs empty, RunPage signs an ignoring page, `RobotsModeFromParam`); `tests/crawl_scope_robots.rs` helper fixed plus a 401 replay test.
- Golden: sha256 of `"ignore"` `c571063a...283751`; HMAC robots-only `2a239e08...927c99`; HMAC policy-snapshot + robots `8ca20740...5fc166` (secret `0123456789abcdef0123456789abcdef`, `org_a`, `https://example.com/pricing`), computed with an independent Python implementation, which also reproduced W2-U34's existing golden `e85e8d3b...888a`.

Why partly: the orchestrator's `CrawlInputFromParams` (`workflows/inputs.go`, not this unit's file) still reads `params.robots` with `stringFromParam`, so an override job runs as respect until that one line uses `activities.RobotsModeFromParam` (depth W23-b-S7a-1). Fail-safe: robots.txt keeps being honoured.

## Commits (lane b)
- ce33d702a feat(quarry): run binding covers crawl.robots with a two-sided golden (WS14.S7a)
- aac640844 feat(quarry-edge): ignore-robots override branch on /v1/crawl (WS14.S7a)
- (this file and the depth rows: the docs commit that adds this file)

## Code only, not compiled
No cargo, go or other build/test was run (owner order 2026-11-03). Only `gofmt -l` (formatting, clean) was run on LF copies of the two Go files. Expected commands for the integration step: `qtest -p quarry-edge --lib -- crawl_request internal_auth`, `qtest -p quarry-edge --test crawl_scope_robots`, `go test ./internal/activities` (GOWORK=off).

## Flag and default
`QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (default `enforce`) AND `QUARRY_EDGE__WEB_POLICY_CLAIM_ENFORCEMENT` (default `observe`) must both be `enforce`, AND the new `QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS` (default empty) must list the org. With the defaults the override is refused for everyone, exactly as before (same 403). auth-core's `WEB_POLICY_CLAIM_ENABLED` (default off) mints rev "0", which is refused too. The run-binding robots line is not flagged: it only appears on pages with `robots != respect`, which no job produces before the override is turned on.

## Assumed decisions
- Grantor (OQ-11, platform operators only): an operator-set edge env list of org ids, default empty, on top of org-core's enterprise-only `web.robots_override` (an org admin can raise the org's own ceiling only if the plan allows; only operators can grant the override itself).
- The override binds only when the claim binds the run: both enforcement modes must be `enforce` (claim `observe` is the default, so default off).
- The lattice cannot express an unlock (tighten-only, catalogue robots baseline `respect`), so the plan's `require(RobotsIgnore, Human)` is an explicit unlock rule in `decide_robots_override`; quarry-core was not changed.
- "Refuse any service principal" is `!claims.is_person()`: services and INNH-10 API keys (both autonomous) are refused; a token without a user id is refused too (the audit needs `user_id`).
- `params.robots` becomes the object only for override jobs (plan and S7b contract); other jobs keep the `"respect"` string byte-identical.
- `HandoffAck.effective.robots` stays a string (`"ignore"`) for type stability; `effective.robots_override` = `{authority, rev}`.
- `rev` is the issuer's 12-hex form (`policy_rev_hex`), as the denial envelope uses.
- Run-binding robots line rule: present iff `crawl.robots` present, non-null and not `"respect"`; the value hashed is the canonical JSON of the string (`"ignore"`), not an object (the page's `crawl.robots` is the mode string).
- The Model surface: unchanged; the gateway's `model_surface_drops_external_and_robots_keys` test already pins that no robots key reaches Quarry from the Model, and the Model's service principal is refused at the edge regardless.

## MP asks
None. Nothing appended to MP-ASKS.md.

## Not verified
- Nothing compiled or run (see above), so the unit tests, the golden on both sides, and the 401 replay test are unproven.
- End to end: the orchestrator still runs an override job as respect (inputs.go wiring, depth W23-b-S7a-1); quarry-control's audit-before-ack (W2-CP-S7b) is not built; no live stack, no fixture-a crawl.
- Whether any other edge integration test asserts the exact key set of `effective` or of the job params for an override (none found: no test sends an allowed override).
- OpenAPI, SELF_HOST.md, compose and RUNBOOKS do not mention `QUARRY_EDGE__ROBOTS_OVERRIDE_ORGS` or `effective.robots_override` (depth W23-b-S7a-3, -4).
- `crates/quarry-edge/tests/crawl_scope_robots.rs` was changed outside the listed files because its run_page helper would otherwise fail the new binding (depth W23-b-S7a-6).

## Depth rows
W23-b-S7a-1 to W23-b-S7a-7 in W23-depth-lane-b.md.
