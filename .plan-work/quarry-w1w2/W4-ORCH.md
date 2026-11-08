# W4 orchestration ledger (Quarry Wave 4, E4.1 to E4.7)

Date: 2026-11-07. Branch `quarryv2`, worktree `gracious-gagarin-087139`.
Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md` section 3.4 (lines 3163-3277). Inputs: `W23-STATUS.md` (W2/W3 integration ledger), `MP-W2W3-STATUS.md`, `INNH-12-status.md`, Model Plane board `epic-shaw-4d613b/docs/plans/wave3-5-execution-control.md` (untracked there).

## 0. Base

- `quarryv2` was at `bed81d162` (2026-10-16). It was 2778 commits behind `main` and had none of the W2/W3 work that W4 depends on (WS1, WS3 S2, WS5, WS6, WS10, WS14, WS15, INNH-12).
- On 2026-11-07 it was fast-forwarded to `main` at `2a776c837` (`git merge --ff-only main`). It had no local commits. Its 317 untracked `.plan-work` files do not overlap any file that `main` tracks, so nothing was overwritten. To undo: `git branch -f quarryv2 bed81d162` from another worktree.
- `main` contains `w23/integ` (W2/W3 Quarry), `integ/quarry-mp-main` and `mp-w2w3` (Model Plane W2/W3). `main` does **not** contain branch `model-plane`, which has 18 Model Plane EP commits (section 6).
- Rules: build dark (every new behaviour sits behind a flag that is off by default), no live flag flips, and local commits only. This orchestrator commits notes only. Builders commit code on their own lane branches, cut from `quarryv2`.

## 1. Status key (built vs proven)

| Mark | Meaning |
|---|---|
| B | Built: code on `main`/`quarryv2`, compile-checked, flag off |
| T | Tested in process: unit/integration tests ran green (doubles, scratch DB) |
| P | Proven live: run on a stack (compose), flag on in a test org |
| - | not built |
| gated | waits for an ADR or owner decision; not dispatchable |
| parked / deleted / absorbed | per plan disposition, not W4 scope |

## 2. Wave completion

Scope is the plan's "build" items, weighted by plan effort in days. Parked, deleted and absorbed items are excluded. F-t2-059 is counted only if a scale need exists, so it is excluded here.

| Epic | Scheduled d | B (d) | T (d) | P | % built | Gate on the remainder |
|---|---|---|---|---|---|---|
| E4.1 result sets, saved jobs | 17 | 8 | 6 | 0 | 47 % | none for export; D14b D9 (index) to ratify |
| E4.2 procedure store | 30 | 0 | 0 | 0 | 0 % | store-owner ledger entry (store half); postconditions and canonicalisation are open after WS3 S10 / WS11 |
| E4.3 URL-aware planning, provider reads | 41 | 7 | 3 | 0 | 17 % | catalog-owner ledger (043 s4); ZDR-token / service-principal decision (055 end to end) |
| E4.4 request context, egress tiers | 27 | 1 | 1 | 0 | 4 % | ADR 5 + ADR 10 (tiers only) |
| E4.5 remote backends, per-tenant | 17 | 0 | 0 | 0 | 0 % | ADR 10 (all of it) |
| E4.6 research jobs | 12 | 0 | 0 | 0 | 0 % | open question 12; EP-12 S1 overlap (section 6) |
| E4.7 external contract | 3 | 1.5 | 1.5 | 0 | 50 % | ADR 7 (MCP, CLI); 065 parked |
| **Total** | **147** | **17.5** | **11.5** | **0** | **~12 % built, ~8 % tested, 0 % proven** | |

Of the 87 days that need no ADR (E4.1, E4.3 without s4, E4.4 a+b, E4.2 postconditions and canonicalisation, E4.6, E4.7), 17.5 are built (about 20 %).
No W4 slice has been dispatched as W4 yet. Everything counted as built arrived as a side effect of W2/W3/INNH work.

## 3. Slice ledger

### E4.1 Result sets and saved job definitions (F-t2-052, F-t2-053)

| Slice | Status | Evidence | Lane | Notes |
|---|---|---|---|---|
| E4.1-1 `run_id` on `ArtifactSummary` + `?run_id=`/`?kind=` filter | B T | INNH-12 (`resources.rs:98`, `resource_routes.rs:131-140`); tests artifacts_fs_list | done | not behind a flag (additive); S3 store lists nothing |
| E4.1-2 per-run items: control `jobs_results.go` + edge `/v1/{jobs,crawl,batch}/:id/results` | B T | INNH-12; `routes.rs:208-210`; job_results 7, Go 9+3 | done | flag `QUARRY_EDGE__JOB_RESULTS` off; served as `/results`, not `/v1/runs/{id}/items` (alias not needed) |
| E4.1-3 gateway proxy + SPA results table | B | `gateway/.../ingestions/jobs.rs:106,410`; `IngestionsRunsView.tsx:581 RunResults` | done | gateway unit tests; SPA test status not recorded |
| E4.1-4 export: `csv`/`jsonl` stream, field projection, `kind: page_index`, 10 MiB cap | - | | **a** | dispatch now |
| E4.1-5 "Ingest selected" (action contract first) | - | | **d** | dispatch now; regenerate the admission golden at integration |
| E4.1-6 saved definition = `Source` with typed, versioned options (D5/D14a validators), schedules via `target_ref` | - | | **a** (after 4) | WS15 S2 schedule firing is already on main (flag `QUARRY_SCHEDULED_JOBS_ENABLED`) |
| E4.1-7 `GET /v1/presets`: delete the Go presets or gate them behind D7 `JobPolicy` (`fast` sets RobotsIgnore) | - | `presets.go` still mounted on control | **a** | owner pick; recommend delete + rename to "saved job" |
| QuarryPack manifest, schema forms | parked | | | |

### E4.2 Procedure store and run-as-procedure (F-t2-051)

| Slice | Status | Lane | Notes |
|---|---|---|---|
| E4.2-1 step-level `expect` postconditions (contracts.rs, observation.rs; `postcondition_required` still at `observation.rs:291-319`) | - | **b** (after E4.4-a) | open: WS3 S10 and WS11 stage 2 are on main |
| E4.2-2 canonicalise snapshot refs to `ClickSemantic`/fingerprint, `TARGET_REPAIR_REQUIRED` | - | **b** | `ClickSemantic` exists (`contracts.rs:847`) |
| E4.2-3 store (capability-core `internal/procedures`) + runner (execution-core planner mode) | gated | MP | ledger entry for the store owner. Migration numbers in the plan are stale: capability-core is at 0037 (so ≥0038, not 0017); allocate at merge. Check overlap with EP-11 versioned skills first (section 6). |

### E4.3 URL-aware planning and provider-first reads (F-t2-043, 055, 056; 057 parked)

| Slice | Status | Lane | Notes |
|---|---|---|---|
| 043 s1 web_search/web_fetch descriptions + `SNIPPET_WEB` (execution-core `agent.rs`) | partial B | MP | WS11.S6 routing guidance (`EXECUTION_CORE_ROUTING_GUIDANCE`, off) covers part; no `SNIPPET_WEB` |
| 043 s2 forward `driver_capabilities`/`execution_tier` into PlannerView | - | MP | |
| 043 s3 `url_patterns` on `AgentSkill` (sessions.proto, session-core) | - | MP | file conflict with EP-11 S1 versioned skills (on `model-plane`); after model-plane merges |
| 043 s4 catalog served by integration-corev2 | gated | MP + Ingestion | catalog-owner ledger entry |
| 055a `map:read` scope + registry rows | B T | E3.7.10 / R-13 | `auth.rs:827`, principals json 185/205 |
| 055b `/v1/map` seeded from robots `Sitemap:` | B T | E3.7.4-6w | `map_routes.rs:633` |
| 055c crawl-plan tool to approval to user-token crawl (`web_tools.rs`, `CrawlPagePicker.tsx`) | partial B | MP + **d** | E3.7.10 cap.web.* tools (`EXECUTION_CORE_WEB_CRAWL_TOOLS` off), E3.7.9a/b approval (observe). Blocked end to end by blocker B4. |
| 056a read-only `drive.file.content` rows | B | done | `integration_tools.rs:109,120`; `MAX_RESULT_CHARS` still 2000 (`:57`). The larger read cap is an MP slice. |
| 056b public-URL rewrite table (Docs/Sheets/Slides export, GitHub blob to raw) in `content_route.rs` | - | **c** | dispatch now |
| 056c Notion page-content action (integration-corev2) | - | Ingestion (integration) | |
| 056d live verification of the Google/Microsoft content ops | - | stack session | needs live connections |

### E4.4 Request context and egress tiers (F-t2-058)

| Slice | Status | Lane | Notes |
|---|---|---|---|
| a. `locale`/`timezone`/`location.country`/`device` on ScrapeOptions + CDP overrides in `apply_page_hygiene` (`chromiumoxide.rs:4860`) | - | **b** | open: WS3 S2 is on main. `profile_restore.rs:734-875` already uses Set{Locale,Timezone}Override for profile restore; reuse it. |
| b. `profile_id` through ProfileStore (D16 owner guard), personalised scrape bypasses the cache | - | **b** (after a) | the org-keyed cache (WS4.K) is done: `cache.rs:1-16` |
| c. egress tiers (`EgressBroker` tier, org ceiling, browser proxy chain) | gated | | ADR 5 + ADR 10. Not the same as Model Plane EP-9 "egress tiers" (that is the code sandbox, `mp-contracts/egress_tiers.rs`). |
| SessionPool | parked | | |

### E4.5 Remote backends and per-tenant isolation (F-t2-059, 060)

| Slice | Status | Lane | Notes |
|---|---|---|---|
| ADR 10 decision memo (RSS per slot, sidecar-proxy feasibility) | - | **doc** | first slice, no code; can start now |
| 060 step 1 | done elsewhere | WS3 S0 | compose `QUARRY_BROWSER_NO_SANDBOX:-0` (`docker-compose.yml:134`) |
| 060 step 2 per-org CDP pool with pinned-proxy sidecar | gated | | ADR 10 |
| 059 parts 1-3 | gated / scale need | | needs F-t2-074 `ProviderError` first (not built; E5.2) |

### E4.6 Research jobs (F-t2-062; 054 deleted, 061/063 parked)

| Slice | Status | Lane | Notes |
|---|---|---|---|
| 062a attribution: `x-request-id` + advisory parent-run header from `web_tools.rs` / model-gateway `quarry.rs`; Quarry accepts it via `usage_attrs.rs` (service tokens only) | - | MP + **c** (edge half) | prerequisites on main: `usage_attrs.rs` (WS6), WS10 request id. Align with EP-12 S1 (origin + parent id on every run). |
| 062b durable `deep_research` run record, budget, 409 on an active run | - | MP | session-core migration ≥0113 (the plan's 0048 is stale; session-core is at 0112) |

### E4.7 External contract (F-t2-064; 065 parked)

| Slice | Status | Notes |
|---|---|---|
| OpenAPI route-coverage check | B T | `scripts/tests/openapi-route-coverage.test.mjs`. It fails today on 3 undocumented routes (hit-test, `/v1/audit/page`, user receipts; W23 host gate). |
| coverage extended to crawl/jobs/schedules | B | E3.7.11 (SDKs not regenerated) |
| coverage for agent routes | - | lane **a** may take it as a fill-in (docs-only, `openapi.yaml` section) |
| SDK wait helpers, MCP, CLI, n8n | parked / deleted | ADR 7 |

## 4. Lanes and file ownership for step W4-S1

One owner per hot file per step. A lane does not edit a file another lane owns in the same step. Shared append-only files (`openapi.yaml`, `CONTRACTS.md`, `RUNBOOKS.md`, `MP-ASKS.md`, `W23-depth.md`) get one new section per unit, and conflicts are resolved at integration by keeping both sections.

| Lane | Units (W4-S1) | Owns (exclusive this step) | Must not touch |
|---|---|---|---|
| a (Quarry edge + control, Rust/Go) | E4.1-4, then E4.1-6 | `quarry-edge/src/job_results_routes.rs`, `quarry-control/internal/resources/{jobs_results.go,resources.go,presets.go}`, quarry-control store/pg sources | `routes.rs`, `quarry-core/src/contracts.rs` |
| b (Quarry browser/runtime) | E4.4-a, then E4.2-1 | `quarry-edge/src/routes.rs` (ScrapeRequest), `quarry-core/src/contracts.rs`, `quarry-browser/src/chromiumoxide.rs`, `quarry-runtime/src/observation.rs` | results routes, content_route.rs |
| c (Quarry runtime content + edge attribution) | E4.3-056b, then E4.6-062a edge half | `quarry-runtime/src/content_route.rs`, `quarry-edge/src/usage_attrs.rs` | `pipeline.rs` (ask lane b if needed) |
| d (Frontend SPA + gateway) | E4.1-5, then E4.3-055c UI half (after B4) | `src/shared/actions/action-registry.ts`, `features/ingestions/**`, `features/dashboard/home/CrawlPagePicker.tsx`, `gateway/src/domains/ingestions/**` | `BrowserChrome.tsx` (WS9 hotspot) |
| doc | ADR 10 memo; ledger entries (catalog owner, store owner, D14b D9 index) | `docs/` decision files only | code |
| MP (Model Plane program, branch `model-plane`) | asks only, see section 6 | execution-core, model-gateway, session-core, capability-core, `sessions.proto` | Quarry lanes edit no Model Plane file |

Integration guard per step (same as W23): `cargo check --workspace --tests` (Quarry), chromiumoxide feature check when lane b touches the driver, `GOWORK=off go build/vet` for touched Go modules, `tsc6 -p tsconfig.app.json` (baseline 1 error), gateway `cargo check --tests`, admission golden regeneration when `action-registry.ts` changes (copies to the four Model Plane files plus the sha pin).

## 5. Blockers

| Id | Blocker | Blocks | Owner | State |
|---|---|---|---|---|
| B1 | `quarryv2` base 3 weeks stale | all | orchestrator | **resolved** 2026-11-07 (fast-forward to `2a776c837`) |
| B2 | The three pre-W4 decisions are unrecorded: (a) results are an index, not a dataset (INNH-12 built the index); (b) procedure-store owner = capability-core; (c) provider-catalog owner = integration-corev2 | E4.1-4 `kind` field, E4.2-3, E4.3 s4 | plan owner | open; recommended defaults as listed |
| B3 | ADR 5, 6, 7, 9, 10 have no recorded decision | E4.4-c, all of E4.5, E4.6 061/063, E4.7 MCP | owner | open; the ADR 10 memo is the only unblocking work |
| B4 | execution-core's Quarry token is ZDR, and the edge refuses durable jobs under ZDR and refuses service principals on `/v1/crawl`, `/v1/batch` and the job routes (W23 lane d finish) | E4.3-055c end to end; Model Plane reads of E4.1 results | owner (Control + Ingestion) | open |
| B5 | Model Plane EP work on `model-plane` (18 commits, not on main) edits the same files as W4's Model Plane slices: `runtime_loop/agent.rs`, `tool_loop.rs`, session-core `skill_versions.rs` (EP-11) vs 043 s3; EP-12 S1 run origin/parent vs 062a; EP-11 versioned skills vs the E4.2 "saved recipe" store | E4.3 Track A, E4.6, E4.2-3 | MP controller | open; Model Plane W4 slices go to MP cells after `model-plane` merges to main |
| B6 | No W4-relevant behaviour is proven live: INNH-12 results flag dark, no stack run, no Temporal replay of a recorded history | moving any row from T to P | stack session | open |
| B7 | The plan's provisional migration numbers are stale (session-core 0048 is now 0112+; capability-core 0017 is now 0037+) | E4.6-062b, E4.2-3 | migration arbiter | renumber at merge |
| B8 | Inherited W3 items W4 leans on: web.* ids have no action-registry rows (E3.7.9a), `CRAWL_APPROVAL_MODE` stays at observe, openapi route-coverage RED on 3 routes | 055c, E4.7 | lanes d, a | open |

## 6. Model Plane status (synced 2026-11-07)

**Quarry W2/W3 Model Plane rows** (`MP-W2W3-STATUS.md`, branch `mp-w2w3`): all W2 rows are built and tested in process. W3 rows are built and compile-checked, with some named tests run. Everything is merged into `main` (mp-w2w3 is an ancestor of main). Nothing is proven live. Still open from that file:
- the WS8.J Quarry mark route (an ask; the mark is a logged 404 until it lands);
- the stage-4 legacy planner removal;
- the C6/C7 Frontend consumers;
- the asks in `MP-ASKS.md`.

The agent-action admission golden, held at hand-over, was regenerated and re-vendored at W3 S12 / lane b finish (sha `091abfa2...`).

**Model Plane EP program** (branch `model-plane`, 18 commits ahead of main, tip `2af78fea2` 2026-11-07 05:52). Controller board: `epic-shaw-4d613b/docs/plans/wave3-5-execution-control.md` (untracked). Policy: build dark, three-agent cells. The board is behind git; the table below is from git:

| Cell | Epic | Board says | Git says |
|---|---|---|---|
| C1 | EP-13 S1 picker + list price | done | cc4566b70 |
| C2 | EP-14 S1 settled-turn folding | done | 16673db40 (alt a480f985d on `mp-ep14-s1-gap` superseded) |
| C3 | EP-8 S2 containment | done | 345311759 |
| C4 | EP-2 S1-S3 web reading | done (3 zdr_posture fails cross-cell) | 8d7d38fbe |
| C5 | EP-6 phase-1 automations | done | 49666f070 |
| C6 | EP-1 takeover | dispatched | **landed** 724f8aae8, 58ca2d8a9; verifier verdict not recorded |
| C7 | EP-3 S4 revision store wiring | dispatched | **landed** 794aae5b0, 09db920a8 (two commits, same title); verdict not recorded |
| C8 | EP-4 inline + MCP Apps | done | 282a07810, c3ff2128b |
| C9 | EP-5 deck | done | b8b720e19 |
| C10 | EP-7 goals/triggers | dispatched | **landed** 6432012f5 (S1 persistent goals); verdict not recorded |
| C11 | EP-9 policy/egress | dispatched | **landed** 16e70e5f9, df513628e (S1 egress tiers); EP-9-E4/E5 already on main |
| C12 | EP-10 connectors | dispatched | **landed** 2af78fea2 (S1 connector suggestion); verdict not recorded |
| C13 | EP-11 authored behaviours | dispatched | **landed** d2fb40573, e8f556835 (S1 versioned skills); verdict not recorded |
| C14 | EP-12 sharing/API | dispatched | no commit yet |

The board puts "Wave-4 platform" after the EP-1 to EP-12 cells. No Model Plane cell has taken a Quarry W4 slice.

**Model Plane asks from W4** (to add to `MP-ASKS.md` when W4-S1 is dispatched; not added yet):
1. E4.3 043 s1 + s2 (`agent.rs` descriptions, `SNIPPET_WEB`, `driver_capabilities` into PlannerView).
2. 056a larger cap for read-only integration rows (`integration_tools.rs:57`).
3. 062a attribution headers (`web_tools.rs`, model-gateway `quarry.rs`), designed with EP-12 S1.
4. 062b durable research run (session-core ≥0113).
5. 043 s3 after EP-11 merges.
6. Store-owner decision for E4.2-3, weighed against EP-11.

## 7. Next dispatch (W4-S1, all open now, flags off)

| Lane | Unit | Done when |
|---|---|---|
| a | E4.1-4 results export (csv/jsonl, projection, `kind: page_index`, 10 MiB cap) behind `QUARRY_EDGE__JOB_RESULTS` | route tests + control tests green; openapi section; ZDR row unchanged (still refused) |
| b | E4.4-a locale/timezone/geolocation/UA override per page, new flag | lib tests + chromiumoxide feature check; live Chromium test written (`#[ignore]`) |
| c | E4.3-056b URL rewrite table in `content_route.rs`, new flag | unit tests per provider pattern; no network in tests |
| d | E4.1-5 "Ingest selected": action contract first, then the RunResults button | vitest + tsc baseline; admission golden regenerated at integration |
| doc | ADR 10 memo draft; ledger entries for B2 (a)(b)(c) | files under `docs/`; owner to ratify |

Track each unit here with B/T/P marks. Integration rows go to `W23-STATUS.md`-style entries in this file under "## Integration".

## Integration

(none yet)
