# Fixer F5 log: segment 5 (plan lines 2277-2924, file `.plan-work/fix/seg5.md`, now 652 lines)

Segment contains WS14 (whole), WS15, WS16, WS17. Effort change: WS14 36.5 d to 37 d (new slice WS14.S0b 0.5 d), header and slice table updated and re-summed (37). Other totals in the plan (section 4.4, wave 2 sums) are outside this segment and need the +0.5 d.

## Skeptic findings

| id | status | note |
|---|---|---|
| K1-01 to K1-23 | out-of-range-skipped | none names a location in 2277-2924 (K1-15 names 2201 and 2271, below the range) |
| K2-01 to K2-17 | out-of-range-skipped | all in 417-1571. K2-03 (gateway `AppState` literals) is the same defect as WS15.S0's new `AppState.schedule_capabilities`: applied the five-test-literal sentence there as an extra (unlisted location), see inconsistencies |
| K3-01 | applied | only in-range location is 2816 (WS17.S0): path now `crates/quarry-browser/tests/profile_cdp_spike.rs`, command `cargo test -p quarry-browser --features chromiumoxide --test profile_cdp_spike -- --ignored`; other K3-01 locations out of range |
| K3-02 | out-of-range-skipped | 1767 and 2242 are below 2277 |
| K3-03 | applied | orchestrator and control `main.go` paths corrected in 7 lines (2466, 2529, 2582, 2602, 2605, 2626, 2913), every occurrence by global replace; provenance C for both |
| K3-04 | out-of-range-skipped | 1751 |
| K3-05 | applied | WS15.L5: `ingestions.run_schedule_action` split into `ingestions.pause_schedule` (no approval) and `ingestions.change_schedule_state` (approval for unpause, trigger, backfill, delete; delete `reversible:false`), alternative per-action approval in the dispatcher, test `model_surface_unpause_requires_approval`; L5 DoD, S5 go-live gate and check (3) reworded to "approved enable". Verified registry text at `action-registry.ts` 2585-2594 and dispatcher at `dispatchers.rs` 3996. L5 effort not re-estimated (reviewer gave no number) |
| K3-06 | applied | new slice WS14.S0b (0.5 d): hard `go-test` job in `.github/workflows/quarry-v2-ci.yml` (matrix quarry-control, quarry-orchestrator, pkg/quarrycontracts, no `continue-on-error`, no coverage threshold); WS14.S0 test text and WS14.S3 DoD ("Go coverage floor holds" removed) rewritten; WS15 intro, Depends and S3 Depends cite it; added [UNVERIFIED] that today's Go tests all pass (CI only ran them report-only) |
| K3-07 | applied-modified | the reviewer's fix text keeps WS15.S4 at `015_register_quarry_schedule_notifications` and moves E3.2 (outside range) to `016_register_approval_requested`; applied exactly that: S4 keeps 015 and carries the R-32 allocation sentence. The R-32 row itself (table at 337-341) and the E3.2 change are other segments' work |
| K3-08 | applied | WS14.S4 and WS16.S6 now name all four full `AppState` literals (`main.rs:1426`, `test_support.rs:96`, `tests/support/mod.rs:41`, `routes.rs:1301`), file columns extended, "16 literals" claim removed. Verified by grep |
| K3-09 | applied | WS16.S1 gets a `.github/workflows/contract-goldens.yml` [C] row (both `paths:` lists get `apps/**/model-gateway-service-routes.json*` and `apps/**/model-gateway-contract/**`); WS16.S3 vendored copy renamed to `model-gateway-service-routes.json`. WS10.S2 half is another segment |
| K3-10 | applied | WS17 reconciliation line now repeats the single `chromiumoxide.rs` queue of R-31 verbatim; WS12 and WS13 copies are other segments |
| K3-11 | applied | R-19 line, WS14.S6 request table and WS14.S8 gate: `max_pages: 0` takes the WS6.A default at W1, 400 only under `CRAWL_REQUEST_VALIDATION=strict`, gateway keeps its 400; test `crawl_request_zero_pages_is_default_in_lenient_and_400_in_strict` added |
| K3-12 | applied | WS16.S3 request type `structured_output_schema: Option<String>` built with `serde_json::to_string`, fixture `request.ok.json` string-typed, fake test `object_typed_schema_is_400`; WS16.S4 says "JSON string, never an object" |
| K3-13 | applied | `QUARRY_EDGE__AI_FORMATS_ZDR` default stays `refuse` until an ADR-4 amendment; four places reworded (Gated by, flag text, rollout sentence, ZDR check) |
| K3-14, K3-15 | out-of-range-skipped | 1836, 1934, 1536 and 1641 |
| K3-16 | out-of-range-skipped | locations 2984, 3007, 3058, 3114, 3156 are epics. Checked the segment is consistent with the proposed allocation (WS15 control 016 to 018, WS17 runtime 0015 to 0017, org-core 024; E3.5 and E3.7 take 021 and 022, so no collision with this segment) |
| K3-17 | applied-modified | chose the root stack: overlay moved to `apps/Ingestion Plane/docker-compose.crawl-fixture.yml` [NEW] mounting `./Quarry-v2/deploy/compose/crawl-fixture/a` and `/b`; validation, CI grep guard and evidence commands rewritten; the mount and network resolution on the root stack is marked [UNVERIFIED, not run] and made the first step of the slice |
| K3-18 | applied (WS17.E1 only) | restore p95 under 1.5x pre-slice p95, at least 20 restores; the "7-origin fixture" wording was not used because that fixture exists only with WS12/C1. WS10 and WS12 criteria are other segments |
| K3-19 | applied | WS15.S3 evidence 6 uses `grep -rn '"go.temporal.io' --include=*.go services/quarry-control` and the `client.go` row adds removing the `cycle23.go:276` comment. Note: with the quoted pattern the `cycle23.go` comment (backticks, not quotes) would not match anyway; the removal is still listed as the reviewer asked |
| K3-20, K3-21 | out-of-range-skipped | 1852 and 2960 |
| K3-22 | applied-modified | per the reviewer's option of a stated band: WS15.L2 (7 to 12 d), WS16.S6 (8 to 14 d), WS17.E2 (8 to 14 d) carry a band note in the slice table; headline effort numbers and totals unchanged. The 4.4 risk-band statement is outside this segment |

