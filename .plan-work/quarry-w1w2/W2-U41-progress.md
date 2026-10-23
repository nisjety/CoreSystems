# W2-U41 progress: WS17.E3 backfill, erasure hop, plaintext removal (batch 7)

Status: BUILT to scope. Code committed (four commits, below). Tests WRITTEN and compile-checked, NOT run (wave build strategy). Deployment evidence NOT run. Flags default off (erasure hop off while QUARRY_EDGE_URL is empty; backfill operator-run; encryption mode unchanged).

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.E3 detail". Queue: W2-queue.md section 3, BATCH 7, U41. Hand-over: .plan-work/quarry-w1w2/wf-W2-U41.md. Depth: .plan-work/quarry-w1w2/depth/W2-U41.md.

## Commits (local, quarryv2-next, not pushed)

| Hash | Subject | Content |
|---|---|---|
| 54958bf96 | feat(profile): backfill, org erasure and migration 0017 for profile envelopes (WS17.E3) | runtime: migrations/0017_profile_plaintext_null.sql (new), org_erasure.rs (new), profile_backfill.rs (new), plaintext_page and purge_org on the blob store, evict_org hook, two mod lines |
| afe212786 | feat(control): erasure hop to quarry-edge for the profile families (WS17.E3) | Go: internal/gdpr/edge_purger.go (new), edge_purger_test.go (new), erasure.go (HandleErasureWithEdge), subscriber.go, erasure_test.go (fakes and cases), cmd/control/main.go |
| d1eb42d22 | feat(edge): internal org erasure route and profiles_backfill binary (WS17.E3) | edge: internal_routes.rs (new), bin/profiles_backfill.rs (new), AppState.org_erasables and its four literals, main.rs wiring, one route line, verify_internal_token pub(crate), mod lines |
| (next) | docs(quarry): WS17.E3 RUNBOOKS section; docker-compose env (quarry-control, empty defaults); plan-work progress, hand-over and depth | docs/RUNBOOKS.md (append), apps/Ingestion Plane/docker-compose.yml (two env lines), .plan-work/quarry-w1w2 files |

The commits carry only this unit's hunks. Other units' uncommitted hunks in quarry-edge (routes.rs, main.rs, state.rs, lib.rs and others), in the runtime lib.rs and in the shared RUNBOOKS and compose stay in the working tree. The commits were built from HEAD blobs with the scripts in scratchpad/W2-U41 (lib-head.js, head-edit*.js, commit-blobs.sh).

## Compile checks (qtest, quarry-rust-env:2; shared working tree)

- `cargo check -p quarry-runtime --tests --features postgres-queue`: rc 0 (retry after another unit fixed its policy_mode gap). The first attempt failed on quarry-browser (capture_plan, another unit) and on the policy_mode gap; both were outside this unit.
- `cargo check -p quarry-edge --lib --bin profiles_backfill --features postgres-queue`: rc 0.
- `cargo check -p quarry-edge --tests --features postgres-queue`: the lib and the integration test targets compile; the bin-root test target fails with `schedule_preflight` not in main.rs (WS15.S2 unit's edit: the module is in lib.rs, main.rs lacks the mod line). Not this unit's file; not touched. Re-run after that unit lands.
- `go vet ./internal/gdpr/ ./cmd/control/` (services/quarry-control): rc 0. gofmt clean on the new Go files.
- Not run: any `cargo test` or `go test`.

## Tests named in the plan: status

Written, compile-checked (runtime test target), NOT run:
- profile_backfill.rs: backfill_seals_every_plaintext_row_and_is_idempotent; backfill_cas_loses_to_a_newer_save; backfill_unlinks_old_plaintext_cache_keys (REDIS_URL-gated).
- org_erasure.rs: glob_escape_neutralises_pattern_characters; purge_org_deletes_rows_tombstones_and_cache_keys (in-memory part always; Redis part REDIS_URL-gated).
- Go: TestHandleErasureWithEdge_PurgesStoreThenEdge, _NilEdgeIsTheStoreOnly, _EdgeFailureIsRetryableNotPoison, _UserSubjectNeverReachesTheEdge (erasure_test.go); TestEdgePurgerFromEnv_UnsetIsOff, _MissingTokenFailsEveryCall, _RejectsNonHTTPURL, TestHTTPEdgePurger_SendsBearerAndParsesCounts, _EscapesOrgID, _RefusalIsAnError (edge_purger_test.go). Vet-compiled only.

Not written (depth): the Rust route test for purge_org_profiles; the Go handleDelivery NAK path test; the Postgres-lane versions of the backfill tests.

## Plan items and status

- Migration 0017: written. Idempotent (UPDATE with a guard; CHECK added in a DO block that ignores duplicate_object). Applies on the next edge boot. Header and RUNBOOK state the rollback hazard. Not applied anywhere.
- `profiles_backfill` binary: built; checked. No dry-run mode (depth).
- Erasure route: built; checked by the lib build. Route test not written (depth).
- Control erasure consumer hop: built; vet-checked. Edge hop is OFF by default.
- Compose: two env lines on quarry-control, empty defaults.
- RUNBOOKS: one section appended (WS17.E3).
- Deployment evidence: not run (test phase; needs a live stack).

## Deviations from the plan text (recorded)

- Legacy plaintext cache keys are also removed in the erasure (the plan names only the v2 keys for the route). The purge never touches the v2 namespace through the legacy pattern.
- The backfill unlinks the legacy keys with one keyspace SCAN (quarry:profile:*, skipping v2) after the rows, not one SCAN per org. Same effect, one pass.
- The Go hop logs the counts through the existing gdpr line; the edge answers counts per family.
- The Go erasure tests live in erasure_test.go (as planned) plus a new edge_purger_test.go.
- Commit trailer uses Co-Authored-By: Claude Haiku 5.5 (the attribution rule in the session), not the Sonnet line the batch text gave. The batch text's own rule said to use the owner's convention; the session's attribution rule applies here.

## Shared-tree notes for the next batch

- quarry-edge main.rs needs `mod schedule_preflight;` (WS15.S2 unit) before the bin-root test target compiles.
- docs/RUNBOOKS.md and docker-compose.yml in the working tree carry other units' hunks; the U41 section and env lines are committed (see the docs commit).
- The runtime test target depends on quarry-browser compiling (capture_plan feature gate): another unit's.
- Migration numbering: 0017 is taken by this unit. WS12.S7 (0014) and WS12.S3 (0013) are not in the tree yet.
