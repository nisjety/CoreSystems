# D2-B-d16-profiles progress (2026-10-28)

Unit: decision 3.5 D16-Q2, option C. Enforce stays observe.

## Commits
- aa2f64988 feat(quarry-browser): a stored profile defaults to user_private in every store; ProfileStore::stamp_owner. Files: quarry-browser/src/session.rs; quarry-runtime/src/{profile_blob_store,postgres_profile_store,cached_profile_store,envelope_profile_store,s3_profile_store}.rs.
- 3f9017d62 feat(quarry-edge): stamp the profile owner at run start for people's runs; observe-mode counters. Files: quarry-edge/src/{profile_guard,metrics}.rs, src/agent_routes/lifecycle.rs, tests/profile_attach.rs.
- docs commit: scripts/evidence/d16-legacy-profile-count.{sql,sh}, docs/RUNBOOKS.md (appended section), this file, wf-D2-B-d16-profiles.md, depth/D2-B-d16-profiles.md, the D2-044 and DB2P-01 ledger lines.

## Test commands (qtest, Linux container) and results
- cargo test -p quarry-browser --lib session: 52 passed.
- cargo test -p quarry-runtime --lib profile: 48 passed; with --features postgres-queue: 63 passed (Postgres and Redis tests return early without DATABASE_URL/REDIS_URL: not exercised).
- cargo test -p quarry-edge --lib profile_guard: 16 passed.
- cargo test -p quarry-edge --test profile_owner --test profile_guard --test profile_attach: 8 + 8 + 10 passed.
- Script: bash scripts/evidence/d16-legacy-profile-count.sh --check OK; the SQL ran in a throwaway postgres:16-alpine with migrations 0002/0005/0015/0016/0017 and six synthetic rows (counts matched).
- cargo check --workspace --all-targets: exit 0 (no error); cargo check -p quarry-runtime -p quarry-edge --features quarry-edge/postgres-queue --all-targets: no error.

## Uncommitted files of this unit
None after the docs commit. Strays that are not mine and were left: quarry-edge lib.rs/main.rs, tests/support/fixture_site.rs, quarry-runtime step_receipts.rs, .plan-work/next-slices.md, rb.patch.
