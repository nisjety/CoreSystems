# W4 orchestration, round 2: Model Plane EP-1..EP-14 versus Quarry W4 files, and the next slices

Date: 2026-11-08. Branch `quarryv2` at `de9678470`, worktree `gracious-gagarin-087139`.
Supersedes the Model Plane section (6) of `W4-ORCH.md` and corrects `W4-BLOCKERS-RANKED.md` where noted. Those two files stay as the history.
Method: git only (`main..model-plane`, `main..quarryv2`, `main..quarryv2-next`, working trees of `epic-shaw-4d613b` and this worktree) plus reads of the cited code. I ran no tests and no builds. Lane b's writer was active in this worktree while I worked.

Naming: **Q-W4** means Quarry Wave 4 (E4.1 to E4.7, this file). **MP-W4** means the Model Plane program's own "Wave-4 platform" cells (`feat(mp-w4a..f)`). They are different programs that share a number.

## 0. What changed since `W4-BLOCKERS-RANKED.md` (b696f4d22)

| # | Change | Effect |
|---|---|---|
| N1 | Lane b committed **E4.4-a as a checkpoint** (`de9678470`, 2026-11-08 02:32): keys `locale`, `timezone`, `location` behind `QUARRY_EDGE__PER_PAGE_LOCALE` (off). New `quarry-core/src/page_emulation.rs`, `quarry-browser/src/page_emulation_live.rs` (`#[ignore]`). 15 files. The commit says "not yet verified green". | E4.4-a is **B, not T**. The next lane b commit must take it to green. |
| N2 | Lane b's file set is wider than the ranked ownership list: it also edits `quarry-edge/src/scrape_actions.rs`, `scrape_contract.rs`, `routes.rs` (+44) and `quarry-browser/src/lib.rs`. It did not touch `pipeline.rs` or `crawl_scrape.rs`. | Add those four to lane b's S1 list. Lane a must not edit `scrape_contract.rs` / `scrape_actions.rs` while lane b is open. |
| N3 | **Scope drift to rule on:** the slice ships `location` as a geolocation override (lat/long/accuracy for `navigator.geolocation`). `W4-BLOCKERS-RANKED.md` put `location` / geolocation outside a1, behind ADR 5. | Owner or reviewer call before lane b's green commit; see section 3, rank 1. |
| N4 | **ADR-5 option (a), honest identity, is decided** (recorded in `quarryv2-next:.plan-work/quarry-w1w2/W5-ORCH.md` section 1, "do not reopen"; `escalation.rs:34` pins `Honest`). Open sub-decisions: D-W5-1 (one product identity string), D-W5-2 (per-tier UA stance, which must be settled before E4.4-c). | Corrects ranked gate 6 ("no record"). E4.4-a2 (UA, `Accept-Language`) and E4.4-c wait on D-W5-2, not on all of ADR 5. The W5 guard rules (no UA toward a browser identity, no fingerprint masking) apply to every W4 slice too. |
| N5 | `model-plane` is now **28 commits** ahead of `main` (was 18), tip `5fbbe2c79` (2026-11-07 22:13). `main` is an ancestor (`model-plane..main` = 0), so the merge to main would be a fast-forward. It now carries EP-12 "S1 export", MP-W4 a/b/c/d/f and W5 069/078/079/086. | Section 1 and 2. |
| N6 | **E4.1-5 does change the admission golden.** `actionSchemaHash` (`catalog-manifest.ts:82`) hashes `inputSchema`, and `ingestions.create_run` has hash basis `catalog_entry` (`agent-action-admission.json:1405`). Adding `ingest` changes its `schemaHash`. | Corrects ranked Tier 1 rank 3 ("very likely no golden change"). Collides with `mp-w4b`, which also rewrote the golden; see section 2. |
| N7 | **062a is no longer blocked by "EP-12 header".** EP-12 D1 (`docs/plans/wave3/EP-12-sharing-export-and-agent-api.md`, ledger line 674 on `model-plane`) fixes run origin as derived from the verified caller, never a request field. Origin inheritance (`SESSION_RUN_ORIGIN_INHERIT`, `MODEL_GATEWAY_RUN_ORIGIN_FIELD`) is already on `main`. `x-request-id` already travels on every `quarry_agent.rs` call (`:2067`), and the edge adopts it behind `QUARRY_EDGE__REQUEST_ID_ADOPT` (`request_id.rs:28-30`). | The edge half of 062a only needs one header name for an advisory parent-run id. It moves from Tier 2 "after EP-12" to Tier 1 for lane c, with a one-line MP ack. |
| N8 | Migration numbers taken on `model-plane`: session-core **0113** (`0113_event_triggers.sql`, MP-W4f), capability-core **0038** (`0038_computer_tool_capabilities`, MP-W4c). | B7: 062b uses session-core ≥ **0114**; E4.2-3 uses capability-core ≥ **0039**. |
| N9 | `quarryv2-next` (W5) is **1287 commits behind `main`** (merge base `cc8c43389`). It has F-t2-074 ProviderError (`f90ed49a3`: `browserbase.rs`, `browserless.rs`, `kernel.rs`) and a `global.css` deletion. | The E4.5 059 prerequisite (F-t2-074) exists, but only on a stale branch. When lane b makes cloud drivers fail closed for emulation, it must not edit those three files until W5 is rebased. |
| N10 | MP-W5-079 added the **Control Plane flag registry** (`apps/Control Plane/flag-registry`). It scans only `apps/Model Plane/deploy/docker-compose.yml`: every `_ENABLED`/`_MODE`/... variable set there must be registered or listed in `unregistered`. | Any Q-W4 slice that adds a Model Plane compose switch must add a registry entry in the same commit. Quarry-only flags are not scanned. |

