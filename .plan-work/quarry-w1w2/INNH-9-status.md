# INNH-9: Credits per job on JobSummary and JobStatusView

Status: built and committed locally on quarryv2-next (2026-10-31). Go and Rust tests green (Rust run in the rust:1 container after Docker came back); Postgres SQL and side stack still unproven. Flag QUARRY_CONTROL__JOB_CREDITS defaults off; nothing deployed.
Lane: B (control/orchestrator). Plan home: WS6.C. Estimate: 3 d. Innhenting UI reads this one first (with item 1).
Verified by reading quarryv2-next. Nothing was built or run. Paths under `apps/Ingestion Plane/Quarry-v2`.

## What exists
1. No credit field on either type: Rust JobSummary `crates/quarry-core/src/resources.rs:156-186` (job_id, kind, org_id, status, created_at, run_id, started_at, completed_at, stats: Value, origin, phase) and JobStatusView :239-276; control list wire jobWire `services/quarry-control/internal/resources/job_wire.go:26-62` and summary view jobSummaryView `jobs_lifecycle.go:36-47` have none.
2. The price lives only in team aggregation: GET /v1/team/credit-usage -> resource_routes.rs:867 team_credit_usage -> control cycle24.go:189 -> pg teamUsageStore.CreditUsage `store/pg/resources_v2.go:49-73` and mem `resources_v2_mem.go:112-127`; constant creditsPerPage = 1.0 at pg/resources_v2.go:532 and resources_v2_mem.go:54, whose comments cite `quarry_core::credits::CREDIT_COST_PER_PAGE`, which does NOT exist in Rust.
3. Per-page truth exists in control's event log: orchestrator workflows.go:268 emits one page_fetched per fetched page; control already counts it per job: countsFrom jobs_lifecycle.go:166-174 (counts.completed) and jobStats job_wire.go:94-102 via CountsForJobs/SummaryForJob (store.go:293-298; pg resources.go:1297, :1323). So "completed pages" per job is already on the status view (JobCounts.completed) and the list (stats.completed).
4. Billing emission (WS6.C-e) is separate and goes to NATS/billing-core, not control: usage.rs UsageEvent, usage_attrs.rs, QUARRY_EDGE__USAGE_BROWSER_METRICS (default off), metric quarry.crawl.page (docs/CONTRACTS.md:784-798, docs/BILLING.md). W2-closeout U37: partly built, DARK, Lago codes outside.
5. `job_pricing.rs` (estimate in micro-USD, placeholder rates) is a pure module with no caller (only `pub mod job_pricing;` lib.rs:65).
6. Edge builds the status view in `job_lifecycle_routes.rs:104-141` status_view from control's GET /v1/jobs/{id}/summary (ControlSummary :75-89); the list is forwarded as JobSummary by forward_list.
DEFECT FOUND: pg CreditUsage SQL (resources_v2.go:56-66) sums COALESCE((payload->>'pages')::numeric,1) over EVERY event with a job_id, so run_started, page_queued, artifact_written etc. each add 1 credit, while the in-memory impl (:119-123) counts only events with a numeric pages payload or page_fetched. Team credit_used is inflated on Postgres and will not equal the sum of per-job credits. Read from SQL, not run.

## The gap
No credit number per job. Build only:
(a) a pure control function creditsFor(job.Kind, counts) = counts[page_fetched] x one shared price constant, for kinds crawl, batch, scrape, extract (page-based); kinds search, research, agent get no value (agent cost is browser seconds and LLM tokens, not events in control; surfacing it needs billing-core reads, out of scope);
(b) expose it on control's summary view and list wire, and on the edge JobStatusView/JobSummary;
(c) one shared price constant (delete the two copies, fix the false comment) and correct the pg CreditUsage SQL to count page_fetched only so per-job and team figures agree;
(d) openapi/CONTRACTS entries.
Design choice: DERIVE at read time, do not store. Events are never deleted (the janitor only evicts artifacts, janitor.go:104), so derivation is stable, retroactive for old jobs, needs no migration, and cannot go stale when a late page_fetched lands after the terminal event (which a frozen value written in markJobTerminalIfNeeded, resources.go:191-224, would). If the owner insists on "written", freeze into jobs.statistics (JSONB, COALESCE-preserved by updateJobSQL pg/resources.go:218) after terminalStatistics assignment at resources.go:220 instead of adding a column; not recommended.
Semantics decision for the owner: the figure is a Quarry-side estimate (flat 1 credit per fetched page, browser-rendered pages priced the same, failed pages free), not what billing-core invoices; label it estimated.

