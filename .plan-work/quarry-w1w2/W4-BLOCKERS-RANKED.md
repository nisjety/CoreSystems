# W4 blockers and slices, ranked (Quarry Wave 4, E4.1 to E4.7)

Date: 2026-11-07. Branch `quarryv2` at `ad9ff1a7c`, worktree `gracious-gagarin-087139`.
Inputs: plan section 3.4 (`docs/plans/quarry-browser-build-plan-2026-10-14.md`, E4.1 to E4.7 at lines 3350-3421), `W4-ORCH.md` (`b3dec5dd6`), `W4-VERIFY.md`, `W23-STATUS.md`, `DECISIONS-ROUND-2026-10-27.md`, `X-9-control-plane-ask.md`, `MP-ASKS.md`. I read the code at the paths cited below in this worktree. I ran no tests.

Rules carried over: build dark (new flag, default off), no live flips, local commits only, one owner per hot file per step.

## 0. Corrections to W4-ORCH.md

| # | ORCH says | What is true | Effect |
|---|---|---|---|
| C1 | E4.1-4 is `-`, lane a, "dispatch now" | Built in `ad9ff1a7c` (csv/jsonl, `fields` projection, `X-Quarry-Result-Kind: page_index`, 100-row and 10 MiB caps, behind `QUARRY_EDGE__JOB_RESULTS`). 75 lines of new tests in `tests/job_results.rs`. The commit does not say whether they ran, so mark it **B**, T unconfirmed. | Lane a moves on. Update the ORCH row. |
| C2 | Builders commit on lane branches cut from `quarryv2` | `ad9ff1a7c` was committed straight onto `quarryv2`, and no `w4/*` branches exist. | Either cut `w4/lane-{a..d}` from `ad9ff1a7c` now or record that W4 commits go straight to `quarryv2` (one lane per file still holds). |
| C3 | B3: "ADR 5, 6, 7, 9, 10 have no recorded decision" | **ADR-6 was ratified on 2026-10-28** (decision round 3.8, owner "yes" to all 13 lines): action lists are closed, declarative and capped, with no Evaluate. `on` needs a verified Model caller plus approval. | E4.2's "stored actions are declarative" condition is met. F-t2-063 (code-mode browser) stays parked because ADR-6 forbids it. ADR 5, 7, 9 and 10 are still unrecorded. |
| C4 | B2(a), "results are an index", is only unrecorded | X-9 was accepted with direction C. Its D9 part (results source) is listed as "waits" in the implementation status, but INNH-12 and `ad9ff1a7c` already ship `kind: page_index`. | The B2(a) ledger entry only has to record what is already built. |
| C5 | E4.4-a owns `routes.rs`, `contracts.rs`, `chromiumoxide.rs`, `observation.rs` | The scrape options are **not** in `routes.rs` any more. They live in `quarry-core/src/scrape.rs:500` (`ScrapeOptions`, `OPTION_KEYS: [_; 12]` pinned by a unit test, `RESERVED_REJECTED` with `mobile`, `proxy` and `headers` at `:550`). Per-render hints go through `quarry-runtime/src/driver.rs:85-116` (`RenderHints`), because `Driver::fetch` (`driver.rs:131`) takes only a URL. | Lane b's ownership list must add `scrape.rs`, `driver.rs`, `browser_driver.rs`, `pipeline.rs` and `profile_restore.rs` (section 3). |
| C6 | E4.1-5 is in `VerevonIngestionsPage.tsx` | `RunResults` is in `features/ingestions/components/IngestionsRunsView.tsx:581`. `ingestionsCreateRunInput` (`action-registry.ts:1039`) has no `ingest` field, and the gateway `ingestions` domain does not forward one. Only `knowledge/quarry.rs:807-811, 1032-1035` forwards `ingest`. | Lane d's slice is a contract change plus a forward in a file it already owns (section 2, d). |
| C7 | E4.3-056b goes "in `content_route.rs`" | `content_route.rs` (68 KB) is the document-parse and Office route and has no URL handling. Its only call site would be `pipeline.rs`, which lane b owns. | Lane c builds the table as a pure module this step and wires it next step (section 2, c). |

## 1. Gates ranked (cheapest unblock and widest fan-out first)

