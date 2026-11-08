# C5 path and citation check: quarry-browser build plan, lines 2279-2926

Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md` (worktree copy, read only). Code truth: live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Tags from `.plan-work/tools/provenance.sh`.

## Counts

- Paths checked: 245 unique product-path tokens (`apps/`, `.github/`, `scripts/`, `docs/`, `proto/`; ellipsis placeholders and globs excluded), each run through provenance.sh: C 122, M 31, U 4, MISSING 88. These were joined to 307 table path/tag pairs and 215 prose mentions.
- Tag mismatches (plan tag disagrees with provenance): 13 occurrences (9 table rows, 4 prose). Other tag defects: 1 `[NEW]` path that already exists (2912), 1 tag list short by one (2512), 1 test named under a `[C]` file that does not exist (2321), 2 untagged WS6.A dependencies (2281, 2293), 3 placeholder paths not written in full (2608, 2630, 2710).
- Missing: 88 unique MISSING tokens. 78 are `[NEW]` in the plan and consistent. 10 need action: 2 wrong command-directory paths, 3 bare relative roots (`docs/openapi.yaml`, `docs/ERROR_REASONS.md`, `proto/baseline.binpb`), 2 untagged WS6.A dependencies (`limits.go`, `limits.rs`), 1 fixture-relative name (`docs/a.html`), 2 command strings (`scripts/e2e/crawl-scope-robots.sh --expect ...`).
- Line citations checked: 383. Each cited range was opened in the live file (cited range plus or minus 40 lines) and the symbol the plan names was located; every row that was not an exact hit was re-read by hand. Result: OK 350; DRIFT 14; WRONG 7; wrong file path 7; planned or new symbol not yet in tree 4 (2321, 2529, 2855, 2883); no claim to check 1.
- In-flight line claims (git hunks and numstat against HEAD, about 20 claims): 2 DRIFT (2291, 2349); the rest match.

## Failures

| Plan line | Path[:line] | Plan tag/claim | Actual | Proposed correction |
|---|---|---|---|---|
| 2466 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/cmd/control/main.go:192 | [C] 192 (control Temporal client passed as nil) | MISSING (wrong path). File is services/quarry-control/cmd/control/main.go; line 192 is `resources.MountScheduleAliases(r, db, nil)`, so the claim is true | Cite apps/Ingestion Plane/Quarry-v2/services/quarry-control/cmd/control/main.go:192 |
| 2529 | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/cmd/orchestrator/main.go:145-152 | [C] | MISSING (wrong path). File is services/quarry-orchestrator/cmd/orchestrator/main.go; 145-152 is the jobsMgr block (`jobs.New` at 145, `go jobsMgr.Run` at 152). `RunControl` is not wired yet (L2 adds it) | Cite apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/cmd/orchestrator/main.go:145-152 |
| 2582 | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/cmd/orchestrator/main.go (registration 94-117) | [C] | MISSING (wrong path). Registration block is correct at cmd/orchestrator/main.go:94-117 | Use apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/cmd/orchestrator/main.go |
| 2602 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/cmd/control/main.go:192 | [C] | MISSING (wrong path). cmd/control/main.go:192 holds `MountScheduleAliases(r, db, nil)` | Use apps/Ingestion Plane/Quarry-v2/services/quarry-control/cmd/control/main.go:192 |
| 2605 | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/cmd/orchestrator/main.go:132-139 | [C] | MISSING (wrong path). cmd/orchestrator/main.go:132-139 holds `schedMgr := schedules.New(` | Use apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/cmd/orchestrator/main.go:132-139 |
| 2626 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/cmd/control/main.go:53-62 | [C] (notify block) | MISSING (wrong path). cmd/control/main.go:53-62 is the notify block (line 55 comment) | Use apps/Ingestion Plane/Quarry-v2/services/quarry-control/cmd/control/main.go:53-62 |
| 2913 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/cmd/control/main.go:203-230 | [C] (wiring) | MISSING (wrong path). cmd/control/main.go:203-230 holds the erasure wiring | Use apps/Ingestion Plane/Quarry-v2/services/quarry-control/cmd/control/main.go:203-230 |
| 2617 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/schedule_preflight.rs | [C] after S2 | MISSING today. S2 creates it (row 2587 is [NEW]) | Tag [NEW from S2]; S4 then edits it |
| 2623 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/schedule_runs.go | [C] after S2 | MISSING today. S2 creates it (row 2586 is [NEW]) | Tag [NEW from S2] |
| 2742 | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs:178 | [C] | M: in-flight hunks in the live tree. (structured_extract.rs and contracts.rs 941-957 are C and correct.) | Tag [M] |
| 2542 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/jobs_lifecycle.go | [C after L1] | MISSING today. L1 creates it (row 2502 is [NEW]) | Tag [NEW from L1]; L4 then edits it |
| 2542 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/job_lifecycle_routes.rs | [C after L1] | MISSING today. L1 creates it (row 2508 is [NEW]) | Tag [NEW from L1] |
| 2781 | docs/openapi.yaml | [M] | MISSING at repo root (bare relative path). The file is apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml, which is M (tag right) | Write apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml |
| 2912 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/profile_routes.rs (alternative in parentheses) | [NEW] | C: exists, committed, unchanged. [NEW] fits only internal_routes.rs | Tag the alternative [C] (edit in place); keep internal_routes.rs [NEW] |
| 2512 | apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml, CONTRACTS.md, REST_RESOURCES.md | [M], [C] (two tags, three paths) | M, C, C. Values match, but the tag list is one short | Make the tag list [M], [C], [C] |
| 2608 | apps/Frontend Plane/verevonv3/src/.../VerevonIngestionsPage.tsx | [C] (ellipsis, not a full path) | Exists as apps/Frontend Plane/verevonv3/src/features/ingestions/components/VerevonIngestionsPage.tsx, C | Write the full path |
| 2630 | apps/Frontend Plane/verevonv3/src/.../VerevonIngestionsPage.tsx | [C] (ellipsis) | Same file as 2608 | Write the full path |
| 2710 | apps/Control Plane/auth-core/src/auth/fixtures/... | [M] (ellipsis) | The full file apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json is M (tag right) | Write the full path |
| 2474 | docs/ERROR_REASONS.md | [NEW, owner WS2] (bare relative path) | MISSING, consistent with NEW. No file exists at repo root or in apps/Ingestion Plane/Quarry-v2/docs/ | Write apps/Ingestion Plane/Quarry-v2/docs/ERROR_REASONS.md [NEW] |
| 2758 | proto/baseline.binpb | "is 0 bytes" (bare relative path) | Exists at apps/Model Plane/proto/baseline.binpb (C), 0 bytes. Claim true | Write apps/Model Plane/proto/baseline.binpb |
| 2281 | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/jobs/limits.go | untagged (WS6.A dependency) | MISSING. No tagged slice creates it | Tag [NEW from WS6.A] |
| 2281, 2293 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/limits.rs | untagged (WS6.A dependency) | MISSING | Tag [NEW from WS6.A] |
| 2321 | apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/workflows/workflows_test.go (TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy) | [C] | File exists; the named test exists nowhere under services/quarry-orchestrator. S0 must add it | Say "[C] file; S0 adds the test" |
| 2349 | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs (live diff at 42, 160-165) | [M] | In-flight hunks at 42 (adds `pub mod crawl_signals;`, not crawl_filter) and 160-163 | Cite 42 and 160-163 |
| 2291 | apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml (+78 at 238-250 and 4012-4150) | [M] | 77 insertions and 1 changed line. Hunks: 238, 243-250, 4012-4031, 4052-4057, 4102-4143 | Cite 4012-4143 |
| 2375 | apps/Frontend Plane/verevonv3/src/shared/api/knowledge-client.ts:716-760 | `normalizeCrawlEvent` (about 716-760) | DRIFT. Function begins at 713 | Cite 713-760 |
| 2538 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/store/pg/resources.go:1018-1030 | "seq is MAX(seq)+1 then insert (1018-1030)" | DRIFT. INSERT at 984-987; NextSeq (`MAX(seq)` at 999, +1) at 996-1005. 1018-1030 is FindByIdempotencyKey | Cite 984-1005 |
| 2538 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs:773-1000 | "test module (773-1000)" | DRIFT. `#[cfg(test)]` at 766 | Cite 766-1000 |
| 2540 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ingestions/runs.rs:24 | "RUN_KINDS (24)" | DRIFT. `const RUN_KINDS` at 27 | Cite 27 |
| 2540 | apps/Frontend Plane/verevonv3/src/shared/api/knowledge-client.ts:619-643 | `streamCrawlRunEvents` (619-643) | DRIFT. Function begins at 616 | Cite 616-643 |
| 2567 | apps/Ingestion Plane/Quarry-v2/services/quarry-control/internal/resources/cycle23.go:325-330 | `scheduleBackfill` (325-) | DRIFT. `func scheduleBackfill` is at 331 | Cite 331- |
| 2648 | apps/Model Plane/rust/services/model-gateway/src/normalize.rs:237 | caps content at 100 KiB (normalize.rs 11, 237) | DRIFT. Cap check is at 238; the constant at 11 is correct | Cite 238 |
| 2651 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs:512 (range "512+") | "the gate and tests at 205-228 and 512+" | DRIFT. `#[cfg(test)] mod tests` starts at 374; test functions run 427-595 (structured tests at 515, 565, 595) | Cite mod tests 374-639 and structured tests 515-595 |
| 2777 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/profile_routes.rs:410-411 | `restorable` (410-411) | DRIFT. `let restorable =` is at 409 (expression 409-411) | Cite 409-411 |
| 2810 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/profile_roundtrip.rs:107 | "assertions at 107" | DRIFT. No assert at 107; nearest asserts 96 and 111 | Cite 111 (cookie-shape asserts are 117-138) |
| 2810 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/profile_roundtrip.rs:209 | "assertions at 209" | DRIFT. No assert at 209; nearest 203-204 and 213 | Cite 203-204, 213 |
| 2810 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/profile_roundtrip.rs:414 | "assertions at 414" | DRIFT. No assert at 414; nearest 384 and 418 | Cite 384, 418 |
| 2810 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/profile_roundtrip.rs:456 | "assertions at 456" | DRIFT. No assert at 456 (file is 461 lines); nearest 418 | Cite 418 |
| 2826 | apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/kernel.rs:575-585 | "kernel save (575-585)" | DRIFT. `.save(` call is at 586 | Cite 586 |
| 2738 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs:38-58 | "handler (38-58, 205-233, 242-331, 344-360)" | WRONG label. 38-58 is `pub struct ExtractRequest` (prompt field at 48 is correct). The handler `pub async fn extract` is at 184 | Relabel 38-58 as the request struct; handler starts at 184 (205-233, 242-331 and 344-360 verified) |
| 2846 | apps/Ingestion Plane/Quarry-v2/docs/SOVEREIGN_SCRAPING_RELIABILITY.md:134 | "134, 289 status note" | WRONG. No status note at 134 | The only status line is line 3 ("Status: proposed backlog feature"); drop 134 or cite 3 |
| 2846 | apps/Ingestion Plane/Quarry-v2/docs/SOVEREIGN_SCRAPING_RELIABILITY.md:289 | "134, 289 status note" | WRONG. No status note at 289 | Drop 289 or cite line 3 |
| 2886 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:1533 | "session info (1533+)" | WRONG. 1533 is inside start_run's resume recovery navigation. The session projection is the `browser_session` handler at 1669 | Cite agent_routes.rs:1669 (browser_session) |
| 2649 | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:8946 | "applies neither `injection_defense` nor required PII redaction (those live on `invoke`, 8946)" | WRONG (partial). 8946 is the PII gate `pii_redaction_required`. http_routes.rs has no injection reference. Injection screening is in apps/Model Plane/rust/services/model-gateway/src/moderation.rs (`scan_injection` 183, `screen_tool_payload` 437, `injection_defense_enabled` 493) | Cite 8946 for PII only; cite moderation.rs for injection screening; confirm which invoke path, if any, calls it |
| 2740 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:246-251 | "builder at `mp.ai_formats` decision (246-251, reuse `build_model_client`)" | WRONG (partial). 246-250 is `model_plane_gate::decide(...)`, features bound at 252. `mp.ai_formats` is not written in main.rs. `build_model_client` is defined at 957 (used at 986, 1024, 1202, 1402) | Cite main.rs:246-252 for the gate decision and main.rs:957 for build_model_client |
| 2779 | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs:268-299 | "routes.rs [M] ... `load_profile` 268-299" | WRONG file. `load_profile` is defined at profile_routes.rs:268-299; routes.rs:144 only mounts `crate::profile_routes::load_profile` | Cite apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/profile_routes.rs:268-299 |

## Notes (not failures)

- 2688: `apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go` is C. The "[UNVERIFIED]" marker qualifies the table name, not the path.
- 2782: `services/quarry-control/internal/gdpr/*` is a glob, not a path.
- 2334-2335: names such as `docs/a.html` and `index.html` are fixture-relative files inside `crawl-fixture/a` and `crawl-fixture/b`.
- 2321, 2529, 2855, 2883: named symbols or files not yet in the tree (planned by S0, L2, P0 and WS3.C). Expected, not failures.
- 2647-2652: many citations use bare names (for example `ai_formats.rs:275`, `mp_client.rs:433`). Resolved to `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/` or `quarry-edge/src/`; they match. Full repo-relative paths would remove the ambiguity.
- Verified as stated: routes.rs 563-588; schedule_routes.rs 307-340; auth.rs 504-545; action-registry.ts 1320-1329; grpc.rs 2568 (cap constant at grpc.rs:320 is 80_000); queue_routes.rs 200-202 (internal routes only, no purge route); ai_formats.rs 472 lines; model_plane_gate.rs 401 lines; extract_routes.rs 639 lines; structured_extract.rs 277 lines; zdr_claim_floor.rs 888-889; proto/baseline.binpb 0 bytes; tool_loop.rs in-flight hunks at 3039 and 3115; MP numstat claims (http_routes +237, grpc +47, auth +415, tool_loop +425).

<!-- COMPLETE -->
