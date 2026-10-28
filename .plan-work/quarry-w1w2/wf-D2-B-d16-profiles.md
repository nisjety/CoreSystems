# D2-B-d16-profiles hand-over (2026-10-28)

Done (decision 3.5 D16-Q2, option C; enforce stays `observe`, nothing flipped):
1. Default scope: a stored profile with no metadata is `user_private` in every store (`ProfileMetadata::stored_default()`): InMemoryProfileStore, InMemoryBlobStore, S3 list, Postgres fallback; the edge reads a metadata-less row the same way. `ProfileScope::default()` stays `ephemeral`. Tests changed because they contradicted the decision: `session::tests::list_carries_name_and_scope_from_metadata` (expected `Ephemeral`) and the Redis-gated `cached_profile_store::tests::metadata_methods_pass_through_to_inner_uncached` (expected `None` after a save; it ran nowhere).
2. `ProfileStore::stamp_owner` and `ProfileBlobStore::stamp_owner` (insert-only, tombstone-aware, ordered by the request start), implemented for the in-memory and Postgres stores, forwarded by the cache and envelope wrappers.
3. `start_run` stamps the person's profile owner after the caps, policy and lease (`profile_guard::stamp_run_owner`): people only, never a service principal, never a resumed run, not under `off`. A store error never fails the start.
4. Counters `quarry_profile_owner_decisions_total` and `quarry_profile_owner_stamps_total`, closed labels.
5. `scripts/evidence/d16-legacy-profile-count.{sql,sh}` for the stack session (read-only; `--check` offline). RUNBOOKS has an appended section that corrects the stale WS17.P1/P2 lines. The D2-044 and DB2P-01 ledger lines are marked resolved.

Left for the owner / stack session (see depth/D2-B-d16-profiles.md): rebuild the edge (stamp live), run the count and note the time, observe a week, choose rescue or backfill, flip `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=enforce`, run the two-user evidence (RUNBOOKS section "Profile owner: stamp at run start").

Traps:
- Postgres store code and its tests only compile with `--features postgres-queue`; the default build never sees them. The test helper `snap()` there did not compile (new SessionSnapshot fields) until this unit.
- `main.rs` and `lib.rs` of quarry-edge both declare `mod profile_guard` and `mod metrics`: any new item in them must compile in both crates.
- `InMemoryProfileStore::save_metadata` is an upsert for name and scope; do not use it for the run stamp (it overwrites a named profile). Use `stamp_owner`.
- A service token's `actor_id()` is `service:<name>`; the stamp must never use it.
