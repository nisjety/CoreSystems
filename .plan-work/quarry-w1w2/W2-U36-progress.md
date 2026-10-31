# W2-U36 progress: WS17.E1 envelope layer (batch 6)

Status: BUILT to scope. Code committed in three commits (below). Tests WRITTEN and compile-checked, NOT run (the test phase is at the end of wave 2). Deployment evidence NOT run. Flag default off.

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.E1 detail". Queue: W2-queue.md section 3, BATCH 6, U36. Hand-over: .plan-work/quarry-w1w2/wf-W2-U36.md. Depth: .plan-work/quarry-w1w2/depth/W2-U36.md.

## Commits (local, quarryv2-next, not pushed)

Hashes: f43b72dc2 (runtime envelope layer), ad21ef5c9 (LF line endings for Cargo.toml and the S3 banner, a fix to f43b72dc2), 3565f6bfa (edge wiring and compose), a71bb8ebf (CONTRACTS and RUNBOOKS). Index is LF; the working copy is CRLF by autocrlf.


| Subject | Content |
|---|---|
| feat(profile): envelope encryption layer for profile snapshots (WS17.E1) | runtime Cargo.toml deps (ring, zeroize, moka, flate2); migrations/0016_profile_envelope.sql; profile_crypto.rs, profile_keys.rs, profile_blob_store.rs, envelope_profile_store.rs (new); postgres_profile_store.rs (PostgresProfileBlobStore, shared ordered upsert); cached_profile_store.rs (CachedBlobStore); s3_profile_store.rs (banner); lib.rs (four mod lines only, U26's `pub mod capture` left uncommitted) |
| feat(edge): profile encryption boot checks and store wiring (WS17.E1) | config.rs (three fields); profile_encryption.rs (new, boot checks); main.rs (my hunks only, WS17.P0 boot-warning hunk left uncommitted); docker-compose.yml (five env lines, defaults off) |
| docs(quarry): profile envelope storage in CONTRACTS and RUNBOOKS (WS17.E1) | docs/CONTRACTS.md (storage and encryption block); docs/RUNBOOKS.md (one appended section, built from HEAD; the working copy of RUNBOOKS carries other units' line-ending changes, not committed) |
| docs(plan-work): W2-U36 progress, depth and hand-over notes (WS17.E1) | this file, wf-W2-U36.md, depth/W2-U36.md |

## Compile checks (qtest, quarry-rust-env:2)

- `cargo check -p quarry-runtime --tests --features postgres-queue`: rc 0, no warnings.
- `cargo check -p quarry-runtime --tests` (default features): rc 0, no warnings.
- `cargo check -p quarry-edge --tests --features postgres-queue`: rc 0. Warnings only in files this unit does not own (fleet_routes.rs, run_live.rs, artifacts_fs_list.rs, zdr_tombstone.rs, browser_grants_prod.rs) and in tests/profile_guard.rs (WS17.P0 file, unused import).
- `cargo check -p quarry-edge --tests` (default features): rc 0.
- Not run: any `cargo test`, the Postgres lane, the Redis lane, the edge boot tests, gateway, `cargo audit`.

## Tests named in the plan: status

Written (unit, in the module named):
- profile_crypto.rs: seal_open_roundtrip, tampered_ciphertext_fails, aad_binds_org_profile_and_snapshot, aad_is_length_prefixed_not_separator_ambiguous, nonces_are_unique_across_10000_seals, empty_and_max_size_snapshots, gzip_threshold, known_answer_vector (determinism and round trip only; the pinned vector is owed), unknown_version_and_compression_are_refused (extra).
- profile_keys.rs: env_provider_wrap_unwrap, unwrap_with_wrong_org_fails, unknown_master_is_destroyed_and_rewrap_moves_to_current, key_cache_ttl_and_org_eviction (eviction only; TTL not tested), config_refuses_weak_or_missing_material (extra).
- profile_blob_store.rs: put_ignores_older_and_tombstoned_saves, reseal_is_cas_on_rev_and_clears_legacy_on_get, metadata_and_list_do_not_touch_payload.
- envelope_profile_store.rs: mode_parse_refuses_unknown_values, blob_cache_keys_are_namespaced_away_from_legacy_keys (partial for old_plaintext_cache_keys_are_ignored), compat_mode_writes_plaintext, on_mode_nulls_plaintext_and_holds_no_canary, plaintext_row_is_resealed_lazily_with_cas, key_unavailable_is_not_start_fresh, delete_then_exists_is_false_and_tombstone_refuses_older_save, s3_profile_store_is_not_constructed_in_production_code.
- cached_profile_store.rs (blob_tests, REDIS_URL gated): cache_holds_envelope_bytes_and_never_cookie_values, legacy_rows_are_never_cached, delete_and_reseal_invalidate_the_blob_cache.
- postgres_profile_store.rs (blob_tests, DATABASE_URL gated): envelope_row_roundtrip_and_reseal_cas.
- profile_encryption.rs (edge): off_needs_no_keys_and_required_refuses_it, required_refuses_compat_and_accepts_on, compat_and_on_need_master_keys_and_an_active_id, org_core_and_unknown_values_are_refused.

Not written: the Postgres-lane versions of plaintext_row_is_resealed_lazily_with_cas, compat_mode_writes_plaintext and on_mode_nulls_plaintext; key_cache TTL; the route-level 503 check. Recorded in depth.

## Flags and settings

`QUARRY_EDGE__PROFILE_ENCRYPTION` (off default), `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED` (false), `QUARRY_EDGE__PROFILE_KEY_PROVIDER` (env), `QUARRY_PROFILE_MASTER_KEYS`, `QUARRY_PROFILE_MASTER_KEY_ACTIVE`. Compose lines added with those defaults.

## Not done (in scope, deferred, with reasons)

- org_core key provider and Control Plane migration 024 (WS17.E2, WAIT-OTHER).
- Master key minting and the secrets ledger entry (a deploy step; no key was minted here).
- Deployment evidence and restore latency (test phase; needs the rebuilt image and a live stack).
- External review of profile_crypto.rs and envelope_profile_store.rs (required before REQUIRED in production).
- Pinned known-answer ciphertext (needs a test run to produce it).
- SOVEREIGN_SCRAPING_RELIABILITY.md status line (doc pass).

## Shared-tree notes for the next batch

- main.rs: the WS17.P0 boot-warning hunk is still uncommitted in the working tree (not this unit's).
- lib.rs: U26's `pub mod capture;` line is still uncommitted in the working tree.
- docs/RUNBOOKS.md: the working copy has a large line-ending change from another unit (1158 CRLF lines vs HEAD). Not committed by this unit; the appended section is in HEAD via the docs commit.
- profile_routes.rs (WS17.P0) is uncommitted and maps store errors generically; the RuntimeNotReady mapping of key failures reaches the client unless the route layer maps it (depth).
- Later units editing profile stores: the envelope layer sits over PostgresProfileBlobStore; keep the ordered upsert (`upsert_payload`) as the single write path.
