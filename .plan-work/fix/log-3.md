# Fixer F3 log: segment 3 (plan lines 947-1571, WS6 to WS9)

File edited: `.plan-work/fix/seg3.md` only (625 lines before, 630 after). Seg line = absolute plan line minus 946 (before my edits). Provenance re-run for every corrected path; no proposed correction was missing on disk.

Statuses: applied | applied-modified | rejected | out-of-range-skipped.

## Skeptic K1

| id | status | what was done |
|---|---|---|
| K1-01 | applied-modified | Only the WS6.G occurrence is in range: states that the shipped 12 versus 32 defaults violate the invariant, that the single consistent pair lives in 4.5 (other fixer), and that the boot check is WARN-only until the WS2.D RSS numbers exist, then FAIL. I did not invent the pair values here. |
| K1-02 | out-of-range-skipped | SC1/W1 gate text is outside. Consistency only: WS8.F and WS8.I evidence now say the real `docker-compose.production.yml`, not the contract overlay, with WS1.A0 and WS3.A landed (via K2-01). |
| K1-03 | applied | WS6.A limits.rs row, gateway row, docs row, API paragraph, tests note: `0` or absent = finite default 1000 with `clamped_by:["default"]`; strict 400 only under `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=strict` (WS14.I); gateway keeps its own existing 400. SC7 wording itself is outside. |
| K1-04 | out-of-range-skipped | SC10 (line 86). |
| K1-05 | out-of-range-skipped | SC2 / WS2.C. |
| K1-06 | out-of-range-skipped | SC6 / WS5. |
| K1-07 | out-of-range-skipped | 4.1 / X-3. WS6.A has no compose anchor row in this segment. |
| K1-08 | applied-modified | Location is outside (7.2). Added to the WS6.A heading and gateway `start_crawl` row: shares `knowledge/quarry.rs` with WS4.A in non-overlapping regions (merge WS4.A first), shares `openapi.yaml` with WS2.A, needs the PRE-1 baseline. Used "WS2.A" instead of "PR 3" because K1-09 may renumber PRs. |
| K1-09 | out-of-range-skipped | PRE-2 ordering. |
| K1-10 | out-of-range-skipped | WS2.A critical path wording. |
| K1-11 | out-of-range-skipped | WS1.A split. My dependency lines say "WS1.A" generically and stay valid. |
| K1-12 | out-of-range-skipped | WS4.H. |
| K1-13 | out-of-range-skipped | WS4.K. |
| K1-14 | applied-modified | Not named by line in my range but the same fact occurs: WS8 reconciliation line (R-11) and WS8.G files/flow. Takeover pause/resume now reuse 142 and 143 (verified in `events.proto:175-181`) with a `reason` field; only `BrowserAttachSharedJar` takes 160; sibling (b) from 161. `BrowserRunPausedByUser`/`BrowserRunResumed` kept as defined shorthand elsewhere in WS8. |
| K1-15 | out-of-range-skipped | WS13.F. |
| K1-16 | applied-modified | WS6.A 1.5 d to 3.5 d (size S to M) in slice table, WS6 header (42.5 to 44.5 d) and WS6.A heading. WS2.C, WS1.A, WS3.F out of range. WS8 critical-path sentence now notes that the 50 d sum used the old WS1.A and must be recomputed (WS1.A is 15 or 16 d per K2-17 / K1-16). |
| K1-17 | applied | WS6.D DoD now reads "exactly the credentials listed in R-13 PR 2, each read-only and org-bound where the API allows". WS4.H and R-13 parts are outside. |
| K1-18 | applied | Sentence added to WS7.I Risks: the existing resolver is precedent only for `tenant` retention, `resolveWebPolicy` is independent and must not change what any retention row means. |
| K1-19 | applied | WS6.E migration row: "gaps are tolerated, checked; entrypoint loops `/app/migrations/*.up.sql` and re-applies every file on each start, so statements stay idempotent". Item (d) of 7.1 is outside. |
| K1-20 | applied | All in-range occurrences fixed: `quarry-core/tests/contracts.rs`, `quarry-edge/tests/support/mod.rs` (WS6.D, WS8.E), and every `src/../tests/` path (crawl_limits, agent_run_budget, usage_metering, org_capacity, drain). |
| K1-21 | out-of-range-skipped | WS4.G. |
| K1-22 | out-of-range-skipped | 4.1 column name. |
| K1-23 | applied-modified | WS8.D and WS8.I: gateway `BROWSER_HUMAN_BROWSE` and `BROWSER_ATTACH` default `0` in every environment until X-13 and ADR-3 are answered; dev and evidence stacks set `1` explicitly (evidence text, test name `human_browse_flag_defaults_off_in_every_environment` updated). The flag-table row itself is outside. |

