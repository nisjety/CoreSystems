# INNH-9: Credits per job on JobSummary and JobStatusView

Status: not started
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

## Commits
(none yet)

## Tests (counts)
(none yet)

## Not verified
Code reading only; nothing built or run. Unverified: AppendNext dedupe on page_fetched; the SQL defect is from reading, not executing.
