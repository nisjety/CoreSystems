# Architect D: Epic outlines for audit Waves 4 and 5 (F-t2-043, 051-065; 066-077)

Date: 2026-10-15. Audit source (read-only): `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Live tree: `C:/dev/CoresSystem` (afb83928 plus uncommitted work). Nothing here is built; every path was opened in the live tree and tagged with `tools/provenance.sh` (`[C]` committed, `[M]` committed but modified, `[U]` untracked, `[NEW]` to be created, `[?]` exists on disk but git status does not list it).

Path legend: **Q** = `apps/Ingestion Plane/Quarry-v2`, **IC** = `apps/Ingestion Plane/integration-corev2`, **MP** = `apps/Model Plane`, **FE** = `apps/Frontend Plane/verevonv3`, **CP** = `apps/Control Plane`. Workstream ids WS1-WS17 are the fixed numbering of the brief (WS1 grants, WS2 run control, WS3 driver stability, WS4 ZDR boundary, WS5 scrape contract, WS6 budgets, WS7 web policy, WS8 shared browser backend, WS9 shared browser frontend, WS10 run trace, WS11 LLM planner, WS12 redaction, WS13 scrape actions/capture, WS14 crawl scope/robots, WS15 job lifecycle/schedules, WS16 structured extraction, WS17 profiles). ADR topics 1-10 are audit section 9.3 (1 policy owner, 2 secret custody, 3 shared human+agent sessions, 4 ZDR per feature, 5 anti-bot posture, 6 script model, 7 external MCP, 8 trace envelope and sharing, 9 automation owner, 10 browser substrate).

## 0. Verdict in one screen

Wave 4 is not one wave. Three of its sixteen items are already inside Wave 1-2 designs (F-t2-054 into WS8, F-t2-060 step 1 into WS3, the OpenAPI half of F-t2-064 into WS5 slice 8), one (F-t2-053 part a) is WS15 slice S2, and six are blocked on an ADR or an owner ruling. What remains buildable without a decision is: result sets, saved job definitions (reduced), provider-first document reads with planner guidance, crawl planning, request-context emulation, the research-job contract, and the postcondition and canonicalisation halves of the procedure store. Wave 5 is polish plus parked bets; only five slices (F-t2-067 slice 1, 068 clear-data, 069 step 1, 071 slice a, 074) are worth scheduling before any ADR; the other seven items are parked, deleted or folded into earlier work.

Two corrections the epics rely on:
- **[CLAIM partly false] F-t2-052 premise.** The only per-page data stored today is `PageExtraction` (`Q/crates/quarry-core/src/output.rs:81-104` [C]: url, title, 300-char excerpt, word_count, lang, driver). It has no artifact id and no structured fields, so a result set built from `page_extracted` events is a **page index, not a dataset of prices or listings**. A real dataset needs crawl/batch to embed scrape options with the `json` format (WS5 Epic E1 plus WS16). WS15's optional `GET /v1/crawl/:id/results` (D14b L4, flag off) is the same page index and is the interim; E4.1 supersedes it.
- **[CLAIM-FALSE for F-t2-053 phase 1a.]** "Stored scrape, crawl and batch schedules never fire" is fixed by WS15 slice S2 (D14b: shared input builders, `workflows/inputs.go`, flag `QUARRY_SCHEDULED_JOBS_ENABLED`), so E4.1 must not re-plan it.

## 1. Disposition table (all 28 items)

| Item | Short title | Epic | Recommendation | Reason in one line |
|---|---|---|---|---|
| F-t2-043 | URL-aware planner guidance | E4.3 | build (slices 1-3); slice 4 after a catalog-ownership ledger entry | slices 1-2 are S and need no ADR |
| F-t2-051 | Procedure store, run-as-procedure | E4.2 | build staged after ADR-lite (store owner) | compiler exists; store, postconditions and runner do not |
| F-t2-052 | Per-run result sets, export | E4.1 | build | page index first, dataset after WS5 E1 + WS16 |
| F-t2-053 | Saved jobs, packs | E4.1 | build reduced; park packs | part a is WS15 S2; pack manifest has one consumer at most |
| F-t2-054 | Run/turn-scoped tabs | E4.6 | delete as standalone | WS8 S8.9 (attach = turn scope), WS2 S4, WS3 S5 already own it |
| F-t2-055 | Crawl planning, stronger /v1/map | E4.3 | build staged after WS14 | `map:read` scope and robots Sitemap seeding are S |
| F-t2-056 | Provider APIs before rendering | E4.3 | build, verify live providers first | read ops exist in integration-corev2; agent catalog omits them |
| F-t2-057 | Engine planner v2 | E4.3 | park | three drivers only; per-request escalation already works |
| F-t2-058 | Request context, egress tiers | E4.4 | build staged; tiers after ADR 5 + 10 | emulation and cache key are M and unblocked after WS3 S2/WS17 |
| F-t2-059 | Remote browser backends | E4.5 | build after ADR 10; park without a scale need | Kernel/Browserless are test-only shims |
| F-t2-060 | Per-tenant isolation | E4.5 | step 1 already WS3; step 2 after ADR 10; step 3 park | |
| F-t2-061 | Agent routines | E4.6 | park | ADR 9, draft-only ruling, no delegated bearer in scheduled lane |
| F-t2-062 | Budgeted research job | E4.6 | build | deep_research exists; attribution is S-M |
| F-t2-063 | Code-mode browser | E4.6 | park | ADR 6; approval-hardening half has no gap, delete it |
| F-t2-064 | External contract | E4.7 | OpenAPI absorbed by WS5 S8; MCP/connector/CLI park | ADR 7 owner decision |
| F-t2-065 | Cached extraction profiles | E4.7 | park | ADR 6, WS16, cumulative cost enforcement, executor rewrite |
| F-t2-066 | Consent and ad-filter preset | E5.2 | park until ADR 5 | WS3 S4 deletes `stealth.rs`, so the consent script is rewritten, not revived |
| F-t2-067 | One honest identity | E5.2 | build slice 1 (single UA constant); park signing | |
| F-t2-068 | Browsing data lifecycle | E5.1 | build clear-data slice; park visit log | |
| F-t2-069 | Page utilities | E5.1 | build step 1 (S) | gateway allowlist and SPA media kind |
| F-t2-070 | Browser chrome polish | E5.1 | build opportunistically after WS9 | `BrowserChrome.tsx` conflict |
| F-t2-071 | MCP App sandbox, retire WebPageViewer | E5.1 | delete `WebPageViewer` now; park MCP App | dead code with `allow-same-origin` |
| F-t2-072 | Share run with Space members | E5.3 | park | ADR 8 taxonomy decision |
| F-t2-073 | SIEM export and governance extras | E5.3 | park | needs F-t2-036; confidence low |
| F-t2-074 | Failure-mode hardening | E5.2 | build (S, verify first) | raw provider bodies in errors confirmed |
| F-t2-075 | Deprecation, signed images, usefulness | E5.2 | mostly delete | deprecation headers are in D5; signing and usefulness have no premise |
| F-t2-076 | Child browser subagents, WebMCP | E5.3 | park | needs WS1, WS11, read-only grant class |
| F-t2-077 | User's own Chrome spike | E5.3 | park (decision note only) | owner ruling 2026-09-28 |

## 2. Sequencing

1. **Open now (no decision, small):** E4.1 slice 1 (`run_id` on `ArtifactSummary`), E4.3 Track A slices 1-2 and the provider-read verification spike, E5.1 slice a (delete `WebPageViewer`) and step 1 (gateway PDF), E5.2 F-t2-074 and the UA constant, E4.4 `map:read` and Sitemap seeding.
2. **After WS5 + WS14 + WS15:** E4.1 items route, E4.3 crawl planner tool, E4.3 URL rewrite table through `content_route.rs`.
3. **After WS3 S2/S10 + WS11 stage 2:** E4.2 postconditions and canonicalisation (Quarry only), E4.4 emulation.
4. **After WS1 + WS6 + WS11 + WS17:** E4.2 store and runner, E4.4 profile pinning and org-prefixed cache key, E4.6 research job.
5. **Gated by ADR:** E4.4 tiers (ADR 5, ADR 10 for browser proxy chaining), E4.5 (ADR 10), E4.6 routines/code mode (ADR 9, ADR 6), E4.7 MCP (ADR 7), E5.3 sharing (ADR 8).

Decisions to take before Wave 4 opens (beyond ADRs 1-10): (a) results are an index or a dataset (D14b D9), (b) owner of the saved-procedure store (recommended: capability-core, session-core migration, per the S4.3 Watch precedent in `MP/docs/decisions/ledger.md:4174` [C]), (c) owner of the provider operation catalog (recommended: integration-corev2 serves it; execution-core `CATALOG` at `MP/rust/services/execution-core/src/integration_tools.rs:71` [C] is generated from it).

