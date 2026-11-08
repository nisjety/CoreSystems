# W2-CP-L2 status: billing_core limits source, plan-aware run caps and plan-cap stamp (U43 Rust half, U59, U32; lane b, branch w23/lane-b)

## Built
Partly: the scope is built in the four listed files, except the wiring of plan-aware render permits into the lease pool (it lives in quarry-runtime `lease_pool.rs` and edge `browser_driver`/`main.rs`, outside the list; see "Not done"). Code only, not compiled.

Checked first what existed: the static resolver, `FailMode`/`WithFailMode`/`LimitsBackend`, `RunCapOverride` and the U59 run caps (run_registry, start_run) were already built (U43, U59). The edge-side admission stamp (`handoff.rs admission_stamp`/`with_admission`, D2-017, 518584b41) was already built too, so U32's gap was the control side. Built only the gap:

- `crates/quarry-edge/src/limits.rs`
  - `LimitsConfig::build` no longer refuses `billing_core`; new `build_with(lookup)` reads `BillingCoreSettings` (only for that source) and returns `WithFailMode<BillingCoreBackend>`. A missing URL or token still stops boot, naming the variable (never the value).
  - `BillingCoreSettings` (redacting `Debug`), `LimitsDoc`/`AllowanceDoc`/`CapsDoc` (the W2-CP-L1 `data` shape; unknown fields ignored), `LimitsDoc::into_org_limits` (refuses an answer about another org: `org_mismatch`; `configured=false` or `unlimited=true` is `Unconfigured`; configured+limited with `remaining: null` is `malformed`, not zero; caps null/negative is `None`, explicit 0 kept, over u32 saturates).
  - `LimitsFetch` trait + `HttpLimitsFetch`: `GET {base}/api/v1/billing/orgs/{org}/quarry/limits` with `X-Service-Id: quarry-edge`, `X-Service-Token`, `X-Quarry-Org-Id: {org}`; org id must be 1-128 of `[A-Za-z0-9_-]` (refused before any request); timeout; body cap 64 KiB (content-length and streamed); status classes 401 unauthorized, 403 forbidden (incl. organization_inactive, body not read), 404 not_found, 5xx unavailable, other rejected; reqwest errors and bodies never logged.
  - `BillingCoreBackend<F>`: per-org in-process cache with an injectable clock. Fresh (< TTL) served without a read; stale (TTL .. TTL+window) tries a read and on failure serves the cached answer (its clamps still apply) with a warn line; past the window or never read, the error goes to the fail mode. Soft cap 4096 orgs (entries past the window dropped).
  - `org_render_permits(pool_org_max, pool_size, &OrgLimits)` (U59): 0 while `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` is 0 (flag off, plan not read); otherwise the plan's `render_permits` replaces the operator floor, capped by the pool size; fail-open = pool size; plan 0 reads as 1 (the pool reads 0 as "no cap").
  - Tests: `billing_core_tests` (15): snapshot mapping, caps mapping, org mismatch/malformed never served, fresh cache + re-read after TTL, cache per org, stale window still clamps when down, past the window floor/open/closed, down with nothing cached, recovery replaces the stale answer, two plans two capacities (run override, job slots, render permits), render permits off at 0, settings required/invalid/defaults/token not printed, `build_with` for billing_core, wiremock HTTP read (path, three headers, 503, 403, invalid org ids send nothing, resolver floor on 503). The old `billing_core_source_is_refused_at_build...` test is now `..._without_settings_is_refused_at_build...`.