## Files
MODIFY `crates/quarry-core/src/resources.rs` (add JobCredits {used:f64, unit, basis, price_per_page, estimated, final} + Option on JobStatusView ~:239 and JobSummary ~:156; extend the serde roundtrip test ~:560); `crates/quarry-edge/src/job_lifecycle_routes.rs` (ControlSummary + status_view :75-141); `crates/quarry-edge/src/graphql/schema.rs:16` (optional mirror); `services/quarry-control/internal/resources/jobs_lifecycle.go` (jobSummaryView.Credits, buildSummary :178), `job_wire.go` (jobWire.Credits, jobWires :105-120, toJobWire), `store/pg/resources_v2.go:49-72` and `:532`, `store/resources_v2_mem.go:54,112` (one shared const, SQL fix: filter e.type = 'page_fetched'); NEW `services/quarry-control/internal/resources/job_credits.go` (pure creditsFor + flag read); `docs/openapi.yaml` (GET /v1/jobs/{job_id} response is untyped `type: object` at ~:1578-1600 and JobSummary :3423 lacks origin/phase/stats details: add typed JobStatusView and JobCredits schemas, additive); `docs/CONTRACTS.md` ("Job lifecycle reads and origin" ~:715-768 plus a credits paragraph); `docs/REST_RESOURCES.md`; sdks only if regenerated (W2-closeout WS13.8: generation blocked by an unparseable spec; do not hand-edit). No migration. No change to quarry-orchestrator or the Rust runtime.

## Flag
Control env `QUARRY_CONTROL__JOB_CREDITS`, default OFF (field absent) because the 1.0 price is a placeholder and the team figure is wrong on pg; read per request via a small helper (same style as zdrTombstoneReceiverPosture(os.Getenv...) in cmd/control/main.go). The edge needs NO flag (forwards what control sends; Option fields skip_serializing_if none). Do NOT touch the live flags. Turning the flag on in the shared stack needs a control image rebuild and is the owner's call; prove on a side-by-side stack (control + Postgres) or Go tests with the in-memory store.

## Contract change
Additive, optional on all four reads. JobStatusView (GET /v1/jobs/{id}, /v1/crawl/{id}, /v1/batch/{id}) gains `credits`: `{"used": 12.0, "unit": "credit", "basis": "pages_fetched", "price_per_page": 1.0, "estimated": true, "final": false}`. `final` is false while status is not terminal (accepted/running/paused), true once terminal; `used` = counts.completed x price_per_page. Field omitted (not null) for kinds search, research, agent and when the flag is off. JobSummary (GET /v1/{kind}/jobs, /v1/crawl/ongoing, /v1/batch/ongoing) gains the same object (control already computes page counts for the whole page with one grouped query, CountsForJobs, so no N+1). Control summary view (GET /v1/jobs/{id}/summary) and list wire carry `credits` first; edge forwards unchanged. GET /v1/team/credit-usage keeps its shape; its pg figure changes (corrected) and CONTRACTS.md says so. No request change, no new route, no new error code.

## Tests to add
Go (`go test ./...` in services/quarry-control): `job_credits_test.go` table test creditsFor by kind and counts (crawl 5 fetched -> 5.0, failed pages free, batch, scrape, search/agent -> nil); `jobs_lifecycle_test.go`: summary carries credits only when flag on, final=false running and true terminal; `job_wire_test.go`: list row carries credits and wire still decodes (existing contract tests job_wire_test.go:27-81 and store/job_wire_test.go); resources_v2 test that pg SQL and mem agree (storetest/ shared suite): events run_started/page_queued/page_fetched x3 -> 3.0 credits; org isolation (other org's job -> 404, other org's events not summed).
Rust (quarry-core, quarry-edge via the Docker rust:1 recipe, >10 min): resources.rs unit test that control's wire JSON with and without `credits` deserializes into JobSummary and JobStatusView (extend roundtrip at :560); `crates/quarry-edge/tests/job_lifecycle.rs`: summary_json() fixture (:38) gets credits, assert GET /v1/crawl/{id} returns it and a control answer without it still works. scripts/tests/docs-contract.test.mjs and openapi-route-coverage still green; openapi-no-bare-null passes.
Evidence for "done" needs a rebuilt control image on a side stack; unit tests alone are not a deployment claim.

