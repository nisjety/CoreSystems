# W2-U15 progress: WS17.P1 (monotonic saves, deletion tombstones, exists, migration 0015)

Status: BUILT and committed to scope on branch quarryv2-next (local, not pushed). Tests WRITTEN and compile-checked, NOT run (test phase at the end of the wave). Deployment evidence NOT run.

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.P1 detail". Queue: W2-queue.md section 3, BATCH 3, U15.

## Commits (in order)

| Commit | Subject | Content |
|---|---|---|
| 27d0657f4 | feat(profile): ordered saves, deletion tombstones and exists (WS17.P1) | migration 0015; session.rs; postgres_profile_store.rs; cached_profile_store.rs; s3_profile_store.rs; profile_routes.rs; kernel.rs; zdr_refusal_modes.rs (mechanical); chromiumoxide.rs HUNK ONLY (persist_current_page and one test seed); main.rs HUNK ONLY (sweeper spawn in the Postgres arm) |
| 28e2a8d3d | style(profile): rustfmt the WS17.P1 changes | formatting of session.rs, postgres_profile_store.rs, cached_profile_store.rs, profile_routes.rs, kernel.rs (three lines of the new save block only) |
| ae70857ca | style(profile): restore two pre-existing lines the rustfmt pass touched | refuse_for_claims call in profile_routes.rs; redact_credentials assert in cached_profile_store.rs |
| de1be35b6 | docs(profile): reflow the cache module doc after WS17.P1 | cached_profile_store.rs module doc |
| fc44dfa55 | ci(quarry-v2): advisory Postgres job for the profile store (WS17.P1) | .github/workflows/quarry-v2-ci.yml, job quarry-postgres-store (continue-on-error) |
| a4531c99b | docs(quarry): RUNBOOKS section for the WS17.P1 profile store (W2-U15) | docs/RUNBOOKS.md, one appended section, HUNK ONLY |
| (this commit) | docs(plan-work): W2-U15 progress and depth notes (WS17.P1) | .plan-work/quarry-w1w2/W2-U15-progress.md, depth/W2-U15.md, wf-W2-U15-progress.md |

