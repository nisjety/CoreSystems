# W2-CP-L1 status: billing-core Quarry limits read API for the quarry-edge credential (U43 Control half; lane a, branch w23/lane-a)

## Built
All of the scope, code only, not compiled. Checked first: nothing of it existed. The quarry-edge credential row (`billing:quota:read:any`, `billing:entitlement:read:any`, token `QUARRY_BILLING_CORE_SERVICE_TOKEN`) is already in Control compose from W2-CP-R13-2, and quarry-edge is the only holder of `billing:quota:read:any` there. billing-core had no `Configured` notion, no `quarry.cap.*` keys and no Quarry-specific read. No compose edit.

- `internal/billing/service.go`: `Service.QuarryLimits(ctx, org, now)` and the types `QuarryLimits`, `QuarryAllowance`, `QuarryPlanCaps`, metric and cap key constants, placeholder `quarryPlanCapDefaultsByPlan` (the plan's numbers), and pure helpers `QuarryAllowanceFromReports`, `QuarryAllowanceFromPlanQuota`, `QuarryAllowanceUnconfigured`, `QuarryPlanCapsFor`. The read never creates an account (`account: "absent"` instead; the generic `GetQuotaStatus` creates one). Allowance rules are `GetQuotaStatus`'s: a not-yet-started policy is skipped, an ended or paused one refuses (remaining 0, exhausted). Unconfigured is `configured: false, remaining: null`, never zero. Plan quota keys use all-time usage (as today); a negative quota limit is `unlimited`. A tombstoned org surfaces `ErrOrganizationDeleted` through `repo.Allowances`.
- `internal/http/quota_handler.go` (new): `GET /api/v1/billing/orgs/:orgId/quarry/limits`. Checks: middleware scope `billing:quota:read:any`; principal pinned to `quarry-edge` (403 `principal_not_allowed`); org binding header `X-Quarry-Org-Id` must be exactly one value equal to the path org (400 `org_binding_required`, 403 `foreign_org`), checked before any read; a snapshot whose `org_id` differs from the path is never served (500 `limits_mismatch`). 200 `{data: QuarryLimits}` with `Cache-Control: private, max-age=30`; errors `{error:{code,message}}` with `no-store`; store failure 503 `limits_unavailable` (the edge applies its fail mode); tombstone 403 `organization_inactive`; invalid org 400 `invalid_org`. The handler reads through a `quarryLimitsReader` interface (fake in tests, no DB).
- `internal/http/server.go`: `Server.quarryLimits` field (wired to the service when non-nil); route registered next to the quotas route.
- `internal/http/service_auth.go`: the route maps to `billing:quota:read:any` only (no `:self`, so no gateway delegation reaches it; non-GET maps to nothing).
- `internal/http/quota_handler_test.go` (new), 9 tests: snapshot served org-bound with the short Cache-Control and the full contract keys (unconfigured null, caps null vs value); foreign-org refusal table (other org, case, missing, repeated, padded, X-Org-Id not accepted as binding) with zero reads; a mismatched snapshot is never served; quarry-edge-only and read-only (gateway delegation 403, another machine with the same scope 403, token/principal mix 401, missing/wrong token 401, POST 403, account read, usage write and allowance write 403, generic quota and entitlement reads still allowed); store error mapping without detail leak; scope mapping; registry with quarry-edge passes `ValidateRequiredServiceCredentialRegistry` and quarry-edge cannot hold `:self`; plan caps opt-in/explicit/negative; allowance unconfigured vs spent vs paused vs future vs plan quota vs unlimited. The service_auth tests live in this file (service_auth_test.go is not in the unit's list). The Postgres path of `Service.QuarryLimits` has no test (depth row).

## Commits (lane a)
- 5fcd7f489 feat(billing-core): org-bound Quarry limits snapshot for the quarry-edge credential (W2-CP-L1)
- 8c9128242 test(billing-core): Quarry limits handler, foreign-org refusal, scope and registry (W2-CP-L1)
- docs(plan-work) commit with this file and the depth rows

## Code only, not compiled
Owner order 2026-11-03: no go build or test was run. Only `gofmt -d -e` (parse and format, no compile) ran on LF copies of the five files: clean. The integration step compile-checks; then `cd "apps/Control Plane/billing-core" && go test ./internal/http/ ./internal/billing/`.

## Flag and default
- The route is read-only and reachable only by the quarry-edge credential; the edge reads it only with `QUARRY_EDGE__LIMITS_SOURCE=billing_core` (default `static`, W2-CP-L2). No separate route flag.
- `BILLING_QUARRY_PLAN_CAP_DEFAULTS` (billing-core env, default off): off serves only explicit per-org `quarry.cap.*` keys from the account's `quota_limits` (every other cap `null`, which is the edge floor); `1|true|on` serves the placeholder plan numbers. Not added to compose (no compose edits in scope).

## Assumed decisions
- Path `/api/v1/billing/orgs/{org}/quarry/limits`: one call per org instead of one per metric; the edge caches the whole answer 30 s per the plan.
- "Org-bound" means the edge states the tenant twice (`X-Quarry-Org-Id` plus the path) and billing-core refuses a mismatch, plus the principal pin. A machine `:any` credential cannot be bound harder without a delegation scheme (R13-2's status said the same). A new header name because the middleware strips `X-Org-Id` for machine scopes.
- Principal pinned in the handler to `quarry-edge` (the response is Quarry-specific); other holders of `billing:quota:read:any` keep the generic quotas route.
- Cache-Control `private, max-age=30` (the plan's edge cache TTL), errors `no-store`.
- Placeholder plan caps are the plan's (free 1/2/1/1/10/1, hobby 2/4/1/2/50/2, standard 4/6/2/3/200/4, pro 8/8/2/5/1000/10, enterprise 16/16/4/10/5000/50, stored plan trial = free). They live in service.go, not defaults.go (outside the list), and are off by default. An active trial resolves to TrialPlan (pro) and takes pro's numbers.
- A negative explicit `quarry.cap.*` key reads as `null` (the edge has no "unlimited" capacity, only its process-wide cap). An explicit 0 is a real cap.
- The plan's `QuotaStatus.Configured` field on the generic quota route was not added (types.go is outside the list); the new snapshot carries `configured` per metric instead.

## Contract for W2-CP-L2 (Rust half, lane b)
Request: `GET {BILLING_CORE_URL}/api/v1/billing/orgs/{org}/quarry/limits` with `X-Service-Id: quarry-edge`, `X-Service-Token`, `X-Quarry-Org-Id: {org}`. Response `data`: `org_id`, `account` (`present|absent`), `plan`, `subscription_state`, `allowances` keyed `quarry.crawl.page`, `quarry.batch.url`, `quarry.browser.second.agent`, each `{metric, configured, source, active, unlimited, limit|null, used, remaining|null, exhausted, period_end?}`; `caps {agent_runs, interactive_runs, render_permits, crawl_jobs, job_backlog, rate_multiplier}`, each an integer or null; `plan_cap_defaults`; `as_of`. Mapping: `configured=false` or `unlimited=true` gives `Allowance::Unconfigured`, otherwise `Remaining(remaining)`; a null cap is `None`. 503 goes to the fail mode; 403 `organization_inactive` is L2's call (suggest floor plus a log line).

## Model Plane asks
None. Nothing appended to MP-ASKS.md.

## Not verified
- Not compiled, no test run (owner order).
- `Service.QuarryLimits` against Postgres (account absent/present, policy, plan quota, tombstone) has no test.
- No live billing-core, no compose, no edge round trip; L2 does not exist yet, so the header and the shape are a one-sided contract.

## Depth rows
Appended to `.plan-work/quarry-w1w2/W23-depth-lane-a.md` (tag W2-CP-L1).