Committed-file overlap today: `main..quarryv2` (21 files) ∩ `main..model-plane` (196 files) = **none**. `quarryv2` ∩ the 5 `quarryv2-next` commits = **none**. `model-plane` ∩ `quarryv2-next` = `src/styles/global.css` only (not a W4 file). Every conflict below is a **planned** Q-W4 slice against already-landed or in-flight MP work.

## 1. Model Plane EP-1..EP-14 status (from git; the board is stale)

The controller board (`epic-shaw-4d613b/docs/plans/wave3-5-execution-control.md`, last written 2026-11-07 04:59, untracked) still shows C6, C7 and C10-C14 as "dispatched". Git:

| Epic | Cell | Commits on `model-plane` | Verdict on board | In flight (uncommitted, `epic-shaw-4d613b`) | Contact with Q-W4 |
|---|---|---|---|---|---|
| EP-1 browser takeover | C6 | `58ca2d8a9`, `724f8aae8` | none | **EP-1 S3** person's note on a released call: `execution-core/src/runtime_loop/agent.rs` (about 150 changed lines), `browser_session.rs`, `browser_events.rs` | **Hard:** `runtime_loop/agent.rs` = 043 s1 target |
| EP-2 web reading S1-S3 | C4 | `8d7d38fbe` | PASS (3 cross-cell zdr_posture fails) | | Semantic: model-gateway `tool_loop.rs`, `page_reader.rs` add `read_page`/`find_in_page`. 043 s1's `SNIPPET_WEB` and web tool descriptions must build on them, not beside them. No shared file with 043 s1/s2. |
| EP-3 S4 revision store | C7 | `794aae5b0`, `09db920a8` (same title twice) | none | | none |
| EP-4 inline + MCP Apps | C8 | `282a07810`, `c3ff2128b` | PASS | | none |
| EP-5 deck | C9 | `b8b720e19` | PASS | | none |
| EP-6 automations phase 1 | C5 | `49666f070` | PASS | | Semantic: user-facing schedules. E4.1-6 saved-job schedules stay Quarry-internal (WS15 `target_ref`); no second schedule UI in `features/ingestions`. |
| EP-7 S1 persistent goals | C10 | `6432012f5` | none | | none (FE `global.css` only) |
| EP-8 S2 containment | C3 | `345311759` | PASS | | none |
| EP-9 S1 egress tiers | C11 | `16e70e5f9`, `df513628e` | none | | Name only: code-sandbox tiers (`mp-contracts/egress_tiers.rs`) are not E4.4-c browser egress tiers. Do not reuse the type. |
| EP-10 S1 connector suggestion | C12 | `2af78fea2` | none | | none: `integration_tools.rs` (056a) is untouched |
| EP-11 S1 versioned skills | C13 | `d2fb40573`, `e8f556835` | none | | **Hard:** session-core `skill_versions.rs`, `skill_versions.proto` = 043 s3 `url_patterns`. **Design:** EP-11 versioned skills vs E4.2-3 procedure store (B2(b)). |
| EP-12 sharing/API | C14 | `86bf418cd` "S1 export" | none | | The commit is share-snapshot retention and erasure (`thread_shares.rs`, `gdpr.rs`, `retention_sweeper.rs`), which is not the epic doc's S1 ("origin on every run"). Origin S1 is already on `main`. Unblocks 062a (N7). |
| EP-13 S1 picker + list price | C1 | `cc4566b70` | PASS | | none |
| EP-14 S1 settled-turn folding | C2 | `16673db40` | done (`mp-ep14-s1-gap` `a480f985d` superseded) | | none |

