# Fixer F4 log: segment 4 (plan lines 1572-2276: WS10, WS11, WS12, WS13)

Segment file edited: `.plan-work/fix/seg4.md` (705 lines before, 708 after: three new table rows). Reviewer ids were stripped from the plan prose after editing (they live here only). Line numbers below are ABSOLUTE plan lines (segment line + 1571) as quoted by the findings.

## Skeptic findings

| id | status | what was done |
|---|---|---|
| K1-01 | out-of-range-skipped | locations are 79, 580, 588, 691, 3427, 3432, 3471; the R-26 invariant sentence at 2135 (`MAX_SESSIONS` must exceed `MAX_CONCURRENT_RUNS` plus the heavy lane) is still true under the fix and was left unchanged |
| K1-02 to K1-11 | out-of-range-skipped | no location in 1572-2276 |
| K1-12 | out-of-range-skipped | WS4.H, line 824 |
| K1-13 | applied-modified | WS4.K itself is outside; in WS13.4b (2202) added that the page-cache org key segment is WS4.K's one-line fix and that capture and actions requests never touch that cache, so WS13 does not wait for it. Needs WS4.K to exist in segment 2 |
| K1-14 | applied | lines 1589 (R-11 text), 1645 (WS10.S2 allocation) and 1793 (cross-session): takeover pause and resume reuse 142/143 with a `reason`, only `BrowserAttachSharedJar` takes 160, sibling (b) starts at 161 and fields 21 |
| K1-15 | applied | WS13.5 (plan 2201 row `scrape_actions.rs` and 2271): new row for the tighten-only `initiated_by` field on `/v1/scrape` (403 `scrape_interact_scope_required` for `initiated_by:"agent"` with actions, whatever the token kind), gateway sets it on Model-originated requests, test `agent_labelled_user_token_cannot_send_actions` in the edge tests and the evidence list, gateway test `model_originated_scrape_carries_initiated_by_agent`, flag text and WS13 "Gated by" line say `QUARRY_EDGE__SCRAPE_ACTIONS=user` is dev-only until that test exists, "closed by construction" sentence rewritten. X-20 itself (line 313) is outside and needs the matching sentence |
| K1-16, K1-17, K1-18 | out-of-range-skipped | no location in range |
| K1-19 | applied-modified | only the WS12 R-1 clause (1967) was in range: replaced the [UNVERIFIED] gap-tolerance note by "gaps tolerated (sqlx::migrate! applies any unapplied version in order); statements idempotent". The other occurrences are outside |
| K1-20 | applied | covered by K3-01 for every `src/tests/` path in WS10/WS12 (the K1-20 list itself names lines outside the range) |
| K1-21, K1-22, K1-23 | out-of-range-skipped | no location in range |
| K2-01 to K2-17 | out-of-range-skipped | K2 covers 417-1571. Two same-fact occurrences in range were aligned: the user-token mint recipe (K2-13) in WS13 (2152 and 2264) and the `buf generate` command of WS10.S2 (K2-04, limited with `--path`, added note) |
| K2-13 | applied-modified | WS13 occurrences (2152 common test environment, 2264 e2e script) now carry the `GET /api/quarry/token` and `POST /api/quarry/internal-token` recipe (route verified in `plane-token.controller.ts`) instead of [UNVERIFIED] |
| K2-04 | applied-modified | WS10.S2 generated-files row: `buf generate --path model_plane/v1/events.proto` plus a note on `buf.gen.go-only.yaml`; WS10 regenerates Python and TS on purpose, so no "stay stale" wording |
| K3-01 | applied | every `src/tests/` path moved to `crates/<crate>/tests/`: `request_id.rs`, `agent_events_replay.rs`, `origin_is_not_authz.rs`, `agent_trace_events.rs`, `zdr_span_export.rs`, `redaction_wire.rs`, `log_hygiene.rs` plus `log_hygiene.allow`, `ax-password-probe.json`; `origin_is_not_authz` and the new doc-comment test read sources with `include_str!("../src/...")`; commands use `--test <file>`; the `zdr_span_export` command was split into `--lib otel_zdr` and `--test zdr_span_export` (a name filter does not select an integration target); rule sentence added at the top of WS10. WS13 already used `crates/quarry-edge/tests/scrape_capture.rs` correctly. WS17 (line 2816) outside |
| K3-02 | applied | `quarry-edge/Cargo.toml` [C] (WS10.S8 row 8b) and `apps/gateway/src/cache_admission.rs` [NEW] (WS13.7) |
| K3-03 | out-of-range-skipped | lines 2466 and later |
| K3-04 | applied | WS10.S7 `agentRuns.ts` row now owner-only (`externalUserId == run.externalUserId`), DoD, tests and evidence changed, new test `non_owner_org_member_sees_no_browser_activity` in `trace-projection.test.cjs`; consistent with ADR-8 part 2, matrix rule 10 and the existing "S7 only narrows access" sentence |
| K3-05, K3-06, K3-07, K3-08 | out-of-range-skipped | locations 2324 and later |
| K3-09 | applied | WS10.S2: new row `.github/workflows/contract-goldens.yml` [C] with `"apps/**/run-trace-envelope.golden.json*"` in both `paths:` lists; vendored copies keep the canonical name. WS16.S1 half is outside |
| K3-10 | applied-modified | one queue sentence written in WS12 (R-31) and WS13 (R-31): WS3.C, WS3.D, WS3.F, WS3.K, WS3.G, WS8.E, WS12.B (= WS12.S1), WS13.C (= WS13 slice 3), WS12.F (= WS12.S5), WS17.E (C1), WS17.G (C2). Modified only by adding that WS13.G (slice 6, stitch) is not named by R-31 and rebases onto slice 3 before WS12.F. R-31 (367) and WS17 (2784) are outside and must carry the same sentence |
| K3-11, K3-12, K3-13 | out-of-range-skipped | lines 2293 and later |
| K3-14 | applied | WS11.S0: new row for `model-gateway/src/moderation.rs` `TrustClass::classify` 228-240 and test `trust_class_browser_mcp_tools_are_browser_scraped`; state-of-code F12 bullet and WS11.S7 mention the other copies; ownership stated: WS11.S0 owns the classification change, WS9.E (line 1542, outside) must say it consumes it |
| K3-15 | applied | WS10.S2 compile-fix row now names `http_routes.rs` 11047 and 11100, `session-core/src/grpc.rs` `replay_event_row_to_proto`, `mp-contracts/tests/proto_wire_parity.rs`; `sse.rs` and `sse_events.rs` removed; the risk line "four [M] files" became "two [M] files" |
| K3-16, K3-17 | out-of-range-skipped | lines 2338 and later |
| K3-18 | applied | in-range halves: WS10.S3 DoD (test `events_stream_doc_comment_names_quarry_event_history_absent`) and WS12.S5 DoD (`mask_ms` p95 under 150 ms for 1280x800, or the number is stated and accepted). WS17.E1 (2849) outside |
| K3-19 | out-of-range-skipped | 2611 |
| K3-20 | applied | WS11.S1: defaults stay 20 and 120 s, ceilings 100 and 900 s; test `runtime_default_is_120`; risks wording changed from "240 s ceiling" to effective run length about 240 s |
| K3-21, K3-22 | out-of-range-skipped | 2960 and WS14 to WS17 efforts (K3-22 touches WS15 to WS17 only; WS10 to WS13 efforts unchanged) |

