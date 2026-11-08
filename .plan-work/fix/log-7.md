# Fixer F7 log: segment 7 (plan lines 3234-3714, `fix/seg7.md`)

Method: three scripted passes of exact-substring replacements (each old string had to match exactly once, or a stated count, or nothing was saved; all 148 edits matched), original kept in the scratchpad (`seg7.orig.md`). Table column counts re-checked after the edits (0 bad rows). Provenance re-run for every new or corrected path: `pg.go`, `resources.go`, `internal/store/store.go`, `tests/cache_policy.rs`, `moderation.rs`, `quarry_agent.rs`, `cache.rs`, `buf.gen.go-only.yaml`, `apps/Control Plane/scripts/run-control-plane.sh`, `quarry-core/tests/contracts.rs`, `quarry-browser/tests/ssrf.rs`, `sdks/typescript/docs/EvaluateAction.md`, `docker-compose.production.yml`, `quarry-v2-ci.yml`, `contract-goldens.yml`, `quarry-runtime/migrations`, `quarry-control/internal/store/pg/migrations`, `observability.rs` are [C]; `quarry-edge/tests/support/mod.rs`, `routes.rs`, `main.rs`, `lib.rs`, `model-gateway/src/quarry.rs`, `tool_loop.rs` are [M]; `quarry-edge/tests/rerank_behavior.rs`, `quarry-runtime/tests/dp_retrieve_contract.rs`, `dp_rerank_contract.rs` are [U]; `zdr_matrix.rs`, `zdr_log.rs`, `scripts/tests/browser-grant-prod-contract.sh`, `src/features/browser/`, `quarry-edge/tests/zdr_feature_matrix.rs` are MISSING and now tagged [NEW]. Cross-segment facts were aligned with the logs and segments of F1 to F4 (W1 148.25 d, WS1.A0/A1/A2, WS4.K, WS5.C gated, 166 slices, SC2a/SC2b, 40/32/1 session and run pair).

## Skeptic K1