- `services/quarry-control/internal/resources/admission.go` (U32, control half): `stampPlanCaps` runs in `backlogFull` (createJob's admission step, before the job is stored; resources.go not touched): with admission on and a dispatched kind, `params.admission` keeps only `cap`/`backlog_cap` as whole positive numbers, clamped to new `CapMax` (`QUARRY_JOB_CAP_MAX`, default 50) / `BacklogMax` (`QUARRY_JOB_BACKLOG_MAX`, default 10000), adds `source: "plan"` (and `clamped: true` when clamped); an empty or invalid stamp is removed so the defaults apply. `backlogCap` now reads ints too (shared `admissionNumber`). Boot refuses a max below its default.
- `admission_test.go`: defaults/env cases extended; 6 new tests (stored stamp, ceiling clamp, invalid stamps removed, off and undispatched kinds untouched, backlog check uses the clamped value, two plans claim two caps plus a default org).
- `run_limits.rs`: not changed (nothing in the gap needed it; the U59 run caps already use `MAX_CONCURRENT_RUNS` as the global cap via `RunCapOverride::bound_for`).

## Commits (lane b)
- 956351dcf feat(quarry-edge): billing_core limits source with cache, stale window and plan-aware render permits (W2-CP-L2)
- 292b7fbc4 feat(quarry-control): plan-cap stamp fixed at admission with ceilings (W2-CP-L2, U32)
- docs(plan-work) commit with this file and the depth rows

## Code only, not compiled
Owner order 2026-11-03: no cargo or go build/test ran. Only parse/format checks on copies: `rustfmt --check` on limits.rs (parses; only width reflows, the file was not rustfmt-clean before) and `gofmt -l -e` on LF copies of the two Go files (clean). Integration compile guard: `cargo test -p quarry-edge --lib limits` (qtest) and `cd services/quarry-control && go test ./internal/resources/ -run 'Admission|Backlog|Plan|Claim'`.

## Flag and default
- `QUARRY_EDGE__LIMITS_SOURCE` default stays `static` (nothing read from billing-core, edge behaves as before). `billing_core` needs `QUARRY_EDGE__BILLING_CORE_URL` and `QUARRY_EDGE__BILLING_CORE_TOKEN` (compose lines are W2-CP-E2b's, empty). New optional knobs, read inside limits.rs: `QUARRY_EDGE__LIMITS_CACHE_TTL_S` (30, min 1), `QUARRY_EDGE__LIMITS_STALE_WINDOW_S` (300, min 0), `QUARRY_EDGE__LIMITS_TIMEOUT_MS` (2000, min 100). `QUARRY_EDGE__LIMITS_FAIL_MODE` default stays `floor`.
- `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` stays 0 (off).
- `QUARRY_JOB_ADMISSION` stays off: the stamp normalisation runs only with admission on. New `QUARRY_JOB_CAP_MAX` (50) and `QUARRY_JOB_BACKLOG_MAX` (10000), not in compose.

## Assumed decisions
- Cache TTL 30 s (the plan's edge cache, matching billing-core's `Cache-Control: max-age=30`), stale window 300 s, timeout 2 s. The cache is in-process per edge replica (no Redis): two replicas can each serve their own 30 s view.
- A stale read is tried on every request past the TTL (no single-flight / backoff); while billing-core is down each request in the window pays one failed read (bounded by the 2 s timeout).
- 403 `organization_inactive` (and any 403/401) goes to the fail mode like a 503 (floor by default) with a warn line, per L1's suggestion; a still-fresh or stale cached answer is served first.
- A configured, limited allowance with `remaining: null` makes the whole answer `malformed` (fail mode), never `Remaining(0)`.
- Org ids accepted for the path: `[A-Za-z0-9_-]{1,128}`; anything else is never sent (fail mode).
- Plan `render_permits: 0` reads as 1 permit (D2-215 encoding: zero must not become "no cap").
- Control ceilings 50 / 10000 sit above the L1 placeholder plan maxima (enterprise crawl_jobs 10, job_backlog 5000). The control-side stamp is fixed at admission ("source": "plan"); the defaults are NOT stamped, so an operator change of `QUARRY_JOB_CAP_DEFAULT` still reaches queued unstamped jobs.
- The stamp is normalised inside `backlogFull` because `createJob` (resources.go) belongs to W2-CP-S7b; the call site already passes `in.Params`, which is stored after the check, so the in-place normalisation reaches the stored job.

## MP asks
None. Nothing appended to MP-ASKS.md.

## Not done
- Plan-aware render permits are computed (`org_render_permits`) but not wired: the lease pool sizes each org semaphore once from a fixed `org_max` (`quarry-runtime/src/lease_pool.rs`), and the browser driver acquires by org id without the org's limits. Wiring needs a per-org cap argument on `acquire_timeout_for_org` (or a resizable org semaphore) and the caller to pass `org_render_permits(...)`; files outside this unit (depth row W23-b-L2-1).
- main.rs still has the comment "the unbuilt `billing_core` ... stops the edge"; main.rs is outside the list (depth row W23-b-L2-2).

## Not verified
- Not compiled, no test run (owner order). In particular: async_trait on the generic `BillingCoreBackend<F>`, reqwest 0.13 `chunk()`/`content_length()`/`path_segments_mut` usage, and the wiremock matchers are written against the APIs as read, not checked.
- No live billing-core round trip; the L1 contract is still one-sided on both ends.
- The pg claim path with a stamped `"source"` key is unchanged in SQL (it reads only `cap`); not run with a DSN.

## Depth rows
Appended to `.plan-work/quarry-w1w2/W23-depth-lane-b.md` (tag W23-b-L2).
