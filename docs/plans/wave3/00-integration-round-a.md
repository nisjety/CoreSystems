# Wave 3 integration, round A (EP-13, EP-14, EP-8, EP-2, EP-6, EP-10)

Written 2026-11-02 at HEAD `c18196af2` (branch `model-plane`). Numbers below were checked against the tree: session-core head `0071_run_question_consume`, capability-core head `0030_tool_policy_expiry`, cost-core head `0011_cache_miss_reason`; protos opened in `apps/Model Plane/proto/model_plane/v1`. The only design edits made: the nine `<next>` migration placeholders replaced with the numbers in section 1.

## 1. Reservation table

### 1.1 Migrations

| Number | Service | Design / slice | Name |
|---|---|---|---|
| 0072 | session-core | EP-8 S4 | `0072_user_profiles` (+ `revert/0072_user_profiles.down.sql`) |
| 0073 | session-core | EP-14 S6 | `0073_run_request_snapshots` (+ revert) |
| 0074 | session-core | EP-10 S4 | `0074_approval_descriptor_revision` (+ revert) |
| 0075 | session-core | EP-8 S6 | `0075_agent_memory_scope_key` (+ revert) |
| 0076 | session-core | EP-14 S10 | `0076_routing_policy_candidates` (+ revert) |
| 0012 | cost-core | EP-13 S8 | `0012_gemini_model_pricing` (.up/.down) |
| none | capability-core | all | head stays 0030. EP-2 S8 uses a new key in the existing `org_agent_policy` JSONB; EP-13 must not re-enable the soft-deleted gemini row |
| none | Convex | EP-6 S7 | schema.ts additions only (no numbering) |
| none | user-core | EP-8, EP-14 | settings are JSON categories (`ai.tonePreset`, `transcriptDensity`); EP-14 S2 must verify table shape at slice start |

Rules: a number belongs to its slice even if slices land out of order (do not renumber; gaps 0063/0065/0066 already exist so gaps are tolerated). Anything not in this table that wants a session-core migration takes 0077 and up and is added here first. Check `git ls-tree` of other branches before the first merge, since no other branch reservation was found in the plan docs (only a grep of docs, not of unmerged branches).

### 1.2 Proto fields

| File / message | Field | Owner | Status |
|---|---|---|---|
| inference.proto `ModelInfo` | `context_window_tokens=9`, `max_output_tokens=10`, `lifecycle=11` | EP-13 S2 | verified free (last is 8) |
| inference.proto `InferRequest` | `purpose=21` (new enum `InferPurpose`) | EP-13 S3 | verified free (last is 20) |
| inference.proto `InferRequest` | `run_id=22` | EP-14 S6 | **moved from 21** (see C1) |
| inference.proto `InferRequest` | `role_fallback_model=23` | EP-13 S4 | **moved from 22** (see C1) |
| inference.proto `InferRequest` | `call_purpose` | EP-14 | **dropped**, use EP-13 `purpose` (C1) |
| inference.proto `RouteNote` | `role=5`, `role_source=6` | EP-13 S4 | verified free (last is 4) |
| inference.proto `ChatMessage` | 5 left free | EP-13 | intentional |
| execution.proto `RunContext` | `user_profile=7` | EP-8 S5 | 1-4 used; 5,6 reserved by comment for WS08/WS20 |
| execution.proto `RunContext` | `agent_roles=8` (`AgentRoleSpec`) | EP-6 S10 | **moved from 7** (C2) |
| execution.proto `RunAgentRequest` | `wide_research=25` | EP-2 S8 | verified free (last is 24). If EP-6 S9 needs an agent ref it takes 26 |
| execution.proto `RunAgentResponse` | `round_timing=12` | EP-14 S5 | verified free (last is 11) |
| orchestration.proto `LineageEdge` | `goal_summary=5`, `batch_id=6`, `status=7` | EP-2 S9 | verified free (last is 4) |
| orchestration.proto `DecideApprovalRequest` | `edit=9` | EP-10 S4 | verified (last is 8) |
| orchestration.proto `Approval` | `revision=17` | EP-10 S4 | verified (last is 16) |
| orchestration.proto `ApprovalStateChanged` | `revision=11`, `edited_fields=12` | EP-10 S4 | EP-10 says 1-10 used; not re-counted |
| orchestration.proto `ConsumeApprovalResponse` | `revision=8`, `original_action_fingerprint=9` | EP-10 S4 | verified (last is 7) |
| orchestration.proto | rpc `GetApprovalPreview` + messages | EP-10 S1 | new names, no clash |
| memory.proto `MemoryEntry` | `scope=9`, `scope_id=10`, `source_label=11` | EP-8 S6 | verified free (last is 8) |
| memory.proto `SearchMemoryResponse` | `withheld=5` | EP-8 S7 | verified free (last is 4) |
| memory.proto `ListMemoryRequest` | `space_id=4` | EP-8 S7 | verified free (last is 3) |
| memory.proto | rpcs `Get/Set/RebuildUserProfile` + messages | EP-8 S4 | appended after `ResetMemory` |
| events.proto `EventType` | `RUN_TIMING_RECORDED=150` | EP-14 S5 | last used 146; 147-149 kept free as slack |
| routing_policy.proto | `RoutingPolicyMessage.candidate_id=5`, `SetRoutingPolicyRequest.expected_version=3`, candidate RPCs | EP-14 S10 | verified (last are 4 and 2) |
| NEW run_ledger.proto | `RunTimingRecorded`, `RunActionTiming`, `RunLedgerService` | EP-14 S5-S7 | add to mp-contracts `build.rs` list |
| SSE (taxonomy gate, not proto) | `Citation.quote/locator`, `research_plan` event, `RecordedArtifact.provenance` | EP-2 S4/S5 | one owner, no clash |