Beyond EP-1..14, now on `model-plane`:

| Commit | Item | Contact with Q-W4 |
|---|---|---|
| `b7740b301` MP-W4a R1 replay scrubber | `BrowserChrome.tsx` | none (lane d already excludes `BrowserChrome.tsx`) |
| `a9b084e72` MP-W4b P1 viewer registry | **`action-registry.ts`**, `model-access.ts`, **admission golden (FE + 4 MP copies + `.sha256`)**, `zdr-contract-sync.test.sh`, `vite-env.d.ts` | **Hard** with E4.1-5 (N6) |
| `5fbbe2c79` MP-W4c C1 typed computer connector | capability-core **0038**, **`zdr-posture-matrix.json`** (+ 3 vendored copies, pins), `runtime_loop/mod.rs`, `fold.rs`, `tool_bridge/mod.rs` | Any MP W4 slice adding a ZDR row (062b) moves the same matrix pins. Migration number (N8). |
| `0a419e35c` MP-W4d M1a/M1b org policy + CSP | `org-agent-policy-schema.json`, `org_agent_policy.rs` | Design note: if E4.4-c needs an org egress ceiling, check whether it belongs in this schema (MP contract) or in Quarry's D7 `JobPolicy`. Not a W4-S1/S2 item. |
| `729dd8599` MP-W4f T1 trigger record | session-core **0113**, `event_triggers.proto` | Migration number (N8) |
| `e2553e3b0` W5-079 typed flag registry | Control Plane `flag-registry/*` | N10 |
| `430a3226c` W5-069 rewind side chat | `sessions.proto`, `message_tree.rs`, gateway `chat*.rs` | `sessions.proto` = 043 s3 target |
| `6653d22fb` W5-078, `52313341f` W5-086 | FE chat/agents, model-gateway review run | none |
| in flight, staged | W5 reader tools (`ReaderTools.tsx`, `chapter-toc.ts`, `ChatMessages.tsx`, `ChatPage.tsx`, `vite-env.d.ts`) | `vite-env.d.ts` only (append) |

## 2. Q-W4 slice versus MP file conflict matrix

"Free" means no landed or in-flight MP commit touches the file. "Wait" means the slice must merge after `model-plane` reaches `main`, or be done by an MP cell on `model-plane` itself.