## Path and citation checker rows (paths-C5.md, by plan line; paths-C4.md tail)

| id | status | note |
|---|---|---|
| C4 tail (2277-2278) | applied (nothing) | no C4 failure falls on 2277-2278; C4's 2242 and 2251 rows are below range |
| C5-2466, 2529, 2582, 2602, 2605, 2626, 2913 | applied | `cmd/...main.go` paths (same as K3-03) |
| C5-2617, 2623 | applied | `schedule_preflight.rs` and `schedule_runs.go` tagged [NEW from S2] in the S4 table |
| C5-2742 | applied | `quarry-runtime/src/lib.rs` tagged [M] |
| C5-2542 (two rows) | applied | `jobs_lifecycle.go` and `job_lifecycle_routes.rs` tagged [NEW from L1] in L4 |
| C5-2781 | applied | full `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` [M] |
| C5-2912 | applied | `profile_routes.rs` alternative tagged [C] (edit in place); `internal_routes.rs` stays [NEW] |
| C5-2512 | applied | tag list `[M], [C], [C]` |
| C5-2608, 2630 | applied | full `VerevonIngestionsPage.tsx` path (also at the WS15.S3 and S4 rows) |
| C5-2710 | applied | full `cross-plane-token-requests.json` path; bare manifest and `zdr-contract-sync.test.sh` names in the same row also made full |
| C5-2474 | applied | `apps/Ingestion Plane/Quarry-v2/docs/ERROR_REASONS.md` [NEW, owner WS2] (provenance MISSING, consistent with NEW) |
| C5-2758 | applied | `apps/Model Plane/proto/baseline.binpb` [C] |
| C5-2281, 2293 | applied | `limits.go` and `limits.rs` tagged [NEW from WS6.A] (provenance MISSING) |
| C5-2321 | applied | file [C], test [NEW from S0] |
| C5-2349 | applied-modified | the checker says line 42 adds `pub mod crawl_signals;`; the live diff shows `pub mod crawl_signals;` is already committed and line 42 adds `pub mod data_plane_http;`, 160-163 rewrite the rerank re-export. Used the diff, not the checker. WS17's in-flight note `(44, 159-164)` corrected to `(42, 160-163)` |
| C5-2291 | applied | openapi hunks 238, 243-250, 4012-4031, 4052-4057, 4102-4143 (+77 and one changed line) |
| C5-2375, 2538 (x2), 2540 (x2), 2567, 2648, 2651, 2777, 2810 (x4), 2826 | applied | line drifts corrected as proposed (713-760; 984-1005; 766-1000; 27; 616-643; 331; 238; 374-639 with structured tests 515, 565, 595; 409-411; asserts 111, 203-204, 213, 384, 418; 586). 2648 also tagged `normalize.rs` [M] (provenance M) |
| C5-2738 | applied | 38-58 relabelled as the `ExtractRequest` struct, handler starts at 184 |
| C5-2846 | applied | status line is line 3; 134 and 289 hold none |
| C5-2886 | applied | `agent_routes.rs` `browser_session` handler at 1669 |
| C5-2649 | applied-modified | verified in code: `http_routes.rs` 8946 is `pii_redaction_required` only; `injection_defense` screening is `moderation.rs` `scan_injection` 183, `screen_tool_payload` 437, `injection_defense_enabled` 493, and `http_routes.rs` has no injection reference; which `invoke` path calls it stays [UNVERIFIED] |
| C5-2740 | applied | `main.rs` 246-252 gate decision, `build_model_client` at 957 |
| C5-2779 | applied | `load_profile` cited in `profile_routes.rs` 268-299, `routes.rs:144` only mounts it |

## New inconsistencies noticed, not fixed

- WS17 R-1 line still says the runner's gap tolerance is [UNVERIFIED]; K1-19 and K2-10 (other segments) verified it (sqlx, quarry-control by name, broker entrypoint glob re-applies all files, so statements must stay idempotent). Whoever owns the R-1 ledger fix should drop the hedge here too.
- WS16.S8 gateway.proto row says to regenerate Go and TypeScript gen trees; K2-04 (other segments) says plain regeneration rewrites Python and TS trees and prescribes `buf.gen.go-only.yaml` with the other trees left stale on purpose. S8 and WS6.E, WS7.K, WS8.C should use one wording.
- WS15.L5 is now larger by the schedule-action split (registry, two handler arms, dispatcher, one test); its 4 d was not changed. Section 4.4 and the WS15 header total (45.5 d) may need a half day if the owner agrees.
- WS14.S0b adds 0.5 d to WS14 (37 d): section 4.4, the wave 2 total and any `W2` core sum outside this segment still use 36.5.
- K3-07: R-32 (notification-core allocation row) and E3.2's `016` must be written by the fixers of the R-table (segment 1) and E3.2 (segment 6); this segment already refers to R-32 by name.
- The WS14 S1 overlay move (root stack) means WS14 S4, S5 and S6 evidence lines that say "overlay up" now implicitly mean the root-stack overlay; they were left generic.

<!-- COMPLETE -->