## Path and citation checks (paths-C4; C3 and C5 have no rows in 1572-2276)

Provenance re-run for every corrected path: `apps/Ingestion Plane/Quarry-v2/docs/gap-quarry.md` C; `.../docs/SECRET_REDACTION.md` MISSING so [NEW]; `apps/Model Plane/scripts/compose.sh` C; `.../quarry-edge/Cargo.toml` C; `apps/gateway/src/cache_admission.rs` MISSING so [NEW]; `.github/workflows/contract-goldens.yml` C; `.../model-gateway/src/moderation.rs` C; `apps/Ingestion Plane/autocomplete-core/src/routes.rs` C; `.../quarry-browser/tests` C and `.../quarry-runtime/tests` C (directories); `apps/Ingestion Plane/Quarry-v2/Cargo.lock` `?` (exists, not in afb83928, git-ignored).

| plan line | status | what was done |
|---|---|---|
| 1589 gap-quarry.md:347 | applied | full path, [C]; line 347 contains "canonical event envelope" (re-read) |
| 1696 trace_config.rs | applied | tag [NEW] (created in S3, extended in S5) |
| 1703 trace_nats.rs | applied | tag [NEW, created in S6]; also noted the provisioner needle test therefore lands with or after S6 |
| 1767 quarry-edge Cargo.toml | applied | `crates/quarry-edge/Cargo.toml` [C] (same as K3-02) |
| 1967 SECRET_REDACTION.md | applied | full path [NEW] |
| 2073 scripts/compose.sh | applied | `apps/Model Plane/scripts/compose.sh` [C] |
| 2242 cache_admission.rs | applied | `apps/gateway/src/cache_admission.rs` [NEW] |
| 2081 Cargo.lock | applied | marked untracked/git-ignored local file; absence of `png` and `image` re-confirmed by K3 and by C4; re-grep after dependency changes |
| 2251 gateway Cargo.toml | applied | command now `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; also quoted the unquoted `grep` path in the DoD at 2252 |
| 1769 elided quarry-control path | applied | `.../services/quarry-control/internal` [C] |
| 1587 Ingestion Plane/docker-compose.yml | applied | `apps/` prefix; also `.github/workflows/model-plane-rust-ci.yml` in the same bullet |
| 1625, 1687, 1706, 1779 TAG-GAP | applied | [NEW] added (request_id.rs unit tests, trace.rs, trace_projection.rs twice, `model-trace-nats-smoke.sh` command) |
| 2009 ax-password-probe.json | applied | path moved to `tests/fixtures`, [NEW, generated] |
| 2056 mp-scrub | applied | [NEW] |
| 2077 sensitive_rects.rs, screenshot_mask.rs | applied | [NEW] |
| 2133 quarry-core scrape.rs | applied | [NEW, created by WS5 slice 4 (WS5.D)]; not a wrong path |
| 2162 take_lease | applied | 202 (re-grepped: `pub fn take_lease` at 202) |
| 1852 browser_execution_from_loop_result | applied | 783 (branch 789-797), re-grepped |
| 2215 persist_captures and builder stubs | applied | `persist_captures` marked new in 4b; `780-799` replaced by the real field and use sites read from `pipeline.rs`: fields 47, 92, 100, 106; uses 795, 846 (built at 255), about 1000-1058 |
| 2031 lib.rs 128-176 (not confirmed list) | applied-modified | text now says `lib.rs` holds only `mod redaction_boot;` and the call is in `main.rs` (lines "about", re-check before editing) |
| other "not confirmed" items (1852 "240", 1933/1921 phase 3, 1837, 1838, 1850, 1855, 1894, 2068, 1808, 2077 672-848, 2191, 2202, 2214, 2242 shared.rs) | not-changed | reviewer marked them not confirmed, not failures; no code evidence to correct them, left as is |

## New inconsistencies noticed, not fixed here (owners of the other segments)

1. WS13.G (slice 6, tile-and-stitch) edits `chromiumoxide.rs` `capture_screenshot` but R-31's single queue (as worded by K3-10) does not name it; the segment now says it rebases onto WS13.C and merges before WS12.F. The R-31 owner (line 367) should add WS13.G.
2. R-31 (367) and WS17 (2784) must be rewritten to the same queue sentence; WS9.E (1542) must say the classification PR belongs to WS11.S0; the R-11 ledger rows (347, 3314) and WS8 text must adopt 142/143 reuse and "only 160 for WS8".
3. WS10.S5's provisioner needle test references `session-core/src/trace_nats.rs`, a file created in S6 (S5 precedes S6): the needle test cannot pass at S5. Either the needle row moves to S6 or S5 ships a stub; noted in the row, not re-sequenced.
4. WS13.4b depends on WS4.K existing in segment 2 (K1-13) only as a reference; if K1-13 is rejected elsewhere, delete that clause.
5. X-20 (line 313) needs the sentence that `QUARRY_EDGE__SCRAPE_ACTIONS=user` is dev-only until `agent_labelled_user_token_cannot_send_actions` exists, and WS13.F's `initiated_by` field should be listed in the contract section of 1.x if one exists.
6. WS10 S8 (`zdr_span_export`) and WS12 S2 (`redaction_wire`) integration tests need `tests/support/mod.rs` `base_state` [M], which is an in-flight file: PRE-1 baseline applies.
7. K3-08 (four `AppState` literals) concerns WS14/WS16 only; none of WS10 to WS13 adds an `AppState` field (WS13 states this explicitly), so no change here.

<!-- COMPLETE -->