| Q-W4 slice | Lane | Files | MP conflict | Verdict |
|---|---|---|---|---|
| E4.4-a (checkpoint) | b | Quarry only (N1, N2) | none | free |
| E4.4-b, E4.2-1, E4.2-2 | b | Quarry only | none | free |
| E4.3-056b table, then wiring | c, b | `quarry-runtime/src/public_url_rewrite.rs` [new], `lib.rs`; `pipeline.rs` in S2 | none | free |
| E4.6-062a edge half | c | `quarry-edge/src/usage_attrs.rs` (or `request_id.rs`) | none | free; needs the header-name ack (N7) |
| E4.1-6 saved definitions | a | `resource_routes.rs`, quarry-control `resources/*`, store | none | free; merge after lane b green (validators) |
| E4.1-7 delete Go presets | a | `presets.go` | none | free; owner pick |
| B8 openapi coverage | a | `openapi.yaml` (append) | none | free; lane b also appended to `openapi.yaml` in `de9678470`, keep both sections |
| **E4.1-5 Ingest selected** | d | `action-registry.ts` (`:1039`), admission golden ×5 + `.sha256`, `zdr-contract-sync.test.sh` pin, gateway `domains/ingestions/**`, `IngestionsRunsView.tsx`, `vite-env.d.ts` | **MP-W4b**: same registry file (different hunks, `:1541`, `:5100`), same golden and sha pin, same sync script; `vite-env.d.ts` (MP-W4a/b, W5-078, EP-14, in-flight reader tools) | **build now, golden at the second merge**: whichever of `quarryv2` / `model-plane` reaches `main` second regenerates the golden from the merged registry and re-copies the 4 MP fixtures and the sha. Never hand-merge the JSON. |
| E4.3-055c UI half | d | `CrawlPagePicker.tsx`, `action-registry.ts` (`web.*` rows, B8) | `action-registry.ts` + golden as above | same rule as E4.1-5; still gated by B4 |
| 043 s1 descriptions + `SNIPPET_WEB` | MP | `execution-core/src/runtime_loop/agent.rs` | **EP-1** (landed) + **EP-1 S3 (in flight)** | **wait** until EP-1 S3 commits; then an MP cell on `model-plane` |
| 043 s2 `driver_capabilities` into PlannerView | MP | `execution-core/src/browser_agent/planner_view.rs` | none | **free for an MP cell now** |
| 043 s3 `url_patterns` on `AgentSkill` | MP | `sessions.proto`, `skill_versions.proto`, session-core `skill_versions.rs` | **EP-11 S1**, **W5-069** | wait; design with EP-11 (a skill version carries `url_patterns`, not the base row) |
| 043 s4 catalog | MP + Ingestion | integration-corev2 | none | gated B2(c) |
| 056a larger read cap | MP | `execution-core/src/integration_tools.rs:57` | none | **free for an MP cell now** |
| 062a MP half (request id + advisory parent run on web reads) | MP | `execution-core/src/web_tools.rs`, model-gateway `quarry.rs` | none (both untouched on `model-plane`) | **free for an MP cell now**, same header as the edge half |
| 062b durable research run | MP | session-core migration ≥0114, run record; ZDR matrix row | MP-W4f (0113), MP-W4c (matrix pins) | wait; renumber; matrix row moves with MP-W4c's pins |
| E4.2-3 store + runner | MP | capability-core ≥0039 `internal/procedures`; execution-core planner mode | MP-W4c (0038); EP-11 (concept) | gated B2(b); decide against EP-11 first |
| E4.4-c egress tiers | — | Quarry `EgressBroker`; maybe org policy | name clash EP-9; possible MP-W4d schema | gated D-W5-2 + ADR 10 |

## 3. Next slices, ranked

Rules unchanged: build dark (new flag, default off), no live flips, local commits only, one owner per hot file per step. W4 commits go straight onto `quarryv2` (no `w4/*` branches exist; ranked C2 is settled that way in practice).

| Rank | Slice | Lane | Effort | Why now | Done when |
|---|---|---|---|---|---|
| 1 | **E4.4-a to green**, plus a ruling on `location` | b | S, 1 | It is already written (`de9678470`). Lane a's E4.1-6 and lane b's S2 wait on it. | `cargo check --workspace --tests`; `cargo check -p quarry-browser --features chromiumoxide --tests`; `quarry-core` and `quarry-edge` lib tests; scrape option schema goldens (`UPDATE_SCHEMAS=1`) and openapi-scrape-parity regenerated; ZDR unchanged. **`location` ruling:** either keep it (an explicit, caller-requested override recorded in the receipt, applied with the honest UA; argue in the commit why it is not W5 guard rule 2 "fingerprint masking") or drop it to E4.4-a2. The cache bypass and the static-path 400 from the ranked slice still apply. |
| 2 | **E4.3-056b public-URL rewrite table** (pure, unwired) | c | S-M, 2-3 | No conflict with anything, Quarry or MP. | As ranked: `QUARRY_RUNTIME__URL_REWRITE=off`, closed id patterns, never an authenticated or API host, unit tests without network. |
| 3 | **E4.1-5 Ingest selected** | d | S-M, 2 | Closes E4.1's user loop. No Quarry conflict. | As ranked, plus: the golden **does** change (N6). Regenerate it on `quarryv2` for the test run, and record in the commit that the golden must be regenerated again at whichever merge comes second (section 2). Put `VITE_INGEST_SELECTED` at the end of `vite-env.d.ts`. |
| 4 | **B8 openapi route coverage green**, then **E4.1-6 saved definitions** | a | S 1, then M 4 | B8 is a quick, docs-only win. E4.1-6 starts after rank 1 is green, because it reads the validators lane b just extended. | B8: `node --test scripts/tests/openapi-route-coverage.test.mjs` green. E4.1-6 as ranked. Lane a stays out of `scrape_contract.rs` / `scrape_actions.rs`. |
| 5 | **E4.6-062a edge half** | c (after 056b) | S, 2 | Unblocked by N7. | The edge accepts one advisory parent-run header from **service principals only**, length-capped like `request_id.rs` (`MAX_INBOUND_LEN` 128), stored as a usage attribute only, never as authority or origin. Under ZDR it is dropped unless the owner adds it to `ZDR_ALLOWLIST` as an opaque id. Proposed name `x-verevon-parent-run`, next to `x-verevon-surface` (`usage_attrs.rs:22`). New flag off. Add the MP ask (below) in the same commit. |
| 6 | E4.4-b profile pinning, or E4.2-1 postconditions | b | M 5 / M 6 | After rank 1. Pick E4.2-1 if the owner wants procedures sooner. | as ranked |
| 7 | E4.3-056b wiring in `pipeline.rs` | b (or c if b releases it) | S, 1 | After ranks 1 and 2. | rewrite runs before SSRF, robots, deny and egress; `fetched_url` added |
| 8 | Doc lane: B2 (a)(b)(c) ledger entries, ADR 10 memo draft, a D-W5-2 note that E4.4-c depends on | doc | S, 1.5 | cheap; owner only says yes or no | as ranked |

