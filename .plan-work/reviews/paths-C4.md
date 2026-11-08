# C4 path and citation check: plan lines 1572-2278

Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md` (WS10 to WS14). Code truth: live tree `C:/dev/CoresSystem` (afb83928 plus uncommitted). Provenance: `.plan-work/tools/provenance.sh`. Method: automatic extraction of every product path mention (`apps/`, `.github/`, `scripts/`, `docs/`) with its tag, provenance check of each unique path, and symbol-proximity check of every line citation against the cited live file, followed by manual review of every flagged item.

## Counts

- Plan lines scanned: 707 (1572-2278). Path-bearing lines: 328.
- Path mentions: 706 = 522 full-path mentions + 184 bare basenames (109 resolved to one live path by same-line or unique-basename match, 75 unresolved).
- Unique full product paths checked by provenance: 248. Glob patterns not expanded: 6 (`apps/**/secret-shapes.v1.json*`, `apps/Ingestion Plane/Quarry-v2/sdks/*/docs/BrowserObservation*.md`, `apps/Model Plane/rust/**`, `.../execution-core/tests/fixtures/planner/prompts/*.txt`, `.../planner/replies/*.json`, `.../session-core/migrations/0044_*`).
- Provenance of the 248: C 130, M 28, U 2, ? 1 (Cargo.lock, see the table), MISSING 87.
- MISSING 87: 79 have at least one `[NEW]` occurrence (consistent). 7 have C/M/U-tagged occurrences: 3 real errors (L1696, L1703, L1767), the rest are row-association artifacts or an elided path. 5 have no tag at all: L1589, L1967, L2073 and L2251 are wrong citations, and `apps/Frontend` is an extraction artifact.
- Tag mismatches (tag vs provenance): 13 automatic flags, all reviewed. 3 real (L1696, L1703, L1767). 1 provenance note (L2081). 9 artifacts of multi-path table rows, where the tag belongs to another cell or path.
- `[NEW]` collisions (tagged new, file exists): 10 automatic flags, 0 real.
- Untagged MISSING occurrences: 28 (5 unique paths with no tag at all, plus untagged new files listed below).
- Line citations: 570 extracted (568 numeric, 1 "(N lines)" count, 1 `:N` suffix). Checked against a resolved live file: 481. Not checkable: 35 (path MISSING), 53 (bare basename unresolved), 1 (directory).
  - Automatic result for the 481: OK 216; no identifier to test 164; flagged 101 (DRIFT 21, WRONG-FAR 54, NOT-FOUND 21, OUT-OF-RANGE 5).
  - Manual review of the 101 flagged: DRIFT confirmed 2; WRONG confirmed 1; UNVERIFIED 23 (listed below); the other 75 are false positives (HTTP status codes, seconds, field/event/section numbers, numbers attributed to the wrong file, or a cited line whose content does match the claim).
  - The 216 OK results rely on identifier proximity (may pass on a generic word). The 164 no-identifier citations were not checked individually.
- DRIFT: 2. WRONG: 1.
- Confirmed correct anchors (sample): agent_routes.rs AgentEventsQuery 937, start_run 1273, AgentStarted emit 1646, agent_events_stream 2658, from_seq 2705, close_run 3562. session-core grpc.rs ORDER BY 4024, replay_thread 4952. model-gateway http_routes.rs route 218. quarry-edge routes.rs layer chain 303/304/305 (TraceLayer, body_limit, timeout), TimeoutLayer 81. observation.rs put_artifact_if_allowed 1428, ActionStarted 391, 2140 truncation. pipeline.rs NormalizedOutput 687, screenshot None 702, local_index_writeback submit 846. telemetry.rs is 64 lines (matches 1586). build.rs mp_protos at 37 (plan 36-56).

## Failures

| Plan line | path[:line] | plan tag / claim | actual | proposed correction |
|---|---|---|---|---|
| 1589 | docs/gap-quarry.md:347 | untagged; audit doc cited as `docs/gap-quarry.md:347` | MISSING at repo root. Exists as `apps/Ingestion Plane/Quarry-v2/docs/gap-quarry.md` [C]. Line 347 contains "canonical event envelope" (claim correct). | [WRONG-CITATION] `apps/Ingestion Plane/Quarry-v2/docs/gap-quarry.md:347` [C] |
| 1696 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/trace_config.rs | [C] ("created in S3") | MISSING | [TAG-MISMATCH] tag [NEW] |
| 1703 | apps/Model Plane/rust/services/session-core/src/trace_nats.rs | [C] | MISSING; no such file in session-core/src | [TAG-MISMATCH] tag [NEW] (or name the existing file if one was meant) |
| 1767 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/Cargo.toml | [M]/[C]/[NEW] (row cell; the dev-dep edit) | MISSING. The crate manifest is `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/Cargo.toml` [C] | [WRONG-CITATION] `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/Cargo.toml` [C] |
| 1967 | docs/SECRET_REDACTION.md | untagged ("S0 ships") | MISSING. Abbreviated, no `apps/Ingestion Plane/Quarry-v2/` prefix (plan lines 2034 and 2088 use the full path with [NEW]) | [WRONG-CITATION] `apps/Ingestion Plane/Quarry-v2/docs/SECRET_REDACTION.md` [NEW] |
| 2073 | scripts/compose.sh | untagged (evidence step) | MISSING at repo root. Exists as `apps/Model Plane/scripts/compose.sh` [C] | [WRONG-CITATION] `apps/Model Plane/scripts/compose.sh` [C] |
| 2242 | apps/Frontend Plane/verevonv3/apps/gateway/src/src/cache_admission.rs | [NEW] inline (row tag [C] belongs to quarry.rs) | MISSING. `src/src/` does not exist; `src/` does | [WRONG-CITATION] `apps/Frontend Plane/verevonv3/apps/gateway/src/cache_admission.rs` [NEW] |
| 2081 | apps/Ingestion Plane/Quarry-v2/Cargo.lock | no tag; cited as evidence ("neither png nor image is in Cargo.lock") | Exists on disk only. Git-ignored (`apps/Ingestion Plane/Quarry-v2/.gitignore:4`), not committed at afb83928 | [PROVENANCE] mark as untracked/ignored local file; re-check the claim by grep before relying on it |
| 2251 | apps/gateway/Cargo.toml | untagged; command run from the SPA root | MISSING as a repo-root path. Exists as `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml` [C] | [WRONG-CITATION, low] write the full path, or say "relative to verevonv3" |
| 1769 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/... | [C] | Elided path. The directory exists [C] | [WRONG-CITATION, low] `apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal` [C] |
| 1587 | Ingestion Plane/docker-compose.yml | [M] | Not written in full. File exists as `apps/Ingestion Plane/docker-compose.yml` [M]; tag correct | [FORMAT, low] write the `apps/` prefix |
| 1625 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/request_id.rs | untagged (tagged [NEW] at 1613) | MISSING, new file | [TAG-GAP] add [NEW] |
| 1687 | apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/trace.rs; apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/trace_projection.rs | untagged | MISSING, new files | [TAG-GAP] add [NEW] |
| 1706 | scripts/tests/model-trace-nats-smoke.sh | untagged | MISSING, new file | [TAG-GAP] add [NEW] |
| 1779 | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/trace_projection.rs | untagged | MISSING, new file (see 1687) | [TAG-GAP] add [NEW] |
| 2009 | apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/tests/fixtures/ax-password-probe.json | untagged ("result recorded in") | MISSING, generated by the probe run | [TAG-GAP] add [NEW] (generated fixture) |
| 2056 | apps/Model Plane/rust/crates/mp-scrub | untagged ("the copy moves into") | MISSING, new crate (tagged [NEW] at 1965) | [TAG-GAP] add [NEW] |
| 2077 | apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/sensitive_rects.rs; .../src/screenshot_mask.rs | untagged | MISSING, new files | [TAG-GAP] add [NEW] |
| 2133 | apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/scrape.rs | untagged | MISSING. Confirm whether this is a new file or a wrong path | [TAG-GAP] add [NEW], or correct the path |
| 2162 | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lease_pool.rs:193-196 | `take_lease` 193-196 [C] | `pub fn take_lease` is at 202 (193-196 is unrelated) | [DRIFT +9] `take_lease` 202 |
| 1852 | apps/Model Plane/rust/services/execution-core/src/tool_bridge/mod.rs:789-797 | `browser_execution_from_loop_result` 789-797 | Function header at 783. Lines 789-797 are the `if let Some(resource_limit)` branch inside it | [DRIFT, minor] `browser_execution_from_loop_result` 783 (branch 789-797) |
| 2215 | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/pipeline.rs | "`persist_captures` (4b); builder stubs 780-799, 846" | No `persist_captures` symbol in pipeline.rs or quarry-runtime/src. 780-799 is content-type parsing. 846 (`local_index_writeback().submit`) is correct | [WRONG-SYMBOL] mark `persist_captures` as a planned new function ([NEW]) or correct the name; drop 780-799 unless the builder stubs are located |

## Not confirmed (manual check needed; not counted as failures)

- 1852 "240": the call site is at 221 and 240 is not the claimed symbol.
- 1933 and 1921 "phase 3 ~2051-2075": the phase 3 marker is at 1979. 2051 lies inside that block, so the claim is imprecise but not wrong.
- 1933 "pre-pass 1801-1826".
- 1837 `tool_bridge/mod.rs` 204-220; 1838 `browser_agent_e2e.rs:76`; 1850 `llm_planner.rs` 172-187; 1855 `docker-compose.yml:200`; 1894 `browser_agent.rs:20`; 2068 `browser_agent.rs` 738-751 (plausible, but `require_quarry_redaction_stamp` is not defined in that file).
- 1808 `agent.rs` in-flight hunks 571, 599, 749, 769, 1937, 4566, 7185: not checked against `git diff` hunk headers.
- 2031 `lib.rs` 128-176: lib.rs has 30 lines, so the call is probably in main.rs.
- 2077 `observation.rs` 672-848; 2191 `test_support.rs` and `tests/*.rs` lines (attribution ambiguous); 2202 `routes.rs` 440; 2214 `browser_driver.rs` 429 and 501; 2242 `shared.rs` 104.

<!-- COMPLETE -->