## Skeptic K2

| id | status | what was done |
|---|---|---|
| K2-01 | out-of-range-skipped | WS1.A0 is another fixer's. Dependency parentheticals added to WS8.F and WS8.I production-overlay evidence (WS1.A0 and WS3.A). |
| K2-02 | out-of-range-skipped | WS1.A files. |
| K2-03 | applied | WS6.F: six `AppState` literals with verified lines (config.rs:146, main.rs:313, browser.rs:4915, chat/shared.rs:745, orchestration.rs:632, stream_e2e.rs:110). WS8.D and WS8.I: field plus the five test-literal sites, or `AppState::test_default()` if WS1.B adds it. WS4.A and WS1.B parts outside. |
| K2-04 | applied | WS6.E, WS7.K, WS8.C: `buf generate --template buf.gen.go-only.yaml` from `apps/Model Plane/proto`; stale `browser_pb2.py` and TS gen tree listed in `apps/STALE_DOC_DELETION_REGISTER.md` (all paths verified); vendored `browser.proto` rows say hand-edited and covered by the L1 vendored-subset check, which I added to the WS8.C `browser-grant-contract.test.mjs` row; dead `tools/sync-protos.sh` reference flagged for deletion (WS1.A owns). |
| K2-05 | applied-modified | WS4.G/WS5.H scanner text is outside. In range: WS8.F "Route gates" bullet (three attach routes plus `mark` need matrix and OpenAPI rows; scan `agent_routes.rs` or `agent_routes/router.rs`, cut `#[cfg(test)]`), and a WS6.I note that the separate `/metrics` router needs no row. |
| K2-06 | applied-modified | `RunEntry`/`RunHandle` literals: WS6.B integration test, WS7.D integration test, WS8.F and WS8.H (`test_support.rs` row) now use `test_support::insert_live_run` or HTTP routes. WS2.A/C/D rows are outside. |
| K2-07 | applied-modified | Rows added to WS6.D (402 `crawl_allowance_exhausted` plus `errors.ts` row) and WS6.H (429 `crawl_backlog_full`) with tests; DoD of WS6.D mentions SPA copy. The 415 `UNSUPPORTED_MEDIA` normalisation became unscheduled follow-up L8 (0.5 d, owner Frontend Plane gateway, precondition WS5.G) rather than a scheduled slice, so no slice-table or total change. |
| K2-08 | applied | WS6.B ErrorCode row: Go mirror has 12 of 17; explicit statuses listed for the five missing codes (from `error.rs:http_status`) plus USER_TOOK_OVER 409, UNSUPPORTED_MEDIA 415, BUDGET_EXHAUSTED 402; grpc.rs/tools.rs rewritten (typed-error match 2696-2708 / 1180-1190, arm before fallback, `UPSTREAM_BLOCKED` missing in tools.rs, agent path is `quarry_agent.rs`). Reviewer's status list "(409, 403, 503, 409, 415, 402)" had six numbers for eight codes, so I wrote the code-to-status pairs from the Rust source instead. Also folds in the C3 correction that lines 2700/1184 are `permission_denied` arms. |
| K2-09 | applied | All in-range paths and line lists (jobs SELECT lines 93, 125, 160, 196, 245, 303, 361; `event_type_token` at nats_event_bus.rs:364 / arm 394). |
| K2-10 | applied | WS6.E (0003 note) and WS8.C (`absolute_expires_at` already in WS1 `0001`). |
| K2-11 | out-of-range-skipped | WS5. |
| K2-12 | out-of-range-skipped | WS3/W1 totals. |
| K2-13 | applied | WS7.C evidence mint recipe (`GET /api/quarry/token` with cookie jar on :3011, service token as in WS7.I); WS7.D evidence points to it. Port and route verified. WS2.A part outside. |
| K2-14 | applied | WS7.C `ScrapeRequest` location before and after WS5.D. |
| K2-15 | out-of-range-skipped | WS1.A/B files. |
| K2-16 | applied | WS6.E and WS7.K broker test command is `cd .../browser-broker && GOWORK=off go test ./...`. |
| K2-17 | out-of-range-skipped | WS1.A effort (only noted in the critical-path sentence). |