## Risks
- COST/BILLING: a customer-visible number that may disagree with the invoice. Price is flat 1.0/page; browser-rendered pages cost more in reality (job_pricing.rs placeholders); agent runs bill by browser seconds. Label estimated=true and keep it off until the owner approves price semantics (owner decision: is 1 credit per fetched page the product price, and should browser pages weigh more).
- The pg team-credit SQL bug means the dashboard total already looks higher than the sum of jobs until the SQL fix ships; the fix changes a number users may have seen.
- Duplicate page_fetched events (activity replay) would over-count if the event idempotency key does not dedupe: emitEvent uses key string(EvtPageFetched) plus url; UNVERIFIED that control's AppendNext dedupes on it.
- ZDR: none new; the value is a count, no URL or content; crawl/batch are refused under ZDR (routes.rs refuse_durable_job_under_zdr ~:1188); existing ZDR matrix rows job_lifecycle_reads/control_records cover these routes (inert).
- Security: credits read through the same org-scoped GetByOrg (jobs_lifecycle.go jobSummary) so a foreign id stays 404; no new route.
- Compat: Rust JobSummary/JobStatusView deserializers must stay tolerant (`#[serde(default, skip_serializing_if = "Option::is_none")]`). Dev compose uses memory stores; the mem CountsForJobs impl must be checked.

## Built (what changed vs the plan above)
All paths under `apps/Ingestion Plane/Quarry-v2`. Line endings preserved per file (CRLF files kept CRLF).
Go (services/quarry-control):
- NEW `internal/store/credits.go`: `store.CreditsPerPage = 1.0` (the one shared price) and `store.CreditEventType = "page_fetched"`. The two old `creditsPerPage` copies and their false comment (`quarry_core::credits::CREDIT_COST_PER_PAGE` does not exist) are deleted.
- MODIFY `internal/store/pg/resources_v2.go`: CreditUsage SQL now `COUNT(*) * price` over `type = 'page_fetched'` events of the org's jobs (was: SUM of `payload->>'pages'` defaulting to 1 over EVERY event with a job_id). `internal/store/resources_v2_mem.go`: counts page_fetched only (the unused `payload.pages` multiplier is gone; no producer ever emitted it). Team figure on Postgres now equals the sum of per-job credits and reads lower than before.
- NEW `internal/resources/job_credits.go`: `creditsFor(kind, status, fetchedPages)` pure (crawl, batch, scrape, extract only; search/research/agent/unknown -> nil), `jobCredits` struct, flag helper `jobCreditsEnabled()` reading `QUARRY_CONTROL__JOB_CREDITS` per request (`1|true|on|yes`), hook `jobCreditsGetenv` for tests.
- MODIFY `internal/resources/jobs_lifecycle.go`: `jobSummaryView.Credits` (omitempty), filled in `buildSummary`, dropped by the `jobSummary` handler when the flag is off. `internal/resources/job_wire.go`: `jobWire.Credits`, filled in `jobWires` (same grouped `CountsForJobs` query, no N+1) when the flag is on. `jobWires` backs `/v1/{kind}/jobs`, `/v1/jobs/ongoing` and the crawl/batch ongoing lists.
Rust (written, not compiled):
- `crates/quarry-core/src/resources.rs`: `JobCredits {used, unit, basis, price_per_page, estimated, is_final (serde "final")}`; `credits: Option<JobCredits>` on `JobSummary` and `JobStatusView` (default, skip_serializing_if none); 2 new unit tests.
- `crates/quarry-edge/src/job_lifecycle_routes.rs`: `ControlSummary.credits` (serde default) passed into `status_view`. List rows pass through `JobSummary` via forward_list with no code change. No edge flag.
- `crates/quarry-edge/tests/job_lifecycle.rs`: control double sends credits for a crawl; asserts they reach `GET /v1/crawl/{id}`; new test `status_without_credits_from_control_still_works` (batch, key absent).
Docs: `docs/openapi.yaml` (typed `JobStatusView`, `JobCredits`, `JobRunOrigin`; `JobSummary` gains origin, phase, credits; the three status 200s now reference JobStatusView), `docs/CONTRACTS.md` ("Credits per job" paragraph, includes the team-figure change), `docs/REST_RESOURCES.md`. SDKs not regenerated (WS13.8 blocker) and not hand-edited.