Foreign hunks left UNCOMMITTED for their owners: chromiumoxide.rs (WS12.S1 / U17 hunks: native projection, redaction, page projection, live tests), main.rs (WS7.C / U13 hunks: policy baseline, web_policy_edge), docs/RUNBOOKS.md (another unit's hunks, the 66-line deletion block), plus unrelated files of other units. None of them were touched by this unit.

## What the slice does (as built)

- `ProfileStore::save(org, pid, snapshot, &SaveMeta) -> QuarryResult<SaveOutcome>`. `SaveMeta { saved_at, owner }` (saved_at = capture START). `SaveOutcome`: `Stored { rev }`, `IgnoredStale`, `IgnoredByTombstone`; `reason_class()` returns `stored|stale|tombstone` for logs.
- `ProfileStore::exists` added with a default impl (via load); Postgres overrides with a row probe; cache passes through; in-memory overrides.
- `clamp_saved_at` (session.rs): `saved_at` clamped to now + 5 s. Postgres clamps with `LEAST(..., NOW() + 5 s)` in the same statement.
- Postgres save: one conditional upsert (tombstone NOT EXISTS guard, ON CONFLICT ... WHERE saved_at IS NULL OR saved_at < EXCLUDED.saved_at), `rev = rev + 1` only on Stored, owner stamped on INSERT only. Diagnose query only when nothing was written (reason class).
- Postgres delete: tombstone upsert (GREATEST) and DELETE in one transaction; TTL 7 days (`TOMBSTONE_TTL_DAYS`).
- `purge_expired_tombstones()` and `spawn_tombstone_sweeper(store)` (hourly, first tick at boot), started from main.rs.
- Redis cache: invalidate-on-write (DEL after the inner answer, on every outcome). No write-through.
- S3 store: unordered; reports `Stored { rev: 0 }`. Documented in code and in the depth file.
- Edge: `save_profile` stamps `SaveMeta::now(Some(claims.actor_id()))`; logs `profile save ignored reason=...` for ignored saves and still answers 200 (depth: decision). `update_profile` uses `exists`.
- Capture paths: chromiumoxide `persist_current_page` and kernel `release` stamp the capture START (`captured_at` taken before the snapshot), owner None, log the outcome class.

## Flag

None. Strict tightening (plan: "Flag: none"). Rows with `saved_at NULL` accept the next ordered save.

## Migration

`crates/quarry-runtime/migrations/0015_profile_ordering.sql` (new, idempotent): `saved_at`, `rev`, `owner_user_id` on `quarry_profiles`; `snapshot` DROP NOT NULL; `quarry_profile_tombstones` + expiry index. Free numbers after this: 0016 (WS17.E1), 0017 (WS17.E3).

## Tests named in the plan, written, NOT run

- In-memory (`crates/quarry-browser/src/session.rs` tests): `save_with_older_saved_at_is_ignored`, `save_after_delete_with_older_saved_at_is_ignored_by_tombstone`, `save_after_delete_with_newer_saved_at_creates_a_new_row`, `saved_at_is_clamped_to_now_plus_5s`, `rev_increments_only_on_stored`, `owner_is_stamped_on_insert_only`, `exists_matches_load`.
- Postgres (`crates/quarry-runtime/src/postgres_profile_store.rs`, needs DATABASE_URL): the same names, plus `delete_writes_tombstone_in_same_transaction`, `expired_tombstones_are_swept`. The existing four Postgres tests take the default meta.
- Redis (`crates/quarry-runtime/src/cached_profile_store.rs`, needs REDIS_URL): `cache_is_invalidated_not_populated_on_ignored_save`.
- Test-phase commands: `cargo test -p quarry-browser session`; `DATABASE_URL=postgres://... cargo test -p quarry-runtime --features postgres-queue profile`; `REDIS_URL=... cargo test -p quarry-runtime cached_profile`.
- CI: job `quarry-postgres-store` (advisory) runs the `profile` tests on Postgres 16. Not run here. The YAML was not parsed locally (no parser on this host).

## Compile checks (qtest, quarry-rust-env:2)

- Before the rustfmt commit: `cargo check --tests -p quarry-browser --all-features` rc=0 (after the other unit's BrowserNativeProjection hunk landed); `cargo check --tests -p quarry-runtime --features postgres-queue` rc=0; `cargo check --tests -p quarry-edge` rc=0; `cargo check -p quarry-edge --features postgres-queue` rc=0; warning-location listing for the edge build with postgres-queue shows no warning in any file this unit touched.
- After the rustfmt commit (28e2a8d3d): `cargo check --tests -p quarry-browser --all-features`, `cargo check --tests -p quarry-runtime --features postgres-queue` and `cargo check --tests -p quarry-edge --features postgres-queue` all rc=0 (log: scratchpad W2-U15/check5.log). That run started before the two follow-up commits ae70857ca (restores two original lines) and de1be35b6 (doc reflow); both are formatting or doc only and were not re-compiled. rustfmt changed whitespace and line breaks only.
- rustfmt (host 1.9.0): the parents of session.rs, postgres_profile_store.rs and s3_profile_store.rs were clean, so the pass changed only this unit's lines there. The parents of cached_profile_store.rs, profile_routes.rs and kernel.rs were not clean. The pass also reformatted two pre-existing lines (restored in ae70857ca). The remaining changes in those files are this unit's lines (checked against the pre-change parent). kernel.rs: only the three lines of the new save block are reformatted; a pre-existing rustfmt diff near the state-fetch warning (about line 553) is left alone.

## Hand-over: what the next batch needs to know

1. `ProfileStore::save` has a NEW signature (`&SaveMeta`, returns `SaveOutcome`). Any new implementor or call site must use it. All call sites in the tree are converted (session, postgres, cached, s3, profile_routes, chromiumoxide, kernel, zdr_refusal_modes).
2. `ProfileStore::exists` is a new trait method with a default impl.
3. WS17.P0 (U30, batch 5) and WS17.E1 (U36, batch 6) edit `cached_profile_store.rs` and `postgres_profile_store.rs`. E1 replaces the invalidate-on-write cache with the blob-level cache. Keep the ordering semantics: E1 must keep `ON CONFLICT ... WHERE saved_at ...` (or move it) and the tombstone guard.
4. WS17.C1 (U55, `chromiumoxide.rs` profile capture, H4 serial queue) edits the same capture path: `persist_current_page` now takes `captured_at` before `capture_snapshot` and calls `save` with `SaveMeta`. C1 must keep the capture-start stamp.
5. WS17.P2 (U24) needs the owner stamp on capture and on create. The browser lease carries no actor id and `save_metadata` has no ordering metadata (depth bullets, high severity). Decide before P2 enforces ownership.
6. The S3 store is unordered (depth). Do not treat it as ordered.
7. main.rs: the sweeper is started in the Postgres arm; the handle is detached (depth, WS6.J drain).

## Not done (in scope but deferred)

- Owner stamp on capture and on create (WS17.P2; needs a lease actor field and an ordering-aware save_metadata; both are cross-unit changes).
- Test suites (test phase). Deployment evidence (test phase: rebuild the edge image, migration 0015, the three checks in the RUNBOOKS section).
- S3 ordering (depth, medium).
- The 200-on-ignored-save API decision (depth, medium).

Depth items: `.plan-work/quarry-w1w2/depth/W2-U15.md` (severity-tagged, see the file).