## Skeptic K3

| id | status | what was done |
|---|---|---|
| K3-01 | applied-modified | In-range `src/tests/` paths fixed under K1-20. New finding of the same class, not in any review: WS8.B ran `cargo test -p quarry-browser --test browser_egress_boundary`, but that target is `quarry-runtime/tests/browser_egress_boundary.rs`; command split into the two crates. |
| K3-02 | out-of-range-skipped | No `src/src` path in my range (grep clean). |
| K3-03 | out-of-range-skipped | Orchestrator/control main.go paths are in WS15. My range already uses `cmd/orchestrator/main.go` and `cmd/control/main.go`. |
| K3-04 | out-of-range-skipped | WS10.S7. |
| K3-05 | out-of-range-skipped | WS15.L5. |
| K3-06 | applied-modified | Location is WS14/WS15. WS6.A, WS6.H and WS6.K Go test commands now say CI runs them only in the report-only `go-coverage` job until WS14.S0b (named as in K3-06) adds the hard `go-test` job. |
| K3-07 | out-of-range-skipped | notification-core migrations. |
| K3-08 | applied | WS6.D `AppState.limits`: four quarry-edge literals (`main.rs:1426`, `test_support.rs:96`, `tests/support/mod.rs:41`, `routes.rs:1301`). WS7.C changes a field's type, not the field list, so no edit; WS14/WS16 rows outside. |
| K3-09 | applied-modified | WS10/WS16 outside. Extended to WS7 (same defect, same workflow): rows in WS7.B and WS7.I adding `contract-goldens.yml` path filters for the web-policy catalogue, `quarry-token-claims.golden.*` and `auth-core-org-core-web-policy.contract.json*`; verified that none is named in the workflow today. |
| K3-10 | applied | WS8.E single R-31 queue line verbatim, WS8 header and WS8.E slice-table dependency aligned. |
| K3-11 | applied | WS6.A (see K1-03) plus state-of-code evidence (edge forwards `Option<u32>`, orchestrator reads 0 as unlimited, 400 exists only at the gateway). WS14.S6/S8 lines outside. |
| K3-12 | out-of-range-skipped | WS16.S3. |
| K3-13 | out-of-range-skipped | WS16.S6. |
| K3-14 | applied | WS9.E: owner of the classification PR is WS11.S0, WS9.E consumes it; moderation.rs row and test name recorded. The WS11.S0 row itself is outside. |
| K3-15 | out-of-range-skipped | WS10.S2. |
| K3-16 | out-of-range-skipped | Epic migrations. WS7 outcome B `0016` kept and tagged [NEW]. |
| K3-17 to K3-20 | out-of-range-skipped | WS14, WS15, WS11. |
| K3-21 | out-of-range-skipped | E3.3 dependency; WS9.J text here is already consistent ("built only if ADR-3 is refused or late"). |
| K3-22 | out-of-range-skipped | Effort risk bands of WS15/16/17. |

## Path and citation check C3 (plan lines 947-1571)

All rows applied; each corrected path re-run through provenance.sh and tagged by its result.