### 1.3 Flags

No two designs use the same name. All are default off. Total 47 new names (EP-13: 4, EP-14: 14, EP-8: 8, EP-2: 10, EP-6: 5 new, EP-10: 3 new, reuses `TOOL_POLICY_ENFORCED`, `VITE_SCHEDULE_RESULTS`, `CRON_CONTRACT_GUARDS_ENABLED`, `SESSION_CORE_SCHEDULED_RESULT_PERSIST`). Near-misses to keep distinct: `MODEL_GATEWAY_WEB_READER` (EP-2) vs `EXECUTION_WEB_READER` (EP-2 S10); `SESSION_USER_PROFILE_ENABLED` vs `MODEL_GATEWAY_USER_PROFILE_PREFIX` (EP-8, deliberate pair). `APPROVAL_EDIT_ENABLED` is one name read by three services (EP-10): fine, but deploy order is exec-core, then session-core, then gateway.

## 2. Hot-file matrix

Count is the number of designs naming the file (basename match). Convention for every row: one slice at a time in the file, rebase onto the previous landing, CRLF preserved, new behaviour behind its own flag, tests added not rewritten.

| File | Designs | Order / rule |
|---|---|---|
| `model-gateway http_routes.rs` | all 6 | Largest conflict surface. Land additive route blocks at the end of the router; never reformat. |
| `session-core grpc.rs` | all 6 | Same. One new RPC handler per slice in its own new module (`run_ledger_grpc.rs` pattern from EP-14); grpc.rs gets only the registration line. |
| `FES shared/actions/action-registry.ts` (+ test) | all 6 | Append-only entries; test file count assertions will collide, rebase each time. |
| `model-gateway sse.rs` | EP-13, 14, 2, 6, 8 | EP-13 S3 (purpose tag at every call site) first, since it touches ~26 call sites; others after. |
| `execution-core runtime_loop/agent.rs` | EP-10, 13, 14, 2, 6 | WS03 ordering stays: EP-13 S3 call-site tags, then EP-14 S5, EP-10 S5, EP-2 S10 / EP-6 S10 (both near `subagent.task`, `:6969`). |
| `zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs` | EP-10, 2, 6, 8 | Each slice adds rows; tests that assert row count change every time. |
| `tool_loop.rs` | EP-10, 13, 14, 2 | EP-2 S2/S3 (web tools) and EP-10 S8 (`suggest_connector`) add match arms: independent. |
| `gdpr.rs` | EP-14, 2, 6, 8 | EP-14 S6 and EP-8 S4/S6/S7 add erasure statements; each needs its own test row. |
| `execution.proto` | EP-14, 2, 6, 8 | Numbers fixed in 1.2; buf lint/breaking each time. |
| `inference.proto` | EP-13, 14 | Numbers fixed in 1.2; EP-13 S3 lands first. |
| `orchestration.proto` | EP-10, 2, 6 | Disjoint messages. |
| `chat-client.ts`, `use-chat-controller.ts`, `ChatMessages.tsx`, `sse_events.rs` | EP-10, 14, 2 (+8, 13 for chat-client) | FE chat surface: EP-14 S1-S3 (fold, density, step strip) restructure ChatMessages; land them before EP-2 S4 chip/passage UI and EP-10 S6 editable card, or those two rebase onto a moved file. |
| `FE routing-policy page` / `RoutingPolicy` JSON schema | EP-13 S4/S5, EP-14 S10 | See C3. |
| Model Plane compose file | all | Every flag lands here; additive lines only. |
| `MP/docs/decisions/ledger.md` (CRLF), depth register | all | One ledger entry per slice; append at top. |

