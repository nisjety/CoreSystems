# EP-8-S7 evidence: Space memory recall, owner list and delete, erasure

Build status: partial (code first). Recall, the owner list and delete, Space-deletion erasure, the per-user GDPR source erasure, the proto fields, the model-gateway and BFF routes and the SPA tab are written with their unit and DB tests. Nothing was compiled or run in this slice: the batch guard compiles it. Three pieces live outside this slice and are lead or other-plane steps: the Control `model.space.memory.manage` decision (user-core), the GDPR subscriber wiring (`gdpr_nats.rs`) and the tab's mount point and action contract. The live proof is a lead step and is not claimed here.

## Decisions assumed (owner order 2026-11-03, "code first, stop asking")

| Id | Taken as | Where it shows |
|---|---|---|
| D3 | `scope` is the kind, `scope_id` the instance (S6's migration `0103`) | every reader filters `scope = 'space' AND scope_id = <Space>` |
| D4 | isolation: a shared room recalls its own Space rows only; no user, org, thread or global row | `SPACE_RECALL_SQL`, test `space_recall_never_returns_user_or_org_rows` |
| D5 | recall only rows whose `audience_hash` equals the thread's current one; the rest counted as `withheld` | `SearchMemoryResponse.withheld = 5`, test `audience_change_withholds_old_rows` |
| D11 | refuse: a waived thread recalls nothing (`skip_reason = zdr`); a ZDR caller never reaches `SearchMemory` | test `waived_space_thread_recalls_nothing` |
| GDPR (DPO) | a row only the erased person stated is deleted; a row other members also stated loses the person's message link and stays | `gdpr::space_source_erasure`, test `gdpr_erasure_drops_user_sources` |
| Role-bearing decision | a new Control action `model.space.memory.manage`, schema `sha256:space-memory-manage-v1`, sole permission `memory:manage`, bound to operation and row | `grpc::verify_space_memory_decision` |

## Switches

- `SESSION_SPACE_MEMORY_ENABLED` (session-core, S6's switch, strict parser): recall. Off, `load_agent_memory_context_rows` and `SearchMemory` take exactly today's path.
- `GATEWAY_SPACE_MEMORY_UI` (BFF `AppState`, only the literal `true`): the two `/api/v1/spaces/{space_ref}/memory` routes. Off answers 404 `feature_unavailable`.
- List and delete in Session Core are not behind a switch: they need a Control decision that no Control issues today, and a delete must stay possible after the recall switch is turned off again. Space-deletion erasure and GDPR erasure are not behind a switch either: they delete rows that exist only if the switch was ever on.

## What changed

| File | Change |
|---|---|
| `MP/proto/model_plane/v1/memory.proto` | `SearchMemoryResponse.withheld = 5`; `ListMemoryRequest` and `DeleteMemoryRequest` gain `space_id = 4`, `space_control_decision_ref = 5`, `space_control_decision_token = 6`. Additive, commented. |
| `SC/src/dreaming.rs` | `space_recall_target`, `recall_space_memory`, `list_space_memory`, `delete_space_memory`; the context reader takes the Space path for a shared room with the switch on. |
| `SC/src/memory_grpc.rs` | `SearchMemory` Space branch (no Letta); `ListMemory`/`DeleteMemory` with `space_id` under the Control decision and the revocation check. |
| `SC/src/grpc.rs` | the `model.space.memory.manage` verifier; Space deletion deletes `scope IN ('space','agent')` rows of the Space in its transaction. |
| `SC/src/gdpr.rs` | `parse_user_erasure_event`, `space_source_erasure`, `erase_user_space_memory_sources`. |
| `MG/http_routes.rs` | `POST /v1/spaces/:space_id/memory/list` and `/delete` (decision in the body). |
| `BFF/domains/spaces/memory.rs` (new) | `GET /api/v1/spaces/{space_ref}/memory`, `DELETE .../memory/{memory_id}`. |
| `FES/features/spaces/components/SpaceMemoryTab.tsx` (new) | list, withheld explanation, delete with in-place confirmation. |

## Tests written (not run in this slice)

- session-core unit: `space_recall_target_only_for_a_contained_thread_with_the_switch_on`, `space_recall_sql_reads_only_the_space_scope_of_one_space`, `space_memory_decision_binds_subject_space_operation_and_row`, `space_memory_decision_refuses_any_other_permission_set`, `non_owner_cannot_list_space_memory`, `user_erasure_is_parsed_and_every_other_subject_is_not`, `space_source_erasure_deletes_sole_rows_and_narrows_shared_ones`.
- session-core DB harness (`#[ignore]`): `space_recall_returns_only_this_space`, `space_recall_never_returns_user_or_org_rows`, `audience_change_withholds_old_rows`, `other_space_rows_never_returned`, `waived_space_thread_recalls_nothing`, `space_deletion_erases_space_rows`, `gdpr_erasure_drops_user_sources`.
- model-gateway unit: `space_memory_fields_are_bounded_and_trimmed`, `space_memory_refusals_use_fixed_words_and_object_details`.
- BFF: `space_memory_route_resolves_space_server_side`, `space_memory_ids_are_validated`, `space_memory_answer_is_rebuilt_field_by_field`.
- SPA: `SpaceMemoryTab.test.tsx` (6 cases). e2e: `tests/e2e/space-memory.spec.ts` (local only).

## Lead steps

1. Apply the shared-file lines in the slice report, regenerate the proto bindings, add the four surface-registry rows and re-pin.
2. `cargo test -q -p session-core --lib space_` and the DB harness with the seven DB tests above; `cargo test -q -p model-gateway --lib space_memory`; BFF `--all-targets space_memory`; `pnpm exec vitest run src/features/spaces/components/SpaceMemoryTab.test.tsx`.
3. Live proof (after Control issues the decision): Room A states a fact, Room B asks for it and gets nothing; Room A asks and gets it "from this Space". Add a member to A: the old row is withheld and the tab shows the count. Delete Space A: `psql` shows no `scope='space'` row for it.