## Response shape
GET /v1/crawl/{id} (also /v1/batch/{id}, /v1/jobs/{id}), flag on, running crawl:
`"credits": {"used": 12.0, "unit": "credit", "basis": "pages_fetched", "price_per_page": 1.0, "estimated": true, "final": false}`
Same object on each list row. Absent (not null) for search/research/agent and whenever the flag is off.

## Commits
feat(quarry-control): credits per job on job summaries and status (INNH-9), local on quarryv2-next (sha in git log).

## Tests (counts)
Go, `cd services/quarry-control && GOWORK=off go test ./...`: all 19 packages ok (build clean). New/changed tests, `-run 'Credit|TeamCredit' -v`: 28 PASS, 1 SKIP.
- resources/job_credits_test.go: TestCreditsFor (14 table cases), TestCreditsJSONShape, TestJobCreditsEnabled, TestBuildSummary_CarriesCredits, TestJobSummary_CreditsOnlyWhenFlagOn (off absent, on used 3, final false -> terminal final true, a late page_fetched still counted), ..._CreditsAbsentForNonPageKinds, ..._CreditsOrgScoped (foreign org 404), TestJobLists_CarryCreditsAndStillDecode (ongoing + by-kind rows decode into jobWire, search row has none), TestJobCredits_AgreeWithTeamCreditUsage.
- storetest.CreditUsageConformance (run_started/page_queued/page_failed earn nothing, page_fetched earns the price, org isolation, empty org 0): TestMemoryCreditUsage PASS (3 subtests); TestPostgres_CreditUsage SKIPPED (needs QUARRY_TEST_DSN, no Postgres; the SQL fix is therefore untested against a real database).
- cycle24_test TestTeamCreditUsage_DerivesFromEvents updated (3 page_fetched + a run_started = 3.0; was 6.0 from a `pages: 2` payload that no producer sends).
Node: docs-contract 7 pass / 0 fail (12 pre-existing pending), openapi-no-bare-null 2/2, openapi-route-coverage 10/10, openapi-internal-guard 9/9, openapi-scrape-parity 5/5, openapi-challenge-parity 8/8.
Rust (qtest.sh, rust:1 image, 2026-10-31): `cargo test -p quarry-core --lib` 253 passed / 0 failed; `cargo test -p quarry-edge --test job_lifecycle` 14 passed / 0 failed (incl. status_without_credits_from_control_still_works and the credits passthrough); `cargo check -p quarry-edge --tests` Finished, no errors. Go re-run with -count=1: all packages ok. Node contract tests re-run: docs-contract 7 pass/0 fail, no-bare-null 2, route-coverage 10, internal-guard 9, scrape-parity 5, challenge-parity 8, all 0 fail.

## Not verified
- The Postgres SQL (TestPostgres_CreditUsage skipped; also the numeric cast of the COUNT * float param) is unproven against a real database; needs QUARRY_TEST_DSN or the side stack.
- Nothing built into an image or deployed; no side-by-side stack run. Duplicate page_fetched events (activity replay): pg dedupes on idempotency_key only if the orchestrator sets it (UNVERIFIED; the in-memory log never dedupes).
- Owner decisions still open: is 1 credit per fetched page the product price; should browser pages weigh more; turning QUARRY_CONTROL__JOB_CREDITS on needs a control image rebuild.