## 3. Contradictions and recommended resolutions

C1. **`InferRequest` fields 21/22 claimed twice, and two purpose vocabularies.** EP-13 S3 takes `purpose=21` and S4 `role_fallback_model=22`; EP-14 S6 takes `run_id=21` and `call_purpose=22` (its own closed string list). Resolution: EP-13 owns purpose: `purpose=21` (enum `InferPurpose`), EP-14 `run_id=22`, EP-13 `role_fallback_model=23`; EP-14 drops `call_purpose` and snapshots/metrics use the enum. Consequence: EP-14 S6 depends on EP-13 S3 (or lands `run_id` first and backfills purpose later). EP-14's own note (section 7 item 7) already says EP-13 owns the call-purpose vocabulary. Both design files need this edit by their owners (not done here; only placeholders were touched).

C2. **`RunContext` field 7 claimed twice** (EP-8 `user_profile`, EP-6 `agent_roles`). Resolution: EP-8 gets 7 (lands earlier), EP-6 gets 8. Fields 5 and 6 stay reserved for WS08/WS20 per the comment at `execution.proto:332`.

C3. **Routing policy: two writers of one platform singleton.** EP-13 S4/S5 add role entries inside the RoutingPolicy JSON and give the router-policy page a role editor that round-trips the whole policy via PUT; EP-14 S10 turns PUT into "propose candidate" behind `ROUTING_POLICY_PROMOTION_REQUIRED`, with promote needing an `expected_version`. Resolution: EP-14 D10 already builds on today's schema with opaque JSON validated by inference-core's serde, so roles ride in the candidate unchanged. Keep EP-13 S5 using PUT (switch off); when EP-14 S10 flips, the role editor must be moved to propose/promote in the same change. The action contract for the policy page must be written once (EP-13 S5) and extended (EP-14 S10), not forked. Add an EP-14 S10 test that a candidate containing roles validates and promotes.

C4. **Memory key naming.** The plan and EP-6 phase 3 say `(scope_kind, scope_id)`; EP-8 D3 keeps the column `scope` and adds `scope_id`. Resolution: EP-8 wins (no rename, closed CHECK added NOT VALID then VALIDATE). EP-6 P3 wording should be updated when phase 3 is designed. D3 needs the Space session's acknowledgement before S6.

C5. **Agent memory authority vs agent runs.** EP-8 D1 lets `agent_run` threads read the owner's memory but not write; EP-6 OD-6 says unattended runs use the WS20 stored-consent contract with service credentials. These are consistent only if "owner" resolves to the schedule creator on a fire. Resolution: state in EP-8 S2 test list that a scheduled fire reads memory as the creator subject and writes none; EP-6 P3 re-checks.

C6. **Prompt inspector vs new prompt sections.** EP-14 D3 shows platform sections as label+digest only and user/memory sections as text; EP-8 adds locale, tone and user-profile sections and EP-6 adds role instructions, none registered in EP-14's label table. Resolution: EP-14 S7 label table must list them and classify user_profile as user text (owner-only), tone/locale as platform text, agent role instructions as org text. Add as an EP-14 S7 acceptance line.

C7. **Waived/ZDR handling is consistent** (EP-14 D13, EP-8 D11, EP-2 OD-6, EP-13 never-ZDR Gemini): all refuse persistence on ZDR and waived turns. No change; record once in the ledger so reviewers do not re-ask.

