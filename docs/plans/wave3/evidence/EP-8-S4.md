# EP-8-S4 evidence: standing user profile (store, builder, editor)

Build status: code written, nothing run (code-first batch W3-08, owner order 2026-11-03). Every result below is still to be produced by the guard and the lead. The live proof is a lead step and is not claimed here.

## Switch

`SESSION_USER_PROFILE_ENABLED` (session-core), default off, read by the strict parser `mp_contracts::loop_fold::switch_from` (only the literal `true` enables it). `MemoryGrpc` reads it once at start. Off: `GetUserProfile`, `SetUserProfile` and `RebuildUserProfile` answer `failed_precondition` `profile_not_enabled` (after authorization, before any database read), Dreaming never rebuilds, and the SPA hides the card on that code. Erasure is not switched: ResetMemory, a memory delete and the organization purge always reach `user_profiles`, so a profile written while the switch was on is never left behind by turning it off.

## What the builder reads

session-core `agent_memory` only (integration round A, C8): the person's own `scope = 'user'`, `session_id IS NULL`, `review_state = 'accepted'`, `is_latest`, unexpired rows that carry a source link (`source_links <> '{}'`, design risk 4), excluding legacy room-origin rows. Never org, thread, Space or agent rows, and never letta-bridge blocks. Stated rows first, then unknown-provenance (phrase matcher) rows, then inferred rows with confidence >= 0.8; one line per key and per text; preference, fact, instruction; cut at 1400 characters on a line boundary.

## Migration

`0100_user_profiles.sql` (round A reserved 0072, held by `0072_space_event_log` on this branch; round B moves round A's five to 0100-0104 and EP-8 S4 is the first). It also widens the closed `memory_control_events.event` CHECK with `profile_edited` and `profile_rebuilt` (content-free audit rows). Reversal: `revert/0100_user_profiles.down.sql`.

## Tests written (not run)

| Command | Tests |
|---|---|
| `cd "apps/Model Plane/rust" && cargo test -q -p session-core --lib user_profile` | `build_orders_stated_before_inferred`, `build_respects_1400_chars_on_line_boundary`, `build_skips_non_user_scopes`, `build_never_reads_space_or_org_rows`, `build_keeps_one_line_per_key_and_per_text`, `without_sources_drops_only_the_deleted_lines`, `the_migration_and_its_revert_match`, `switch_off_refuses_every_profile_rpc` |
| session-core DB harness (`--ignored`, `DATABASE_URL` naming a `test` database) | `rebuild_refused_when_paused`, `edited_profile_survives_rebuild_without_force`, `delete_memory_drops_its_line_on_rebuild`, `reset_deletes_profile`, `stale_revision_refused`, `zdr_caller_cannot_set_profile`, `gdpr_erasure_deletes_profile` |
| `cargo test -q -p session-core --lib gdpr` | `purge_summary_total_sums_every_field` (now 33) |
| `cargo test -q -p model-gateway --lib user_profile_route_tests` | `profile_routes_take_identity_from_claims`, `profile_refusals_keep_their_typed_codes` |
| `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets profile` | `profile_read_passes_the_switch_refusal_through`, `profile_edit_forwards_only_text_and_revision`, `profile_rebuild_without_force_is_forwarded_as_false` |
| `pnpm exec vitest run src/features/settings/components/MemorySection.profile.test.tsx src/features/settings/components/MemorySection.test.tsx src/shared/actions` | 8 card tests; the registry test for `memory.profile.save` / `memory.profile.rebuild` |
| `cd "apps/Model Plane/proto" && buf lint && buf breaking --against proto/baseline.binpb && buf generate` | three RPCs and five messages appended to `memory.proto`; no existing tag moves |
| `bash scripts/tests/zdr-contract-sync.test.sh` | feature `user_profile` in both catalogue copies (pins to be regenerated) |

## Live proof (lead, depth phase)

Seed three stated memories through the UI ("remember that I prefer tables"), run one Dreaming cycle with the switch on, open Settings > Memory: the profile shows three lines, each linking to its memory row. Edit one line; delete one memory; "Rebuild" without force (the Dreaming path) keeps the edit and the card shows "1 kilde(r) er slettet siden du redigerte". Capture screenshots and `psql` rows of `user_profiles` and `memory_control_events` (event, actor, detail only).

## Before the switch goes on

1. The pre-S2 report: count `scope = 'user'` rows whose `source_links` point at messages in Space threads; reject them (`review_state = 'rejected'`) with a ledger note if any exist.
2. D2-480 (capability-core writes `agent_memory` around the gate with `review_state = 'accepted'`): the builder's `source_links <> '{}'` filter keeps rows with no trace out; confirm capability-core rows carry no links before relying on it.
