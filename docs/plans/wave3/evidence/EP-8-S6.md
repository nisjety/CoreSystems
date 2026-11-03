# EP-8-S6 evidence: Space memory scope, migration and writers (dark)

Build status: partial (code first). The migration, the writers, the proto fields and the unit and DB tests are written. Nothing was compiled or run in this slice: the batch guard compiles it, and session-core does not build until the lead adds `mod space_memory;` to `main.rs`. The live proof is a lead step and is not claimed here.

## Switch

`SESSION_SPACE_MEMORY_ENABLED` (session-core), default off, strict parser (only the literal `true` enables it). Off, the three writers do what they did before: the inline writer returns before any query, Dreaming takes the existing `skipped_space_scope` branch, and an `IndexMemory` SPACE save stays a thread note.

## Decisions taken by default (owner order "code first", 2026-11-03)

- D3 = (b): `scope` stays the kind with a closed CHECK added NOT VALID; `scope_id`, `audience_hash`, `audience_revision`, `source_label` added. capability-core keeps its smaller enum.
- D5 = (b): every Space row carries the audience hash and revision it was learned under (recall and `withheld` are S7).
- D11 = refuse: a thread with a waived run and a waived credential write nothing; ZDR credentials never reach a writer.
- D4 = (a): Space rows are isolated; nothing here reads them.

## What changed

| File | Change |
|---|---|
| `SC/migrations/0103_agent_memory_scope_key.sql` (+ revert) | four columns, four NOT VALID CHECKs (closed scope set, scope_id iff space/agent, Space audience required, closed source_label), one partial index. Numbered 0103 per round B's renumber of round A's five (0075 is taken on this branch). |
| `SC/src/space_memory.rs` (new) | stamp table, room-row re-scoping (explicit "remember that" only), inline writer, Dreaming pass, IndexMemory save. No Letta. |
| `SC/src/grpc.rs` | the contained arm of the append calls `space_memory::write_inline(space_memory::enabled(), ...)`. |
| `SC/src/dreaming.rs` | a contained thread with the switch on goes to `space_memory::mine_room_turns`; `stable_fragment` and `collapse_whitespace` are `pub(crate)`. |
| `SC/src/memory_grpc.rs` | SPACE save in a shared room with the switch on goes to `space_memory::index_space_memory`; the new `MemoryEntry` fields are set empty. |
| `MP/proto/model_plane/v1/memory.proto` | `MemoryEntry.scope = 11`, `scope_id = 12`, `source_label = 13` (9 and 10 were taken by WS17-S2). |
| `CC/internal/api/memory_space_scope_test.go` (new) | capability-core cannot write or list `space`/`agent`. |

## Tests written, not run

- `cargo test -q -p session-core --lib space_memory`: `enabled_only_by_the_literal_true`, `stamp_table`, `skip_labels_are_closed_tokens`, `space_candidate_never_user_scope`, `two_spaces_never_share_a_key`, `assistant_and_inferred_candidates_never_become_room_rows`, `scope_for_topic_space`, `space_rows_not_mirrored_to_letta`, `the_switch_off_path_reads_nothing`, `every_writer_is_behind_the_switch`, `migration_closes_the_scope_set_and_has_a_revert`.
- DB harness (`#[ignore]`): `space_turn_writes_space_row_with_audience_stamp`, `space_turn_without_decision_writes_nothing`, `waived_space_thread_not_mined`, `switch_off_space_writes_nothing`, `background_pass_writes_the_room_row_and_skips_external_content`, `space_save_is_stamped_with_the_thread_audience`, `migration_constraints_reject_unknown_scope`.
- `cd "MP/go/services/capability-core" && go test ./internal/api/ -run TestCapabilityCoreScopeEnumCannotWriteSpace`.

## Live proof (lead, before the switch goes on)

Switch on in dev, state "Husk at vi leverer til Bergen på fredager." in a shared room, run a Dreaming cycle; `psql` shows one `scope='space'` row with the room's `scope_id` and audience hash and zero new `user` rows. Before that: run `SELECT DISTINCT scope FROM agent_memory` and, if it lists only the closed set, `VALIDATE CONSTRAINT` the four checks. The switch stays off until D2-480, D2-481 and D2-461 are closed.