| Rank | Gate | Kind | Blocks | State | Unblocking work (effort) |
|---|---|---|---|---|---|
| 1 | **B2: three ledger entries**: (a) results are an index; (b) the procedure-store owner is capability-core; (c) the provider-catalog owner is integration-corev2 | owner ratify | (a) the E4.1 `kind` contract wording; (b) E4.2-3; (c) E4.3-043 s4 | open, defaults recommended | docs lane drafts all three (0.5 d). The owner only says yes or no. (a) records what is already built. |
| 2 | **ADR 10**: proxy chaining versus the loopback-only `PinnedEgressProxyEndpoint` (`quarry-browser/src/lib.rs:331-360`); remote CDP; per-org pools | ADR | all of E4.5 (060 step 2, 059); E4.4-c tiers (together with ADR 5); WS3.G beyond K=1 (D1-075) | no record | memo with measurements: RSS per Chromium slot, sidecar-proxy feasibility (2-3 d). The measurements need a running Chromium, so the memo can be drafted now but closes with the stack session (B6). |
| 3 | **B5: `model-plane` (18 EP commits) is not on main** | merge order | every Model Plane W4 slice: 043 s1-s3 (`runtime_loop/agent.rs`; EP-11 `skill_versions.rs`), the MP half of 062a (EP-12 S1 origin/parent), 062b, E4.2-3 (EP-11 versioned skills) | open | MP controller merges `model-plane` to main. The Quarry lanes edit no MP file until then. The MP asks wait in ORCH section 6. |
| 4 | **B4: execution-core's Quarry token is ZDR**, and the edge refuses durable jobs under ZDR and refuses service principals on `/v1/crawl`, `/v1/batch` and the job routes | owner (Control + Ingestion) | E4.3-055c end to end; Model Plane reads of E4.1 results (also closed by `auth.rs:504-545`) | open | a decision: a non-ZDR "acquisition" audience for agent-initiated crawls under user approval, or crawls go only through the gateway under the user token (the plan's 055c already says user token). The second needs no Control Plane PR. **Recommend the user-token route.** That reduces B4 to "MP reads go through the gateway". |
| 5 | **X-7 / DR-15.1**: owner of the Quarry-to-Model-Plane extraction lane | owner | the E4.1 dataset `kind` (needs WS5.I + WS16); E4.7-065; 062b output-schema validation | `proposed` (`MP-ASKS.md:79`), owner unnamed | the owner names the lane owner. Nothing in W4-S1 needs it. |
| 6 | **ADR 5**: identity and escalation stance | ADR | E4.4-c tiers; E4.4-a's UA, `Accept-Language` and `device` parts; E5.2-066; the per-tier part of 067 | no record | owner ruling. It does **not** block E4.4-a's locale and timezone part (section 2, b). |
| 7 | **X-9 Control Plane asks** (a consumer row on `interactive_retention.enabled`, and the plan-change emit) | other plane | going live with scheduled saved jobs (E4.1-6 schedules), not building them | direction accepted 2026-10-28; the ask is drafted, not sent | owner sends `X-9-control-plane-ask.md`. E4.1-6 builds dark regardless (`QUARRY_SCHEDULED_JOBS_ENABLED=0`). |
| 8 | **B8: inherited W3 items**: openapi route coverage fails on 3 routes (hit-test, `/v1/audit/page`, user receipts); `web.*` ids have no action-registry rows; `CRAWL_APPROVAL_MODE=observe` | fill-in | E4.7 coverage green; 055c | open | lane a fill-in (S, docs only); lane d with 055c |
| 9 | **B6: nothing proven live** | stack session | moving any row from T to P | open | one stack session (large; outside W4-S1) |
| 10 | **B7: stale migration numbers** (session-core is now ≥0113, capability-core ≥0038) | arbiter | 062b, E4.2-3 | trivial | renumber at merge |
| 11 | **ADR 7** (MCP, CLI, n8n) | ADR | parked E4.7 items only | no record | record the plan's "delete CLI/n8n, park MCP" as a decision note (0.25 d). Low value. |
| 12 | **ADR 9** (routines, scheduled lane without a human bearer) | ADR | F-t2-061, parked | no record | none until 061 is unparked |

## 2. Slices ranked (buildable now first)

Effort is in days. Dependencies marked "on main" are already in `quarryv2`.

### Tier 1: buildable now, no gate