C8. **Two stores for similar data: none found.** Request snapshots (EP-14, session-core) vs EP-2's stream-only research ledger are deliberately separate; EP-8 user_profiles vs Letta-bridge memory blocks: EP-8 S4 must state the profile is built from session-core stated memories only and does not read letta blocks (check at S4 start).

C9. **Same owner decision under two names:** EP-6 OD-5 (allowlist/autonomy cap in Convex) and EP-8 D12 (agent scope) both defer to DEC-14. One ruling should cover both.

## 4. Spot-check results

Seven randomly chosen `path:line` references per design (42 total) were opened. Abbreviations resolved from each design header.

| Design | Result |
|---|---|
| EP-13 | 7/7 match (e.g. `ProviderCapabilities` struct, `INJECTION_MODEL_ENV`, `PriceSource` enum, `list_models`). |
| EP-14 | 7/7 match (events.proto zdr comment, `delete_thread_rows`, intent insert with `args_digest`/`effect_class`, `handle_record_trajectory`). |
| EP-8 | 7/7 match (RunContext fields, `threads_origin_chk` closed set, `load_agent_memory_context_rows`, `PromptContext`). |
| EP-2 | 7/7 match; `turn_evidence_metadata` is at `session_flow.rs:1939` as cited and `RecordedCitation` is in `sse_events.rs` at the cited range. |
| EP-6 | 7/7 match; `can_author_own_schedule` citation lives in `BFF/domains/agent_actions.rs` (not model-gateway), correctly written in the design. |
| EP-10 | 7/7 match (`MAX_SNAPSHOT_BYTES`=64 KiB, `approvalReason`, `DecideApprovalBody`, capability seed rows). |

Proto "next free" claims verified against the tree for InferRequest, ModelInfo, RouteNote, RunAgentRequest, RunAgentResponse, LineageEdge, MemoryEntry, SearchMemoryResponse, ListMemoryRequest, DecideApprovalRequest, Approval, ConsumeApprovalResponse, RoutingPolicy messages and EventType (146). Mismatches: none in line references. The only inconsistencies are the cross-design collisions in C1 and C2. Not independently re-counted: `ApprovalStateChanged` (nested message at `orchestration.proto:1305`), EP-2 `RunAgentRequest` beyond the highest tag, and CRLF state of files.

## 5. Recommended build order (this round)

Startable now with no owner decision (flag flips and live proofs excluded):

1. **EP-13 S3** (purpose tag; fixes C1 numbering, unblocks EP-14 S6), then **S2**, **S1** (price join, switch off), **S4**, **S5**. S6 and S7 can build dark; S8 waits on D1, D2, D3.
2. **EP-8 S2** (fail-closed containment fix, small and security-relevant) first, then **S1**, **S9**, **S3** (D6 recommended default), **S4** after S2 merges, **S5** after S4, **S8** after S1 and S3. S6, S7, S10 wait on D3, D5, D11, D12.
3. **EP-14 S1, S3, S4** (FE only), **S8, S9** (CI and replay; only the required-check flip needs D8). S2 needs D5, S5 needs Q-D2-199, S6 needs D1/D2/D4, S7 needs D3, S10 needs D9 for the flip only.
4. **EP-10 S2, S3, S1**, then **S4** (after S3), **S7**, **S8**; then S5, S6, S9. S10 is blocked on OD-3 and OD-6.
5. **EP-6 S0, S1, S2, S5**, then S3, S4, S6 (live proof needs WS20-S1/S3 live). Phase 2 waits on OD-4, OD-5.
6. **EP-2 S1** (FE only), then **S2, S3** (OD-5 only gates enablement), **S4**, **S5**, **S6**, **S8**, **S10**. S7 waits on OD-2, S9 enforcement on OD-3, S8 clamp on OD-4.

Sequencing across designs: chat-surface FE slices (EP-14 S1-S3) before EP-2 S4 and EP-10 S6; EP-13 S3 before any slice adding a new inference call site; EP-14 S6 after EP-13 S3; migrations may land in any order.

Owner decisions that unblock the most slices: EP-13 D1/D2/D3 (Gemini enablement), EP-14 D1/D2/D4 (snapshots), EP-8 D3/D5/D11 (Space memory), EP-10 OD-3/OD-6 (live writes), EP-6 OD-4 (agent versions), DEC-14 (EP-6 OD-5 and EP-8 D12 together).