| id | status | note |
|---|---|---|
| K1-01 | applied-modified | One pair everywhere in my range: 40 sessions, 32 runs, heavy lane 1 (flag table rows for `MAX_SESSIONS` and `MAX_CONCURRENT_RUNS`, 4.5 capacity rule now WARN-only until RSS is recorded then a refusal to boot, SC3 row of 6.2 with 48 sessions in the RSS test env, W1 exit evidence 3 states the 13th-session test sets 12 in its test env file). The SC3 definition (line 79) is outside. |
| K1-02 | applied | W1 exit gate, 6.2 SC1 row and L2 sentence, 6.4 A (contract overlay may only set env and volumes; WS3.A proof recorded on the same stack; L1 assertion 6 reworded). SC1 definition outside. |
| K1-03 | applied-modified | 6.2 SC7 row rewritten (W1 clause: 0 or absent takes the default, `clamped_by`); PR 2 gets a Semantics sentence (0 is absent at the edge, 400 only under `CRAWL_REQUEST_VALIDATION=strict`, WS14.G/I). SC7 definition (83), X-10 and WS6.A text are outside. |
| K1-04 | applied | 6.2 SC10 row now excludes the owner's frame stream and names the PROBE. SC10 definition outside. |
| K1-05 | applied | 6.2 SC2 split into SC2a and SC2b; W1 exit evidence 2 names flag value `1` on the evidence stack, default stays `0`; W2 evidence 2 adds the pause and the flag flip; flag table row for `TAKEOVER_ENFORCEMENT` updated. |
| K1-06 | applied | First alternative (as F2): WS5.C gated and in the W1 exit gate, 4.4 gated 16 and optional 6, L5 row (28 d, opt 0), position 10, lane row L5, `DOC_ROUTING` flag row, SC6 row and exit evidence 5 (`DOC_ROUTING=on` on the evidence stack). |
| K1-07 | applied-modified | "disjoint" replaced by "hunk-disjoint" in 4.1, 4.3 position 1 and 7.2; new per-file merge-order table in 4.1 (compose: WS3.A, WS5.A, WS6.A, WS1.A0, WS1.A2 (I added WS6.A and the WS1.A split to the reviewer's WS3.A, WS5.A, WS1.A order; matches seg1's header note); openapi: WS2.A, WS6.A, WS5.A; `quarry.rs`: WS4.A, WS6.A; quarry-edge `routes/main/lib/config`: per region, PRE-2 first on `main.rs`); 4.2 row 10 order fixed; position 0 paragraph and position 1 reason say which slices need position 0 (WS3.A, WS5.A, WS5.B, WS6.A, WS1.A0, WS1.A2, PRE-2). X-3 text (212-216) and at-a-glance 7 are outside (F1 did them). |
| K1-08 | applied | 7.2 intro rewritten (hunk-disjoint; PR 1 and PR 2 share `knowledge/quarry.rs`; PR 2 and PR 4 share `openapi.yaml`; only PR 1 mergeable before position 0; PR 2 needs position 0, also stated in its branch bullet). |
| K1-09 | applied | PRE-2 is PR 3 (new full block with files, defects, proposed tests, evidence), WS2.A is PR 4 (block kept in full), next candidates renumbered (PR 5 WS3.A, PR 6 WS15.C, PR 7 WS1.A1); PRE-2 added to the W1 exit gate, 4.3 position 1 and 4.2 row 6; 7.3 sequence updated. |
| K1-10 | applied | PR 4 (ex PR 3) text and the critical-path paragraph say WS2.A is the entry condition of WS2.B, which the WS1.A2 Quarry validator stream waits for. |
| K1-11 | applied | The WS1.A0/A1/A2 split is carried through 4.1 L1 row, 4.2 rows 1, 6, 15 to 18, the ledger, 4.3 positions 1, 3, 8, 13 and the W1 gate, the critical path (WS1.A1 10 d, off-path note for A2 and A0), the Other-chains line, 4.4, 4.5 flag owners (A1 for broker flags, A2 for the validator URL), 5.1 B1, 6.1 L1, 6.4 A legs and manifest rows, 7.1 row 3, 7.2 candidates and 7.3. Slice table, 2.1 and 2.4 B are outside. |
| K1-12 | applied-modified | WS4.H text is outside (F2 did it). In range: 6.4 C inversion test list now names wind-down routes (list and delete for schedules, monitors and profiles) and `zdr_org_can_list_and_delete_profiles`. |
| K1-13 | applied | WS4.K in L4 row (slice, `cache.rs` file, day 0), position 1 (PR 1 with PR 1b), 4.4 (0.5 d), per-file table, new PR 1b block in 7.2 (files, merge, evidence), exit evidence 1, SC5 row, 6.3 row 1 (the "only the gateway cache is fixed" claim was false after this finding and is rewritten). SC5 definition outside. |
| K1-14 | applied | Ledger row for `events.proto` (142 and 143 reused with `reason`, 160 for `BrowserAttachSharedJar`, sibling (b) from 161, reserved 14-20 note), 4.2 row 16, 5.1 B8 and B9. R-11 text (347) is outside (F3 and F4 did theirs). |
| K1-15 | applied-modified | Only the in-range echo: flag row `SCRAPE_ACTIONS` says `user` is dev-only until `agent_labelled_user_token_cannot_send_actions` exists. WS13.F and X-20 are outside. |
| K1-16 | applied-modified | 4.4 recomputed with WS6.A 3.5, WS2.C 10, WS1.A 16 (A1 10 + A2 6) plus WS1.A0 1.5, WS3.F 10, WS4.K 0.5, WS5.C gated, WS14.S0b 0.5: W1 core 132.25, gated 16, opt 6, core+gated 148.25, 29.7 eng-weeks, 34 to 44 risk-adjusted, 6.6 to 8.6 weeks at 6.5 engineers; W2 447.5/510.5; W1+W2 579.75/79/25.5/22.5, 658.75, 131.8, 167 to 228; lane table rows L1, L2, L3, L4, L5, L7 recomputed (W1 lane column sums to 148.25, W2 to 510.5). Modified: the reviewer's "about 140 d" is stated as the skeptic's figure; 132.25 is the slice sum (same call as F1 and F2), the difference sits in the risk band. K3-22 band added. |
| K1-17 | out-of-range-skipped | WS4.H, WS6.D and R-13 are outside (F2 did WS4.H). |
| K1-18 | out-of-range-skipped | ADR-1 (283). |
| K1-19 | applied | Ledger notes for quarry-runtime, quarry-control and broker replaced by the verified statements (including "re-applies every file on each start, so idempotent"); item (d) removed from the "Raised by this sequencing pass" list and (e), (f) renumbered; Appendix A updated. |
| K1-20 | applied | `src/tests` paths fixed in 4.2 row 6 and row 9, 6.1 L2, L3, 6.4 C, `src/src` scan path replaced; also `dp_retrieve_contract.rs` and `dp_rerank_contract.rs` given their `crates/quarry-runtime/tests/` paths (C7). |
| K1-21 | applied | 6.4 C: matrix "generated from the Rust `ROWS` table", `Row` gains `log: Canary{test} \| LogUnverified` (as F2's WS4.G), new test names, never-loosen sentence; 6.2 SC5 row. |
| K1-22 | applied | Column renamed "Primary editor of"; rule "other lanes edit these files only in the order of 4.2" added under 4.1. |
| K1-23 | applied | Flag rows for `BROWSER_HUMAN_BROWSE` and `BROWSER_ATTACH` (default `0` in every environment until X-13 and ADR-3), rollout rule 1. |
| Gaps vs the mission | applied (four of seven) | New 6.5 Programme risk register (ten risks, role-level owners, observable triggers, drawing on audit 10.1 lines 1409-1422 and the plan); new open question 13 Owners in 7.1 (recommended default plane leads) with 7.3 updated to thirteen; 4.3 now states which success criteria each exit closes (aligned with the 1.2 table of F1); PRE-2 in the W1 exit gate. Not applied: the "Apify-class" mapping row, baseline measurements for SC3 and SC4, the least-privilege credential table (R-13 and WS6.D are outside my lines). |

## Skeptic K2

| id | status | note |
|---|---|---|
| K2-01 | applied-modified | In range: the 6.4 A overlay description (production overlay plus overlay that sets env and volumes only, WS1.A0 owns the boot keys, boot bails 132-241), the stack paragraph at the end of 6.1 (durable history flag missing from both compose files), a new flag-table row for `QUARRY_EDGE__ARTIFACT_BACKEND` and `QUARRY_EDGE__DURABLE_EVENT_HISTORY`, exit evidence 4. The slice WS1.A0 itself is in seg2. |
| K2-02 | out-of-range-skipped | WS1.A files. |
| K2-03 | applied | 4.2 intro and row 12: six gateway literals with corrected lines (`main.rs:313`, plus `stream_e2e.rs:110`). The per-slice file lists (WS1.B, WS8.D, WS8.I, WS6.F) are outside. |
| K2-04 | applied | 4.2 row 16 and 6.1 L1: Go-only `buf generate --template buf.gen.go-only.yaml`, stale Python and TypeScript trees registered, Rust generated at build. |
| K2-05 | applied | 6.4 C route scan: named files, `#[cfg(test)]` cut, `agent_routes/router.rs`, `ROUTE_BASELINE_UPDATE=1`, two new test names. |
| K2-06, K2-07 | out-of-range-skipped | WS2.A/C/D files; WS6.D/H file tables. |
| K2-08 | applied-modified | 4.2 row 9 (Go mirror: five missing codes plus `USER_TOOK_OVER`, each with a `case`; `UNSUPPORTED_MEDIA` and `BUDGET_EXHAUSTED` follow with WS5.A and WS6.B, as F2 did; `grpc.rs`/`tools.rs` scope and the `UPSTREAM_BLOCKED` arm; agent-path consumer is `quarry_agent.rs`) and 5.1 B6. |
| K2-09 | applied (range part) | `quarry-core/tests/contracts.rs`, `quarry-edge/tests/support/mod.rs`, `browser.rs` line labels; other lines in 1000-1156 are outside. |
| K2-10 | applied (range part) | Ledger and 7.1 notes (see K1-19). |
| K2-11, K2-14, K2-15, K2-16 | out-of-range-skipped | No location in range (my 6.1 L1 command already uses `GOWORK=off`). |
| K2-12 | applied (range part) | W1 gate and 4.4 count WS6.A (3.5 d). The `launch_count()` and WS3 header parts are outside. |
| K2-13 | applied (range part) | PR 4 evidence: `curl ... /api/quarry/token` recipe (service principal via `POST /api/quarry/internal-token`). |
| K2-17 | applied-modified | WS1.A 16 d (K1-16) split 10 + 6, WS1.A0 1.5 d on top (WS1 33.5 d, L1 W1 33.5), as F2. |

## Skeptic K3

| id | status | note |
|---|---|---|
| K3-01 | applied | Rule "new Rust integration tests live in `crates/<crate>/tests/`" in 4.1 (with the reason and the harness path); the in-range test paths corrected. Lines 1625-2816 are outside. |
| K3-02, K3-03 | out-of-range-skipped | No location in range; checked: my lines already use `cmd/orchestrator/main.go`. |
| K3-04, K3-05 | out-of-range-skipped | WS10.S7 and WS15.L5; 5.2 S5 already says owner-only in v1. |
| K3-06 | applied-modified | 6.1 L7 CI cell (the only `go test` job is report-only `go-coverage`, `quarry-v2-ci.yml` 306-309; WS14.S0b adds the hard `go-test` job), WS14.S0b in lane L7 and position 13, 0.5 d in W2 totals (matches seg1 and seg5). |
| K3-07 | applied-modified | Ledger gets a notification-core row (R-32: 015 WS15.S4, 016 E3.2, later 017+). The E3.2 number itself (2951) is outside. |
| K3-08 | applied-modified | 4.1 rule and 4.2 intro name the four quarry-edge literals (`main.rs:1426`, `test_support.rs:96`, `tests/support/mod.rs:41`, `routes.rs:1301`) and the six gateway literals. WS14.S4 and WS16.S6 rows are outside. |
| K3-09 | applied-modified | 6.1 contract paragraph and 6.4 B rule: path filters of `contract-goldens.yml` grow with each vendored golden, canonical file name kept. The WS10.S2 and WS16.S1 file tables are outside. |
| K3-10 | applied | One queue line (WS3.C, WS3.D, WS3.F, WS3.K, WS3.G, WS8.E, WS12.B, WS13.C, WS12.F, WS17.E (C1), WS17.G (C2)) in 4.2 row 2, lane row L3 and position 16; plus "WS13.G rebases onto WS13.C and merges before WS12.F" as F4 did. R-31 (367) and the WS12/13/17 text are outside. |
| K3-11, K3-12, K3-15, K3-17, K3-18, K3-19, K3-20, K3-21 | out-of-range-skipped | All locations are 1572-2960. |
| K3-13 | applied | Flag row `AI_FORMATS_ZDR` stays `refuse` until an ADR-4 amendment. |
| K3-14 | applied | 5.1 B3 (three copies of `classify`, WS11.A owns the classification PR) and lane L8 gets `moderation.rs` [C]. |
| K3-16 | applied | Ledger: session-core 0048, 0049; capability-core `0016`, `0017_procedures`; quarry-control 021 `webhook_org_scope` E3.5 (F-t2-031) and 022 `jobs_idempotency_org` E3.7 (F-t2-040). |
| K3-22 | applied | Risk band for design slices WS15 L2, WS16 S6, WS17 E2 in the 4.4 assumptions. |

## Path checker C7 (by plan line)

| row | status | note |
|---|---|---|
| 3254, 3295 `store.go` | applied-modified | Replaced by `resources.go` [C] plus `internal/store/store.go` [C] (the `Job` struct, which provenance finds on disk); the reviewer's `pg.go` is the store type, not the SELECT or struct file. L7 row and 4.2 row 19. |
| 3282, 3542, 3543, 3285, 3629, 3635, 3634 | applied | `tests/` paths for `support/mod.rs` [M], `rerank_behavior.rs` [U], `ssrf.rs` [C] (five `#[ignore]`, not three), `contracts.rs` [C], `dp_retrieve_contract.rs` [U], `dp_rerank_contract.rs` [U]; the doubled `src/src` scan path rewritten (K2-05); `zdr_feature_matrix.rs` under `tests/` [NEW]. |
| 3283, 3295 migrations | applied | `crates/quarry-runtime/migrations/` [C]; `services/quarry-control/internal/store/pg/migrations/` [C]. |
| 3695 sdks `models` | applied | [C] dropped; deprecation text goes into `sdks/typescript/docs/EvaluateAction.md` [C]; regenerated models are git-ignored local output. |
| 3251, 3481, 3517 | applied | `zdr_matrix.rs` [NEW], `zdr_log.rs` [NEW], `browser-grant-prod-contract.sh` [NEW], `src/features/browser/` [NEW]. |
| 3624 `run-control-plane.sh` | applied | `apps/Control Plane/scripts/run-control-plane.sh` [C] at both mentions (6.2 recipes and 6.4 A). |
| 3288, 3273 AppState | applied | `main.rs:313`, six literals including `stream_e2e.rs:110`. |
| 3287 `start_ai_run` | applied | `build_ai_run_request` 1212 and `start_ai_run` 1263-1382. |
| 3608, 3697 `browser.rs:5001` | applied | `:5000` at both mentions. |
| 3686 `crawl_handoff` | applied | 590-725 with the edited sub-range 590-667; `HandoffAck` 728-738. |
| 3624 boot bails | applied | `main.rs` 132-241. |
| 3635 "85 literal" | applied | Kept 85 per K2-05 for the four route files after cutting `#[cfg(test)]` modules (the checker's 56 counted `routes.rs` only). |
| 3677 `record_result_cache` | applied | Marked a new function [NEW] in `observability.rs` [C]. |
| 16 AppState literals (unverified) | applied | Replaced by the verified four (K3-08). |
| `quarry.rs` 546 and `tool_loop.rs` 425 | applied | "changed lines" with full model-gateway paths [M] (git numstat 534+12 and 370+55). |
| notes (go.work, `.env.generated-secrets`) | no change | The plan's go.work statement is correct; the user memory is stale. |

## New inconsistencies noticed, not fixed

1. WS6.A (`QUARRY_EDGE` `crates/quarry-edge/src/limits.rs` [NEW], `CrawlLimits`) and lane L15's `crates/quarry-edge/src/limits.rs` [NEW] (`OrgLimits`, `LimitsResolver`) name the same path; one of them needs another file name (the segment-3 owner).
2. Segment 6 (epics) may still give E3.2 the notification-core number `015_register_approval_requested`, which now belongs to WS15.S4 in the ledger (E3.2 takes `016`).
3. I placed WS1.A0 in lane L1 and WS14.S0b in lane L7 for the per-lane tables; the slice tables of seg2 and seg5 do not name lanes.
4. Segment 1's compose merge order (WS3.A, WS5.A, WS1.A0, WS1.A2) omits WS6.A; the 4.1 table here includes it after WS5.A.
5. "Wave 1 exit evidence" item 3 relies on WS3.F's DoD setting `MAX_SESSIONS=12` in its test env (F2 said it states so); if that sentence is dropped, the 13th-session assertion contradicts the shipped default of 40.
6. Section 7.2 heading still reads "first three pull requests" while PR 1b and PR 4 are described in full; kept to preserve the heading.
7. The 4.1 L9 row still lists notification-core and billing-core credential rows without the K1-17 least-privilege table (R-13 is outside).

<!-- COMPLETE -->