| Rank | Slice | Lane | Effort | Depends on | Hot files | Flags and risks |
|---|---|---|---|---|---|---|
| 1 | **E4.4-a1: per-request `locale` + `timezone`** (scrape path) | b | M, 4-5 | WS3 S2 (`apply_page_hygiene`, `chromiumoxide.rs:4860`; on main); WS17.C2 `ProfileEmulation` / `apply_profile_emulation` (`profile_restore.rs:441, 858`; reuse it, do not duplicate it) | `quarry-core/src/scrape.rs` (two new `OPTION_KEYS`, so the array goes from 12 to 14 and its pin test changes; `resolve`), `quarry-runtime/src/driver.rs` (`RenderHints.emulation`, `needs_browser()`), `pipeline.rs`, `browser_driver.rs`, `chromiumoxide.rs` (scrape page: apply after hygiene) | New flag `QUARRY_EDGE__REQUEST_CONTEXT=off`. Validate against a closed allowlist (BCP-47 locale, IANA zone); no new crate unless approved. **Cache:** an emulated scrape must bypass the URL result cache and the page cache, or partition them by key. The `max_age` read must not serve a different locale. **Static path:** emulation forces a browser (`needs_browser`). With no browser it is a typed 400 `unsupported_option`, never a silent ignore. Browserbase and Kernel fail closed. **Out of a1 (ADR 5):** UA override, `Accept-Language` through `setUserAgentOverride`, `device` (`mobile` is in `RESERVED_REJECTED`, so moving it changes the contract), and `location.country` / geolocation. Regenerate the goldens: scrape option schemas (`UPDATE_SCHEMAS=1`), openapi-scrape-parity and `openapi.yaml` ScrapeRequest. Crawl: the new keys land in the "unknown" branch of `crawl_scrape::check` (strict refuses them; lenient lists them, never applies them), so `crawl_scrape.rs` stays untouched. Guard: `cargo check -p quarry-browser --features chromiumoxide --tests`, plus one live `#[ignore]` test. ZDR: no persistence, nothing changes. |
| 2 | **E4.3-056b: public-URL rewrite table** (pure, not wired) | c | S-M, 2-3 | none (WS5 content routing is on main) | **new** `quarry-runtime/src/public_url_rewrite.rs` plus one `lib.rs` line. `content_route.rs` stays out (C7). | New flag `QUARRY_RUNTIME__URL_REWRITE=off`. Table: Docs `…/document/d/{id}/…` to `/export?format=txt`, Sheets to `format=csv` (with `gid`), Slides to `format=txt`, GitHub `blob` to `raw.githubusercontent.com`. Ids must match a closed character class; every other path returns `None`. **Never** rewrite to an authenticated or API host (`googleapis.com`, `api.github.com`). The rewrite must run **before** SSRF preflight, the robots gate, `domains.deny` and egress, so the new host (raw.githubusercontent.com) is checked on its own. The output keeps the original `url` and adds `fetched_url`. The cache key is decided at wiring time. Unit tests per pattern, no network. **Wiring** in `pipeline.rs` is a W4-S2 unit (lane b releases `pipeline.rs`, or lane c takes it in S2). |
| 3 | **E4.1-5: "Ingest selected"** | d | S-M, 2 | E4.1-3 `RunResults` (on main) | `action-registry.ts` (add `ingest: z.boolean().optional()` to `ingestionsCreateRunInput`, `:1039`; no new id, so very likely no admission-golden change, but regenerate to confirm); gateway `domains/ingestions/**` (forward `ingest` on create_run like `knowledge/quarry.rs:810`); `IngestionsRunsView.tsx` (row checkboxes, current page only, ≤ 50) | The action contract comes first. Selected rows become `{kind:'batch', urls, ingest:true}` under the user token. Batch is refused under ZDR (403), so show the typed refusal; never hide it silently. Selection is capped to the batch URL limit. Label: re-fetch and ingest (the index has no content; `kind: page_index`). New flag `VITE_INGEST_SELECTED=false`. Guard: vitest, `tsc6` baseline 1, gateway `cargo check --tests`. Do not edit `knowledge/quarry.rs` (outside lane d). |
| 4 | **E4.1-6: saved definition = `Source` with typed, versioned options** | a | M, 4 | E4.1-4 (done); WS15 S2 schedule firing (on main, dark); reads `crawl_scrape::check` / `crawl_request::parse_body` read-only (D14a validators) | `resource_routes.rs` (source create/update validation), quarry-control `resources.go` and store | Flag off. Options carry `{version:1, kind, options}`. The edge validates with the existing D5/D14a validators. Do **not** change their public API this step (lane b's new `ScrapeOptions` keys arrive through the same validators, so merge after lane b or tolerate unknown keys). Schedules point at the source through `target_ref`. Going live waits for X-9 (gate 7). |
| 5 | **B8 fill-in: openapi route coverage green** (3 undocumented routes) plus agent-route coverage | a | S, 1 | none | `openapi.yaml` (append-only section) | docs only; takes `node --test scripts/tests/openapi-route-coverage.test.mjs` from RED to green |
| 6 | **E4.5 / ADR 10 memo draft** | doc | S, 1 (draft) | the measurements need B6 | `docs/` decision file | no code |
| 7 | **B2 ledger entries** (a)(b)(c) | doc | S, 0.5 | none | `docs/` / MP `ledger.md` (MP-ASKS for the capability-core one) | owner ratifies |

### Tier 2: buildable after a Tier 1 slice (same lane, next step)

| Slice | Lane | Effort | After | Note |
|---|---|---|---|---|
| E4.3-056b wiring in `pipeline.rs` | b or c | S, 1 | 056b table plus lane b's E4.4-a1 merge | one owner of `pipeline.rs` per step |
| E4.4-b: `profile_id` through `ProfileStore` (D16 owner guard, `observe`); a personalised scrape bypasses the cache | b | M, 5 | E4.4-a1 (same `RenderHints` field) | org-keyed cache (WS4.K) is done; D16-Q2 enforce waits on its observe week |
| E4.2-1: step-level `expect` postconditions | b | M, 6 | E4.4-a1 (frees `contracts.rs` / `observation.rs`) | WS3 S10, WS11 stage 2 on main; ADR-6 satisfied (C3) |
| E4.2-2: canonicalise snapshot refs to `ClickSemantic` / fingerprint, `TARGET_REPAIR_REQUIRED` | b | M, 6 | E4.2-1 | `ClickSemantic` at `contracts.rs:847` |
| E4.1-7: delete Go presets, rename to "saved job" | a | S, 1 | owner pick (recommend delete; `fast` sets `RobotsIgnore`) | `presets.go` is still mounted |
| E4.6-062a edge half (`usage_attrs.rs` accepts an advisory parent-run id from service tokens only) | c | S, 2 | header name agreed with EP-12 S1 (gate 3) | build the edge half against a named header only once MP fixes it |
| E4.3-055c UI half (`CrawlPagePicker.tsx` approval) | d | M, 3 | gate 4 decision (user-token route) + `web.*` registry rows (B8) | |
| E4.3-056c Notion page-content action (integration-corev2) | Ingestion (integration) | S-M, 3 | none (not a W4 lane file) | unassigned; low priority |

### Tier 3: gated by a decision or another plane

| Slice | Gate (rank in section 1) |
|---|---|
| E4.2-3 store + runner | B2(b) (1), B5 (3), B7 |
| E4.3-043 s1, s2, s3 | B5 (3); s3 also needs EP-11 merged |
| E4.3-043 s4 catalog | B2(c) (1), B5 (3) |
| 056a larger read cap | B5 (3) (an MP file) |
| E4.6-062b durable research run | B5 (3), B7, X-7 for schema validation (5) |
| E4.4-a2 (`device`, `location.country`, UA, `Accept-Language`) | ADR 5 (6) |
| E4.4-c egress tiers | ADR 5 (6) + ADR 10 (2) |
| E4.5 all | ADR 10 (2); 059 also needs E5.2-074 `ProviderError` |
| E4.1 dataset `kind` | X-7 (5) + WS5.I + WS16 |
| 056d live Google/Microsoft verification | B6 (9) |

### Tier 4: parked or deleted per plan
QuarryPack and schema forms (E4.1); 057 engine planner v2; SessionPool; 060 step 3; 054 (deleted, absorbed by WS8/WS2/WS3); 061 (ADR 9); 063 (ADR-6 forbids it); 064 SDK helpers, MCP, CLI, n8n (ADR 7); 065 (ADR-6, WS16, cost).

## 3. File ownership for W4-S1 (checked against the code)

| File | Owner in S1 | Why / conflict |
|---|---|---|
| `quarry-edge/src/routes.rs` (3855 lines, hotspot) | **lane b, but expected untouched.** `ScrapeRequest` (`:536`) takes options from `ScrapeOptions`, so E4.4-a1 should not need it. | Nobody else edits it. If lane b must, it is the only editor. Lane a's results work stays in `job_results_routes.rs`. Any new `AppState` field must update all four full literals (plan E4.5 note), so avoid one in S1. |
| `quarry-browser/src/chromiumoxide.rs` (13363 lines) | **lane b only** | E4.4-a1 applies emulation on the scrape page next to `apply_page_hygiene` (`:1516`, `:4860`). No other lane touches the driver. Feature check is mandatory. |
| `quarry-browser/src/profile_restore.rs` | lane b (reuse; at most a `pub(crate)` widening) | `apply_profile_emulation` `:858` |
| `quarry-edge/src/crawl_request.rs` / `crawl_scrape.rs` | **nobody edits in S1.** Lane a reads them only (E4.1-6 validation). | New scrape keys fall into `check`'s unknown branch (`crawl_scrape.rs:146-170`), which is safe. Adding them to `UNSUPPORTED_KEYS` (`[_; 14]`, `:58`) for a clearer code is an S2 item for whoever owns it then. |
| `quarry-core/src/scrape.rs` | lane b | `OPTION_KEYS` pin, `RESERVED_REJECTED` unchanged |
| `quarry-runtime/src/driver.rs`, `browser_driver.rs`, `pipeline.rs` | lane b | `RenderHints` is the per-request channel (WS13.5 precedent). Lane c must not edit `pipeline.rs` in S1. |
| `quarry-runtime/src/public_url_rewrite.rs` [NEW], `content_route.rs` | lane c | `lib.rs` gets one `pub mod` line; keep both lanes' additions at integration |
| `quarry-edge/src/job_results_routes.rs`, `resource_routes.rs`, quarry-control `resources/*` | lane a | |
| `action-registry.ts`, `features/ingestions/**`, gateway `domains/ingestions/**` | lane d | the admission golden is regenerated at integration (4 MP copies, sha pin, `zdr-contract-sync.test.sh` line) only if it changes |
| `openapi.yaml`, `CONTRACTS.md`, `RUNBOOKS.md` | shared, append-only | one section per unit |

## 4. Next buildable slice per lane

| Lane | Done | Next (W4-S1) | Then (S2) |
|---|---|---|---|
| a | E4.1-1..4 (`ad9ff1a7c`) | **E4.1-6** saved definitions (M 4), with the B8 openapi fill-in (S 1) first if lane a wants a quick win | E4.1-7 after the owner pick |
| b | none | **E4.4-a1** locale + timezone (M 4-5) | E4.4-b profile pinning, or E4.2-1 postconditions |
| c | none | **E4.3-056b** rewrite table, pure, unwired (S-M 2-3) | wiring in `pipeline.rs`; 062a edge half after the EP-12 header |
| d | E4.1-3 (gateway + `RunResults`) | **E4.1-5** Ingest selected (S-M 2) | 055c UI after the gate 4 decision |
| doc | none | B2 ledger entries (a)(b)(c), ADR 10 memo draft | ADR 7 decision note |

## 5. Top 3 buildable slices

1. **E4.4-a1: per-request locale and timezone (lane b, M 4-5 d).**
   - Dependencies: WS3 S2 and WS17.C2 `ProfileEmulation`, both on main. No ADR gate for locale and timezone.
   - Flags and risks: `QUARRY_EDGE__REQUEST_CONTEXT=off`. The cache must bypass or partition. A static request with emulation forces a browser or gets a typed 400. Cloud drivers fail closed. UA, `device` and geolocation are held for ADR 5 (`mobile` is a reserved key). Regenerate the schema goldens and openapi parity. Run the chromiumoxide feature check.
   - Ownership: lane b alone owns `scrape.rs`, `driver.rs`, `pipeline.rs`, `browser_driver.rs` and `chromiumoxide.rs` this step.
   - Unlocks E4.4-b, E4.2-1 and E4.2-2.
2. **E4.1-5: Ingest selected (lane d, S-M 2 d).**
   - Dependencies: E4.1-3 on main.
   - Flags and risks: the contract change comes first (`ingest` on `ingestionsCreateRunInput`). Forward `ingest` in the gateway `ingestions` domain. `VITE_INGEST_SELECTED=false`. A ZDR 403 is surfaced, not hidden. Selection is capped to the batch limit. The rows are labelled a re-fetch, because the index has no content. Regenerate the golden at integration if it changes.
   - Closes E4.1's user-visible loop. No collision with any Quarry lane.
3. **E4.3-056b: public-URL rewrite table (lane c, S-M 2-3 d).**
   - Dependencies: none.
   - Flags and risks: `QUARRY_RUNTIME__URL_REWRITE=off`. Build it as a pure new module with closed id patterns and no authenticated or API hosts. It must run before SSRF, robots, deny and egress, so the rewritten host is checked on its own. The original `url` is kept and `fetched_url` added.
   - Wiring waits for `pipeline.rs` to be free in S2.
   - First Track B step of E4.3 that needs no ledger entry and no MP file.

Close runner-up: **E4.1-6** (lane a, M 4). It is ranked fourth only because it reads the validators lane b is extending, so it should merge after E4.4-a1.

Decisions to put to the owner now, all cheap: B2 (a)(b)(c) ratification; B4 → "agent crawls go only through the gateway under the user token"; E4.1-7 "delete Go presets"; send the X-9 ask; name the X-7 lane owner.