**Model Plane cells (asks, not Quarry lanes).** Three Q-W4 MP slices touch no file the EP or MP-W4 work has touched, so they do not need `model-plane` on `main` first. They can run as MP cells on `model-plane`:
1. 056a: raise `MAX_RESULT_CHARS` for the read-only `drive.file.content` rows only (`integration_tools.rs:57`).
2. 043 s2: `driver_capabilities` / `execution_tier` into `planner_view.rs`.
3. 062a MP half: `x-request-id` plus the agreed parent-run header on `web_tools.rs` and model-gateway `quarry.rs` calls.

Hold 043 s1 until EP-1 S3 commits. Hold 043 s3 and E4.2-3 for an EP-11 design pass. Hold 062b until migration ≥0114 and the ZDR matrix row are allocated against MP-W4c/f. The `W4-ORCH.md` section 6 ask list still applies with these changes. It is not yet copied into `MP-ASKS.md`. Do that when lane c starts rank 5.

## 4. Gates, re-ranked (changes only)

| Gate | Was | Now |
|---|---|---|
| B5 `model-plane` not on main | blocks every MP W4 slice | Blocks only 043 s1 (EP-1, `agent.rs`), 043 s3 (EP-11, `sessions.proto`), 062b, E4.2-3 and the **golden regeneration** for lane d. 056a, 043 s2 and 062a MP are free (section 3). The merge to main is a fast-forward. |
| ADR 5 (ranked gate 6) | no record | (a) honest identity decided. D-W5-1 and D-W5-2 are open. D-W5-2 gates E4.4-a2 (UA, `Accept-Language`) and E4.4-c. |
| 062a "EP-12 header" | Tier 2 wait | removed: EP-12 D1 on main; only the header name needs an MP ack |
| B7 migration numbers | session-core ≥0113, capability-core ≥0038 | session-core ≥0114, capability-core ≥0039 |
| new: G-GOLD | — | admission golden shared by lane d (E4.1-5, 055c) and MP-W4b: regenerate at the second merge, never hand-merge |
| new: G-STALE-W5 | — | `quarryv2-next` is 1287 behind main; F-t2-074 lives there. Lane b stays out of `browserbase.rs`, `browserless.rs` and `kernel.rs` until it is rebased. |

B2, B3 (ADR 7, 9, 10), B4, B6 and X-7/X-9 are unchanged from `W4-BLOCKERS-RANKED.md`.

## 5. Progress estimate

E4.1-4 (`ad9ff1a7c`, B) and E4.4-a (`de9678470`, B, unverified) add about 6-8 days to the 17.5 built in `W4-ORCH.md`. That makes about **24-26 of 147 scheduled days built (~17 %)**, still ~8 % tested, 0 % proven live. These are estimates: the plan gives effort per epic, not per slice.

## 6. Owner decisions, cheapest first

1. Keep or drop `location` (geolocation) in E4.4-a (rank 1).
2. Name the 062a parent-run header (`x-verevon-parent-run` proposed) and confirm it is advisory only (EP-12 D1).
3. B2 (a)(b)(c) ratification; for (b), weigh EP-11 versioned skills before choosing capability-core.
4. MP controller: take 056a, 043 s2 and the 062a MP half as cells on `model-plane`; update the stale board (C6, C7, C10-C14 have commits but no verdict).
5. Merge order: `model-plane` → `main` first (fast-forward) is cheaper. Then `quarryv2` regenerates the admission golden once.
