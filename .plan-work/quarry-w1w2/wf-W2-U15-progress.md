# wf-W2-U15 progress (WS17.P1: monotonic saves, deletion tombstones, exists, migration 0015)

Status: BUILT and committed (2026-10-23). Test phase not run. See W2-U15-progress.md for the final state.

## Commits
- 27d0657f4 feat(profile): ordered saves, deletion tombstones and exists (WS17.P1) (code, migration 0015, hunk-only chromiumoxide.rs and main.rs)
- 28e2a8d3d style(profile): rustfmt the WS17.P1 changes
- ae70857ca style(profile): restore two pre-existing lines the rustfmt pass touched
- de1be35b6 docs(profile): reflow the cache module doc
- fc44dfa55 ci(quarry-v2): advisory Postgres job (WS17.P1)
- a4531c99b docs(quarry): RUNBOOKS section (hunk-only)
- (this commit) notes: W2-U15-progress.md, depth/W2-U15.md, this file

## Left for other owners
- chromiumoxide.rs: U17 (WS12.S1) hunks uncommitted; main.rs: U13 (WS7.C) hunks uncommitted; RUNBOOKS.md: another unit hunks uncommitted.

## Final checks
- Compile after all code and formatting changes: see W2-U15-progress.md, section Compile checks.


Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md "Slice WS17.P1 detail" (line ~3043).
Queue row: .plan-work/quarry-w1w2/W2-queue.md section 3, BATCH 3, U15.

## Planned work (owned or hot-edited)
- NEW crates/quarry-runtime/migrations/0015_profile_ordering.sql (idempotent DDL)
- crates/quarry-browser/src/session.rs: SaveMeta, SaveOutcome, save signature, exists default method, InMemory ordering + tombstones, in-memory tests
- crates/quarry-runtime/src/postgres_profile_store.rs: conditional upsert, transactional delete with tombstone, exists, purge_expired_tombstones, sweeper fn, Postgres tests (written, not run)
- crates/quarry-runtime/src/cached_profile_store.rs: invalidate-on-write, exists pass-through, test updates
- crates/quarry-runtime/src/s3_profile_store.rs: signature only (unordered; documented)
- crates/quarry-edge/src/profile_routes.rs: SaveMeta owner on save_profile, update_profile uses exists, logs outcome class
- crates/quarry-browser/src/chromiumoxide.rs (H4 region edit): persist_current_page (675-697 region) + test call near line 7036
- crates/quarry-browser/src/kernel.rs: save call near 587 and test near 1098
- crates/quarry-edge/src/main.rs (H3, one line): spawn tombstone sweeper
- .github/workflows/quarry-v2-ci.yml (H25 append-only job)
- docs/RUNBOOKS.md (append-only section)

## Committed
(see the Commits section above)

## Built (uncommitted, in the shared tree)
- migration 0015, session.rs (SaveMeta, SaveOutcome, clamp, in-memory ordering, tests), postgres_profile_store.rs (ordered upsert, tombstoning delete, exists, sweeper, tests), cached_profile_store.rs (invalidate-on-write, test), s3_profile_store.rs (signature), profile_routes.rs (owner stamp, exists), chromiumoxide.rs (persist stamp; H4 region only), kernel.rs (release stamp), main.rs (one sweeper line in the Postgres arm; U13 hunks also in file, not ours), zdr_refusal_modes.rs (mechanical), .github/workflows/quarry-v2-ci.yml (job quarry-postgres-store, advisory), docs/RUNBOOKS.md (section appended; file also carries another unit change, hunk-only commit needed)
- depth: .plan-work/quarry-w1w2/depth/W2-U15.md (11 bullets)

## Compile check
- 2026-10-23 qtest cargo check --tests -p quarry-browser --all-features: FAILED at my 2 test call sites (SaveMeta path, fixed by qualifying the path in chromiumoxide.rs test and kernel.rs test) and at chromiumoxide.rs:2248 BrowserNativeProjection missing field redaction (U17 WS12.S1 in-progress hunk, not ours). runtime and edge checks not reached (depend on browser lib).
- next: re-run the check after U17 lands its hunk

## Next
1. migration
2. session.rs trait + in-memory
3. postgres store
4. cached, s3
5. edge routes, chromiumoxide, kernel, main
6. compile-check (cargo check --tests) via qtest
7. commits, depth file, W2-U15-progress.md

## Known traps found while reading
- lease (quarry-core/src/lease.rs BrowserLease) carries no actor id: capture path SaveMeta owner = None. Depth bullet.
- save_metadata has no SaveMeta: create_profile owner not stamped in P1. Depth bullet.
- Test call sites of ProfileStore::save in session.rs tests, postgres tests, cached tests, kernel.rs test (~1098), chromiumoxide.rs test (~7036) all need the new 4th argument.
- Files are CRLF (session.rs, postgres_profile_store.rs, cached_profile_store.rs, profile_routes.rs, s3_profile_store.rs, kernel.rs, main.rs, lease.rs); chromiumoxide.rs is LF-only (no CRLF detected by `file`). Use lib-edit.js.
- Worktree also contains a nested .claude/worktrees/vigilant-banach-6933aa copy: ignore it.