| plan line | status | note |
|---|---|---|
| 962 | applied | `dispatcher.go:347-348` [C]. |
| 1014 (contracts.rs) | applied | `crates/quarry-core/tests/contracts.rs:36-60` [C]; `wire_name` marked [NEW] (WS2, R-10c). |
| 1014 (grpc.rs 2700, tools.rs 1184) | applied | Rewritten as typed-error match arms (see K2-08). |
| 1014 (fallback_driver) | applied | `:575` fn, 587 arm. |
| 1046 | applied-modified | The named test does not exist; row says new test [NEW], file [C], cites `usage.go:23`. |
| 1066 | applied | `crates/quarry-edge/tests/support/mod.rs` [M]. |
| 1071 | applied | `apps/Ingestion Plane/Quarry-v2/docs/BILLING.md` [C]. |
| 1089 | applied-modified | The derive call does not exist; row names the `resolve_browser_grant` call at 337 (fn 138-202) as the insertion point. |
| 1110 | applied | `browser_driver.rs` 140 and 173 (plus the `org_id: String::new()` seed at 163, checked). |
| 1128 | applied | `listJobs` 347-387. |
| 1139 | applied | `ZdrMarkLayer` described as planned, not in the live tree. |
| 1183 | applied | `engine.go:97-154`. |
| 1213 | applied | `contracts.rs:640-827` [C]. |
| 1244 | applied | `build_search_options` declared 501, expression 529 (verified in source). |
| 1279 | applied | `authRequired` map declared at 196 (two occurrences in WS7.H and WS7.I). |
| 1355 | applied | WS2 split modules tagged [NEW]; declarations 1273, 2228, 3086 added; also the `step` / `execute_ws_action` site in WS7.D. |
| 1343 | applied | `domains/browser.rs` 663-759 [C]. |
| 1398 | applied | `tests/support/mod.rs` [M]; `configure_egress_policy` declared 1893, `release` 1944. |
| 1416 | applied | `live.rs` [NEW] (WS2); `event_type_token` in `nats_event_bus.rs` 364 (arm 394); `is_user_facing_event` and `event_name` noted at `agent_routes.rs` 949 and 965 today. |
| 1428 | applied | `run_browser_agent_loop` 621-786. |
| 1443 | applied | Full paths for `domains/browser.rs`, `BrowserChrome.tsx`, `browser-client.ts`, `action-registry.ts` (all [C]). |
| 1462 | applied | `domains/browser.rs` [C]. |
| 1512 | applied | `apps/Frontend Plane/verevonv3/scripts/probe-product-demo.mjs` [U]. |
| 1537 | applied | `BrowserMcpRequest::parse` 423-470, `execute_quarry_browser_mcp` 237-295. |
| 1156, 1237, 1286, 1526 | applied | Untagged occurrences now carry [NEW] with their owning slice. |
| 1163, 1299, 1393 | applied | [NEW] (L3), [NEW] (outcome B only), [NEW per WS1]. |

C2 row 943 and C4 rows 1587/1589 are outside 947-1571.

## New inconsistencies noticed but not fixed (for the coordinator)

1. WS1.A effort: K1-16 says 16 d, K2-17 says 15 d (WS1.A0 1.5 d plus 13.5 d). The WS8 critical-path sum (50 d) and the Wave 1 totals (113.75 core) must be recomputed by whoever owns section 2.1 and 4.4; WS6.A alone adds 2 d to Wave 1.
2. WS8.F and WS8.I evidence now depend on a slice `WS1.A0` (K2-01). If the segment-2 fixer names it differently, update both mentions.
3. The R-31 queue (K3-10) places WS3.K and WS3.G before WS8.E, which can delay WS8.E; I softened the slice-table dependency ("only if built"). The section 4 owner should confirm.
4. WS8.D/WS8.I and WS6.F mention `AppState::test_default()` conditionally on WS1.B introducing it (K2-03 preferred option); WS1.B's fixer decides.
5. WS14.S0b is referenced from WS6.A, WS6.H, WS6.K as K3-06 named it; the slice must exist in WS14.
6. The WS6.A gateway row cites "matrix row 12" of section 4.1 (K1-08); a section 4 renumber would break that pointer.
7. Prose still uses the bare name `browser.rs` for the gateway file in about seven lines (C3 notes about 40 abbreviated names across the range, not path-checked); left as is.
8. `events.proto` has no field for the pause `reason` that K1-14 asks for; the payload shape for `reason` is not yet specified in WS8.G or in sibling (b)'s scope.
9. WS8.F evidence step (10): the expected result after a broker restart depends on whether WS1.C persists derived attach grants; written as "record which".

<!-- COMPLETE -->
