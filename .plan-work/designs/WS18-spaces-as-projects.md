# WS18-spaces-as-projects - Spaces as the chat project

Prefixes: `FP` = `apps/Frontend Plane/verevonv3`; `FPG` = `FP/apps/gateway`; `MP` = `apps/Model Plane`; `GW` = `MP/rust/services/model-gateway`; `SC` = `MP/rust/services/session-core`; `EC` = `MP/rust/services/execution-core`; `UC` = `apps/Control Plane/user-core`; `AP` = `apps/Application Plane`. `[wt]` = opened in worktree (HEAD afb83928), `[live]` = opened only in `C:/dev/CoresSystem`. Line numbers are worktree unless marked.

## 1. Scope, items resolved, owner

**Item:** F-t3-013 (Priority, W2, audit effort L). Owner of the stream: Frontend Plane (BFF + SPA) with Model Plane (instruction layering, retrieval receipt, Do-run knowledge scope). Application Plane (Convex) is storage only; Control stays the membership/decision authority (master matrix rules 9-10: `apps/master-ownership-matrix.md:165-166`; the card's "Application Plane membership" is wrong, see section 2).

**Result in one line:** a chat project is a Space (a named room, or the personal Space). A project chat is an ordinary Space-scoped thread started from /chat; Space instructions, Space knowledge and the Control-signed Space decisions already exist. The work is wiring, three silent scoping gaps the audit did not list, and one honest badge.

| Slice | Tag | What |
|---|---|---|
| S0 | [FINISH] | Record OD19 ruling + the partial reversal (ledger, ADR-0003 addendum, close plan I21) |
| S1 | [FINISH] | Pin the Space layer in Do runs on top of WS02 S4+S6 (tests, evidence; no second field) |
| S2 | [FINISH] | Follow-up turns of a Space thread carry the Space retrieval decision (today they retrieve unscoped) |
| S3 | [BUILD] | Knowledge-mode receipt: extend `source_scope_applied`, SPA badge (Ask) |
| S4 | [BUILD] | Project picker in the chat sidebar + composer chip + project thread list (Ask and Do) |
| S5 | [BUILD] | Do runs retrieve from the project's knowledge (forward the Space retrieval decision to execution-core) |

**IN:** OD19 container ruling; project picker; project-scoped chat start and resume; Space layer in Do runs; knowledge-mode badge in Ask and (admission-time) Do; project-scoped retrieval on every turn including Do.

**OUT (explicit):** server-synced folders with their own instruction layer (plan I21, closed as superseded); moving/copying an existing chat into a project (thread `origin` is set once and `(origin='space') = (space_id IS NOT NULL)` is a CHECK, `SC/migrations/0034_thread_origin.sql:12-13,44-47`; "copy to a Space" is a separate OD19 sub-ruling); non-owner append / multi-author project chat (Space session F-t1-001; owner-only `Mutate` check today); a Control `project` Space kind (Space session F-t1-064); an in-context knowledge mode (Verevon has retrieval only, see D6); org and persona layers in Do (WS02 S6 owns them, B3 is wider than F-t3-013); removal of the localStorage organiser (kept for non-project grouping); Channel Plane.

## 2. Verified current state

**Container model [wt].**
- Chat sidebar projects are localStorage folders: `FP/src/features/chat/lib/chat-organization.ts:2-34` (key `verevon.chat-organization.v1:{org}:{user}`, cap 100 at :34); schema `FP/src/shared/actions/chat-organization.ts:1-27` states "browser-only organizer... never grant access". Mounted by `FP/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx` (263 lines).
- /chat has no Space entry by design: `FP/src/features/chat/components/use-chat-controller.ts:2274-2288` (comment: removed `?space_ref=`, "FEWER /chat entry points", BFF still honours `space_ref` in the body). `scopedThreadRefs` (303, 1052-1053, 2288, 2456, 2542) carries a server-reported ref for resume only.
- Chat list is `origin = 'chat'` by a stored fact (`SC/migrations/0034_thread_origin.sql:44-47`; `ListThreadsRequest.origin`/`space_id` filters exist, `MP/proto/model_plane/v1/sessions.proto:947-964`, SQL filter `SC/src/grpc.rs:5418`).
- Space already has what a project needs: instructions (`FP/src/features/spaces/components/SpacePage.tsx:626-729`, PATCH gated to editor/manager/owner `FPG/src/domains/spaces.rs:3720`), knowledge panel (:434), multi-thread room (`getSpaceThreads`, `FP/src/shared/api/spaces-client.ts:157`; `getSpaceThreadTranscript` :672; `listSpaces` :124; `createRoom` :945). The Space room composer sends through the same `streamChat` with `spaceRef` (`SpaceRoomComposer.tsx:411-413`).
- Space kinds in Control: `personal|room|project|case` (`UC/migrations/017_space_authority.up.sql:20`, `UC/internal/spaces/authority.go:19`). The BFF refuses to create `project`/`case` (`FPG/src/domains/spaces.rs:2865-2879`). Convex enforces exactly ONE personal Space per member (`AP/convex-core/convex/spaces.ts:154-170,245`). So multiple projects per user can only be rooms; `createRoom` starts creator-only (`spaces-client.ts:933-945`).

**Authored instructions [wt].**
- BFF stamps `org_instructions`/`space_instructions` on every stream, invoke and AG-UI body: `FPG/src/domains/spaces.rs:4291-4324` (strips client values at 4306-4307; Space layer only after `resolve_space_role` at 4318; def `resolve_space_role` 3728-3763 calls Control `GET /api/v1/internal/spaces/{ref}/membership`). Call order in the stream path verified: `FPG/src/domains/chat/streams.rs:55-90` (instructions before `inject_personal_thread_context`, which removes `space_ref`). Therefore Do (agentic) bodies already carry `space_instructions`.
- Gateway: `InvokeRequest.space_instructions` (`GW/src/http_routes.rs:6753`), composed in `authored_instructions_message` (`GW/src/sse.rs:6243-6296`; platform, org, Space, persona, agent persona; Space cap 4000 chars at `spaces.rs:3806`), used only on the inline path (`sse.rs:1747`). The agentic branch (`sse.rs:1381-1447`) passes `agentic_space_id` (taken from `space_context` or `space_append_context`, :1409-1416) and no instruction; `RunAgentRequest` has no instruction field (`MP/proto/model_plane/v1/execution.proto:76-211`, highest tag 17). WS02 designs the fix (`designs/WS02-run-context-cache.md` S4: `RunAgentRequest.run_context = 18`; S6: `AuthoredLayers` incl. Space, single `compose_authored_instructions`, flag `MODEL_GATEWAY_RUN_CONTEXT` default off; D3 keeps ADR-0003 order, no ledger reversal; D8 reserves tag 18).

**Retrieval and knowledge [wt] (gaps the audit did not list).**
- First message of a Space thread gets `space_context` incl. `retrieval_decision_token` from Control `/internal/spaces/retrieval-decision` (`FPG/src/domains/spaces.rs:239-297`; FORBIDDEN degrades to an empty token, :260-277). Gateway retrieval passes it as `x-space-decision` metadata (`GW/src/retrieval.rs:1029-1092`); `Some(empty)` suppresses grounding (fail-closed, :1119-1131); `None` means a legacy UNSCOPED call.
- **Gap A.** Follow-up turns (body has `thread_id`) take the append branch (`spaces.rs:99-190`) that returns `space_append_context` with NO retrieval token. The gateway reads the token from `req.space_context` only (`sse.rs:1014,1628,1650,2012`), so every follow-up turn of a Space thread retrieves org-wide (user-ACL) instead of Space-scoped. Same for Space-room replies. `ThreadSpaceContext.retrieval_decision_token` is already `#[serde(default)]` (`GW/src/session_flow.rs:141`).
- **Gap B.** Do runs: `knowledge_search` in execution-core uses the user bearer only, no Space decision (`EC/src/knowledge_tools.rs:105-155`; call sites `EC/src/runtime_loop/mod.rs:1048,1221`). A Space-scoped Do run searches the whole org's knowledge, not the project's.
- Gap C: Do admission for a Space-scoped run demands the sandbox-manager bearer (`EC/src/grpc.rs:1224-1235`) but the Space capability decision for sandbox leases requires `KindPersonal` (`UC/internal/spaces/space_capability_decision.go:138`, `personal_thread_decision.go:111-116`), so in a room-kind project `code_interpreter` refuses; Ask and non-sandbox Do tools are unaffected.
- The Space knowledge list is retrieval-engine `/v1/knowledge/space-sources` under the same Control `retrieval.read` decision (`spaces.rs:1639-1700`). Data Plane verifies the v2 decision (`apps/Data Plane v2/services/retrieval-engine-rs/src/space_scope.rs:8-9,26-29`: "a Space never widens a document ACL", cap 2000 documents).

**Receipt [wt/live].** `source_scope_applied` exists at HEAD: `GW/src/sse_events.rs:199` (fields `scope`, `excluded_tools`; family `citations` :359; name :406; payload :491), emitted only when `retrieval_scope != Auto` (`sse.rs:2199-2208`). The SPA accepts only `knowledge_only|web_only` (`FP/src/shared/api/chat-client.ts:1324-1334`, type :553-556), renders `SourceScopeNotice` (`FP/src/features/chat/components/ChatMessages.tsx:3299`), inspector `ChatPanels.tsx:616`, `chat-types.ts:441`. There is no knowledge-mode concept: a Space turn with scope `Auto` emits no receipt at all. `retrieval_notice` is [live]-only (`GW/src/sse_events.rs` live 368, `chat-client.ts` live 250) and means a reduced answer; do not reuse. Non-auto scope with `agentic` is refused (`sse.rs:904-907`).

**[live-only] files that must land first.**

| File | Live status | Why it matters here |
|---|---|---|
| `GW/src/sse.rs` (+883), `sse_events.rs` (+203), `retrieval_metadata.rs`, `tool_loop.rs` | M | S2/S3/S5 edit the same functions; live numbers: inline call 1855, agentic branch 1443-1515, receipt emit 2325, composer 6375. WS01 lands them first. |
| `GW/src/zdr_features.rs`, `GW/tests/zdr_feature_table.rs` | ?? | ZDR field table: `space_context` Honored (:92), `space_instructions` Honored (:139), `space_append_context` Inert (:89), `agentic`/`plan_mode` refused (:305-309). S2/S3 ZDR clauses depend on it. |
| `FP/src/shared/api/chat-client.ts` (+53), `ChatMessages.tsx` (+229), `ChatSidebarOrganizer.tsx` (+2, renders new `ChatWorkLinks.tsx` ??), `chat-sidebar-organizer.css`, `use-chat-controller.ts` (+4), `chat-types.ts` (+7) | M/?? | S3/S4 touch the same files; land first or rebase onto them. |

**Audit text that is stale or wrong.** (1) Card line numbers are live-tree numbers; at HEAD sse.rs cites drift (1855 -> 1747, 1443-1520 -> 1381-1447, 6375 -> 6243) and `retrieval_notice` is absent. (2) `spaces.rs:4281-4330` is `inject_authored_instructions`, not `resolve_space_role` (3728-3763). (3) The card's plane attribution (Application owns membership) conflicts with master matrix rules 9-10. (4) The card says "keep Space conversations in the Space timeline" is a deliberate design: true, but the code-enforced fact is only the default list filter (`origin='chat'`); a Space-scoped thread already exists as a normal session-core thread, so the reversal is smaller than the audit implies (D2). (5) Audit missed gaps A, B, C and the one-personal-Space invariant.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Can proceed without |
|---|---|---|---|---|---|
| D1 | **OD19: project container** | (a) Spaces (b) server-synced folders + project-instruction layer (plan I21 `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:273`) (c) keep per-browser | (a). (b) would create a second authored-instruction store and a fifth precedence layer against ADR-0003 and duplicate Space authority; the audit recommends it (audit :1685). Close I21 as superseded. Owner: product + Control lead. | S4 | S1, S2, S3, S5 |
| D2 | **Partial reversal of "Space conversations live in the Space timeline"** (use-chat-controller.ts:2274-2288, 0034 comment) | full reversal (project threads in the default list) / opt-in | Opt-in only. Keep `origin='space'` and the default `/chat` list untouched (no migration, no 0034 change). A project is chosen explicitly; its threads are listed in a Projects section through the Space threads route. The comment block is rewritten, a ledger entry records it. | S4 | S1-S3, S5 |
| D3 | **Which Spaces are projects in v1** | personal + rooms / + org room / + `project` kind | Personal Space + non-organization rooms (`is_organization_room === false`, lifecycle `active`). Org room excluded (a chat there is a post visible to the whole org). `project`/`case` kinds belong to Space session F-t1-064. Role floor checked on select via `getSpaceMembership` (viewer refused; the server re-checks anyway). | S4 | others |
| D4 | **Audience disclosure** | silent / confirm | A first send into a Space with more than one member shows "Visible to N members" and needs one confirm (per project, remembered per browser). Creator-only rooms: no prompt. | S4 | others |
| D5 | **Do inside a room-kind project** (gap C) | ship with sandbox tools refused + notice / hold Do in rooms | Ship; Do works for instructions, knowledge, governed non-sandbox tools; code_interpreter refuses with a typed notice. Full fix is the Space session's Control "shared-Space capability decision family" (overlap-space D8). Personal Space is unaffected. | S4 Do toggle | S1-S3, Ask in S4 |
| D6 | **Knowledge-mode vocabulary** | `in_context`/`retrieval` (Anthropic) / honest Verevon set | Verevon only retrieves. Closed enum: `retrieval` (decision present; N sources), `not_entitled` (Control `retrieval.read` declined, grounding suppressed), `not_searched` (turn scope excluded project knowledge). `in_context` is reserved, never emitted. | S3 | S1, S2 |
| D7 | **Follow-up retrieval becomes Space-scoped** (S2 behaviour change: narrower than today) | scope / leave | Scope. Fail-closed on FORBIDDEN (empty token suppresses grounding, as the first turn does). Needs Space session and Control acknowledgement because the room composer shares the path. | S2 merge | S1, S3 design |
| D8 | **Do-run retrieval decision transport and lifetime** | metadata header (like `x-sandbox-authorization`, `GW/src/sse.rs:8336-8345`) / proto field | Metadata header `x-space-retrieval-authorization`, no proto tag (avoids the contested `RunAgentRequest` tags, WS02 D8). Decision `expires_at` exists (`UC/internal/spaces/decision.go:92,219`); the TTL value and whether a multi-minute Do run outlives it are NOT determined. If it can expire, execution-core must degrade `knowledge_search` to a typed "project knowledge unavailable" (never fall back to org-wide). | S5 | S1-S4 |
| D9 | Existing chats cannot enter a project | accept / build copy | Accept for v1. Offer "start a new chat in this project" only. | none | all |

**Ledger changes.** New entry in `MP/docs/decisions/ledger.md`: "Spaces are the chat project container; I21 server-synced folders superseded; opt-in project threads (origin stays `space`); Space layer = the project layer, ADR-0003 order unchanged". This is an explicit partial reversal of the 0034/controller design, not of a ledger entry (no ledger entry found for it; `grep` of `MP/docs/decisions/ledger.md` found nothing on origin or Space timeline). WS02 D3 already states no ADR-0003 change.

## 4. Slices (in build order)

Common commands: `cd "apps/Model Plane/rust" && cargo fmt --all -- --check && cargo test -p model-gateway --lib` (use `--lib` where a Windows target fails pre-existing); `cargo test -p model-gateway --test e2e_invoke_chain_test`; `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (filter `space_`); `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run <files> && pnpm typecheck && pnpm lint`; `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; Playwright `pnpm test:e2e <spec>`. Deploy evidence recipe: rebuild touched images (`bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build model-gateway execution-core`; BFF rebuild restarts the frontend container, batch BFF changes), healthy containers, then run the named flow on the stack.

### S0. Decision records [FINISH] (S, 1 d, docs only)
- Goal: make D1/D2 explicit before code.
- Files: `MP/docs/decisions/ledger.md` (entry above); `apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md` (addendum: a chat project is a Space; the Space layer is the project layer; no new layer; owner-only append means no collaborator token question yet, with the "whose token" rule written for later); `FP/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (mark OD19 project ruling and I21 superseded, line 273/507); rewrite the comment at `use-chat-controller.ts:2274-2288` in S4 (not here).
- Flag/ZDR/migration: none. Tests: none. DoD: three docs merged, linked from the PR of S4. Evidence: diff. Deps: D1, D2 ruling. Risks: none.

### S1. Space layer in Do runs [FINISH] (S, 2-3 d; starts after WS02 S4+S6)
- Goal: prove and pin that a Do/Plan run in a Space-scoped thread receives the Space instructions through WS02's single composer, with the same role gate, and that no second field or second composer is added.
- Files:
  - `GW/src/run_context.rs` (NEW by WS02 S6): WS18 adds only tests; `AuthoredLayers.space` must come from `req.space_instructions` (same source as `sse.rs:6259`). If WS02 omits it, add it there, do not fork.
  - `GW/src/sse.rs` agentic call site (`:1423-1445` HEAD): confirm `AuthoredLayers` built from `req` includes `space_instructions` (WS02 owns the edit).
  - `GW/tests/e2e_invoke_chain_test.rs`: NEW test `agentic_dispatch_carries_the_space_layer_after_org_and_before_persona` (seeded `space_instructions`, `space_context.space_id`, flag `instructions`): asserts `RunAgentRequest.run_context.authored_instructions` contains the Space block and `agentic_space_id` is set.
  - `FPG/src/domains/spaces.rs` (in-file tests near 9549-9600): NEW `space_instructions_are_stamped_on_a_plan_mode_turn_and_withheld_from_a_non_member` (plan-mode body with `features:["agentic"]`; 403/404 membership yields no Space layer; client-supplied `space_instructions` stripped).
  - `python/eval-lab-py/cases/run_context/` NEW `17_do_run_follows_space_instruction.yaml` (if harness can set `space_instructions`; else cover in e2e).
- Proto/API: none. Flag: rides `MODEL_GATEWAY_RUN_CONTEXT=instructions|full` (WS02, default off). Rollback: flag off.
- ZDR: agentic is refused under ZDR before composition (`sse.rs:976-994`; live `zdr_features.rs:305-306`), so no Space text reaches a persisted Do run under ZDR; run record stores enums only (WS02 S4). Golden case: reuse WS02 `a_zdr_turn_never_reaches_run_context_composition`; add Space row to `crates/mp-contracts/tests/run_context_posture.rs` (instructions allowed, history/memory refused).
- Tests: commands above. DoD: with flag `instructions`, a Do run in a project answers per the Space instruction; flag off is byte-identical; non-member gets no Space text.
- Evidence: rebuild model-gateway, execution-core, frontend BFF; set a Space instruction ("answer in numbered steps") in the Space page; start a Do-mode turn in that Space; capture gateway log (counts/statuses only), `GET` run record `metadata.run_context`, the answer, SPA screenshot.
- Deps: WS02 S4, S5, S6; WS01 (live `sse.rs`). Risks: WS02 slips (fallback: S1 stays unmerged; do not add `authored_instructions` as its own tag).

### S2. Follow-up turns keep Space retrieval authority [FINISH] (M, 3-4 d)
- Goal: every turn of a Space thread retrieves under the Control-signed Space decision; FORBIDDEN fails closed.
- Files:
  - `FPG/src/domains/spaces.rs` `inject_personal_thread_context` append branch (`:99-190`): after the `thread-append-decision`, call Control `POST /api/v1/internal/spaces/retrieval-decision` (same call and FORBIDDEN-to-empty-token semantics as `:239-297`, key `space-retrieval-append-...`) and put `retrieval_decision_token` into `space_append_context` (extract the shared call into one private fn `space_retrieval_token(...)` used by both branches). Keep `FORGED_SPACE_AUTHORITY_FIELDS` stripping (:34-46).
  - `GW/src/sse.rs` `:1014, 1628, 1650, 2012`: use `req.space_context.as_ref().or(req.space_append_context.as_ref())` through one helper `fn space_retrieval_decision(req) -> Option<&str>`; `None` only for a non-Space turn. Likewise `:1278` if it feeds retrieval (not opened; implementer re-greps `space_context` uses).
  - `GW/src/session_flow.rs:141` unchanged (field already defaulted); verify the append conversion to session-core ignores the extra field (`GW/src/http_routes.rs` append mapping near :7427 not opened).
  - Tests: `spaces.rs` NEW `a_scoped_append_turn_carries_the_retrieval_decision` and `a_forbidden_retrieval_decision_on_append_yields_an_empty_token_not_an_unscoped_call` (wiremock pattern at :5365-5481); `GW/src/sse.rs` NEW `a_space_append_turn_retrieves_with_the_space_decision`; `GW/src/retrieval.rs` existing `scoped_grounding_never_downgrades_to_an_unscoped_data_call` (:1139) must stay green.
- API/proto/migration: none. Flag: `GATEWAY_SPACE_APPEND_RETRIEVAL` (BFF env, default on in staging, off in prod until evidence; off = today). Rollback: set off.
- ZDR: a ZDR turn writes no thread; `space_append_context` is Inert (live `zdr_features.rs:89`), `space_context` Honored (:92). The token is a request-only credential, never persisted or logged. Golden: add `space_append_context` retrieval-token field to `GW/tests/zdr_feature_table.rs` (field already listed, so no new row; assert the token is not echoed).
- DoD: turn 2 of a project thread no longer surfaces org documents outside the Space; FORBIDDEN gives an ungrounded answer, not an org-wide one.
- Evidence: stack up; project with a Space-only document and an org-only document; ask turn 1 and turn 2 the same question; capture the Data Plane request metadata (`x-space-decision` present on both) and the citations (Space document only).
- Deps: D7 acknowledged; WS01 (live `sse.rs`). Cross-session: Space session shares `spaces.rs`. Risks: behaviour narrowing visible to the Space room (intended); Control retrieval-decision load doubles per follow-up (one extra Control call; idempotency key per turn).

### S3. Knowledge-mode receipt and badge [BUILD] (M, 4-5 d)
- Goal: a per-turn, truthful statement of how project knowledge was used, riding the existing family.
- Wire shape (additive, no proto): `source_scope_applied` gains `scope: "knowledge_only"|"web_only"|"auto"` (`auto` only when `knowledge` is present) and `knowledge: { mode: "retrieval"|"not_entitled"|"not_searched", sources?: u32 }`. Old SPAs ignore an `auto` frame (their parse accepts only the two non-auto scopes, `chat-client.ts:1324-1334`).
- Files:
  - `GW/src/knowledge_receipt.rs` (NEW; `pub mod` in `GW/src/lib.rs`): pure `fn knowledge_receipt(space_scoped: bool, token_empty: bool, scope: RetrievalScope, conversation_only: bool, grounding: Option<&Grounding>) -> Option<KnowledgeReceipt>` implementing D6 (derived only from facts the gateway holds; never invents `in_context`).
  - `GW/src/sse_events.rs:199,491` (live 238,556): add optional `knowledge` to `SourceScopeApplied` and its payload; keep family `citations`; update `docs/sse-event-taxonomy.md` via the generator.
  - `GW/src/sse.rs:2199-2208` (live 2325): emit when `retrieval_scope != Auto` OR `knowledge_receipt(...)` is `Some`; thin call into the new module.
  - `FP/src/shared/api/chat-client.ts:553-556, 1324-1334, 1554` (live 558-568): extend `ChatSourceScopeAppliedEvent`, parse `knowledge` defensively (unknown mode ignored).
  - `FP/src/features/chat/components/chat-types.ts:441`, `FP/src/shared/chat-nodes/derive.ts:149`, `FP/src/features/chat/components/use-chat-controller.ts:3094-3097`, `ChatPanels.tsx:616`: carry the new field.
  - `FP/src/features/chat/components/ChatMessages.tsx:3299` `SourceScopeNotice`: add a knowledge chip ("Prosjektkunnskap: 3 kilder" / "Project knowledge: 3 sources"; `not_entitled` states the org has not enabled it; `not_searched` states why). i18n via `i18n.tr`.
- Tests: `cargo test -p model-gateway --lib knowledge_receipt` (table over mode cases incl. `Some(empty)` token => `not_entitled`, WebOnly => `not_searched`, no Space => `None`); `sse_events` serialization test; `taxonomy --check`; vitest `use-chat-controller.retrieval-scope.test.ts` (add case) and NEW `FP/src/features/chat/components/SourceScopeNotice.test.tsx`; `chat-client.test.ts` new frame fixture; old-frame fixture unchanged (`chat-client.replay.test.ts:92`).
- Flag: none (additive frame; absent `knowledge` = today). ZDR: frame carries an enum and a count, no content; no persistence (envelope-only transcript). Under ZDR the frame is emitted on a Space turn with `space_context` Honored; add one assertion to `GW/tests/zdr_feature_table.rs`.
- DoD: Space turn (first and follow-up) shows the chip; non-Space turns unchanged; `retrieval_notice` untouched. Evidence: stack flow with an entitled Space (`retrieval`, N sources), an org without `retrieval_read_entitled` (`not_entitled`), and a web-only turn; capture raw SSE frames and screenshots.
- Deps: S2 (otherwise follow-up turns would report a mode that did not apply), WS01 live files. Risks: `ChatMessages.tsx` +229 uncommitted lines; the count of sources is from the `grounding` value, which assembly-supplied grounding may bypass (check `assembly_supplied_grounding`, `sse.rs:1636,1711`); report `sources` only when the retrieve call ran.

### S4. Project picker, composer chip, project thread list [BUILD] (L, 8-12 d)
- Goal: a user picks or creates a project in the chat sidebar, starts Ask or Do chats in it, and resumes them; authority is always re-resolved server-side.
- Files (FP unless stated):
  - `src/shared/actions/chat-project.ts` (NEW): zod `chatProjectSchema {spaceRef, name, kind:'personal'|'room', lifecycle}` and `isEligibleChatProject(space)` (D3). Action contract before UI: creating a project reuses registered `spaces.create_room` (`src/shared/actions/action-registry.ts:2112`); instruction edit reuses `spaces.update_space_instructions` (:2013). No new server action. Add contract test in `action-registry.test.ts` that the picker only references registered ids.
  - `src/features/chat/lib/chat-projects.ts` (NEW): `loadChatProjects()` = `listSpaces()` filtered by D3; `selectProject(ref)` = `getSpaceMembership` + role floor + audience count (`getSpaceRoster`, spaces-client.ts:598); project threads via `getSpaceThreads`, mapped to `ChatThreadHistoryItem` (carries `spaceRef`, cf. use-chat-controller.ts:1052).
  - `src/features/core/components/sidebar/ChatProjectsSection.tsx` (NEW) mounted above `ChatSidebarOrganizer` in `CoreSidebar.tsx`: project list, "New project" (dialog -> `createRoom`, treats non-`active` lifecycle as "setting up"), "Open project settings" (link to the Space page instructions/knowledge/members), expand to project threads. `ChatSidebarOrganizer.tsx` and `chat-sidebar-organizer.css` unchanged except heading text; localStorage folders stay (D1).
  - `src/features/chat/components/use-chat-controller.ts`: new `projectSpaceRef` signal (new-chat only); at `:2288` `requestedSpaceRef = scopedThreadRefs.get(id) ?? (isNewThread ? state.projectSpaceRef : undefined)`; project thread open uses `getSpaceThreadTranscript(spaceRef, threadId)` (spaces-client.ts:672) because whether the chat transcript route serves a Space-origin thread is NOT determined (`FPG/src/domains/chat.rs:28`); rewrite the comment at `:2274-2288` (D2). Keep the logic in a new small module `use-chat-project.ts` to limit the diff in this 3898-line file.
  - `src/features/chat/components/ChatPage.tsx` (live M +13): project chip in the composer header (name, audience: "Only you"/"N members", Ask/Do toggle unchanged); D4 confirm; D5 notice when kind is `room` and Do is on. Temporary chat (ZDR toggle) disables the chip with "not saved in a project" (a ZDR turn writes no thread).
  - `FPG/src/domains/spaces.rs` / capability catalogue: expose `chat_projects` capability from env `GATEWAY_CHAT_PROJECTS_ENABLED` (server-controlled features come from capabilities, not VITE flags; endpoint name not determined, follow the `DashboardComposer.tsx:1128-1141` pattern in flags digest). No new BFF route: all reads reuse `/api/v1/spaces*`.
- Proto/API/migration: none. Convex: none. Session-core: none (0034 untouched).
- Flag: `GATEWAY_CHAT_PROJECTS_ENABLED`, default off; staging on; rollback: off hides the section, existing Space threads and the Space page are unchanged.
- ZDR: project threads are durable Space threads; nothing new persisted by the SPA except an optional per-browser "audience confirmed" key (non-content, try/catch storage). Temporary chat refuses project selection client-side; the gateway already writes no thread for a ZDR turn. Golden case: extend `GW/tests/zdr_feature_table.rs` row for `space_context` with a ZDR + project turn (answer returned, no thread).
- Tests: NEW `chat-projects.test.ts`, `ChatProjectsSection.test.tsx`, `use-chat-project.test.ts` (new-thread uses project ref; resume uses stored ref; viewer refused; org room excluded; temporary disables); existing `ChatSidebarOrganizer.test.tsx` and `chat-organization.test.ts` stay green; Playwright NEW `tests/e2e/chat-projects.spec.ts` beside `space-shell.spec.ts`/`chat-workspace.spec.ts`: create project, set instruction on Space page, send Ask turn, send Do turn, reload, resume, check badge.
- DoD: end-to-end on the stack: project created, instruction followed in Ask and Do (needs S1 with WS02 flag on), knowledge scoped on turn 1 and turn 2 (S2), badge visible (S3), thread resumes after reload, viewer and non-member refused by the server, never a client-supplied `space_ref` trusted (BFF strips and re-decides).
- Evidence: rebuild frontend + gateway images; capture Playwright run, the Control calls (`membership`, `thread-decision`, `retrieval-decision`), and screenshots including the D4 confirm and the D5 notice.
- Deps: D1-D5; S2 (scoping), S3 (badge, can land after); WS12 (new files must satisfy the architecture/contract guardrails); `ChatSidebarOrganizer.tsx`/`ChatPage.tsx` live edits landed. Risks: shared-room audience surprise (mitigated by D4); one more list call per expand; 3898-line controller merge conflicts (WS09, WS14, WS16 also edit it).

### S5. Do runs retrieve from the project [BUILD] (L, 6-9 d; gated by D8)
- Goal: close gap B; make the Do-mode knowledge claim true and show the admission-time badge.
- Files:
  - `GW/src/sse.rs` `spawn_run_dispatch`/request builder (`:8302-8350` HEAD pattern, `x-sandbox-authorization`): forward the Space retrieval token as `x-space-retrieval-authorization` when `agentic_space_id` is non-empty and the token is non-empty; empty/forbidden: forward nothing and mark knowledge `not_entitled`. `agentic_run_stream` (`:9167`) gets the token as one more argument via WS02's params struct.
  - `EC/src/grpc.rs:1224-1262` (`run_agent`): read the header into `Option<String>`, pass to `runtime_loop::agent::run_agent`.
  - `EC/src/runtime_loop/agent.rs` and `EC/src/runtime_loop/mod.rs:1048,1221`: thread `space_retrieval: Option<String>` to `execute_knowledge_search`; `EC/src/knowledge_tools.rs:105-155` `build_request`/`search` insert metadata `x-space-decision`. For a Space-scoped run with no token, `knowledge_search` returns a typed "project knowledge unavailable" tool error, never an unscoped call.
  - `GW/src/sse.rs` agentic branch (`:1423-1445`): emit the S3 frame once after `connected` with `knowledge.mode` (`retrieval` or `not_entitled`; `sources` omitted).
  - `EC/tests/`: NEW `space_scoped_knowledge_search.rs` (header present => `x-space-decision` on the Retrieve request; absent in a Space run => error, no call; non-Space run unchanged).
- Proto: none (metadata only; D8). Flag: `AGENTIC_SPACE_RETRIEVAL_ENABLED` (gateway env, default off; off = today's unscoped Do search). Rollback: off.
- ZDR: agentic is refused under ZDR, so no Do retrieval token exists in ZDR; token is never logged or persisted (headers only); golden: assert refusal row unchanged. Run records store the admission enum only.
- Tests: `cargo test -p execution-core --lib knowledge_tools` and the new test; `cargo test -p model-gateway --test e2e_invoke_chain_test` new `agentic_dispatch_forwards_the_space_retrieval_decision_only_when_scoped` (pattern at `sse.rs:11839`); cross-service contract tests `EC/tests/cross_service_loop_contract.rs` must stay green (loops stay separate).
- DoD: a Do run in a project cites only the project's documents; a run outliving its decision degrades visibly. Evidence: rebuild model-gateway + execution-core; Do turn "summarise our project docs" with a Space-only and an org-only document; capture citations, run steps, headers (redacted), badge.
- Deps: D8 (TTL), S2, WS02 S6 (params struct, `agent.rs` seams), WS03 (owns checkpoint/resume sections of `agent.rs`), WS24 (action registry touches `agent.rs`). Risks: decision expiry mid-run; `agent.rs` conflict hot spot; Windows build caveat (Docker for the execution-core tests).

## 5. Interfaces

**PROVIDES**
| To | Contract |
|---|---|
| WS02 | Space-scoped fixtures and an acceptance test for `compose_authored_instructions` (S1); confirmation that no separate `authored_instructions` tag is needed. |
| WS15 | Admission-time `knowledge` receipt (S3/S5) as a candidate typed status cue ("project knowledge not enabled"); WS18 uses the existing `source_scope_applied` family, WS15 may relocate it into its typed run-event vocabulary. |
| WS17 | A consumer of `ListThreads(space_id)` / `getSpaceThreads` for the project thread list; WS17 paging/search should extend that route, WS18 adopts it. |
| WS07 | Requirement: Space-scoped chat threads now originate in /chat; non-owner append and memory containment (F-t1-012 slice 1 / F-t3-060 step 1) must land before any multi-author project chat. |
| WS09 | Project thread open path (`getSpaceThreadTranscript`) for the transcript-performance work. |
| Space session | S2's retrieval decision on append turns improves room replies automatically. |

**CONSUMES**
| From | Contract |
|---|---|
| WS02 | `RunAgentRequest.run_context = 18`, `AuthoredLayers`, `MODEL_GATEWAY_RUN_CONTEXT`, params struct for `agentic_run_stream` (S1, S5). |
| WS01 | Live ZDR/session-core wave and `zdr_features.rs` committed (S2, S3). |
| WS12 | CI guardrails for new FP files (contract/architecture checks). |
| Space session | Control shared-Space capability decision family (D5), non-owner append (F-t1-001), `project` kind (F-t1-064). |
| Control | `/internal/spaces/{ref}/membership`, `retrieval-decision`, `thread-append-decision` (all exist). |

**Shared files and edit order**
| File | Others | Rule |
|---|---|---|
| `GW/src/sse.rs` | WS01, WS02, WS03, WS04, WS27 | WS01 lands live changes; WS02 S6 next; WS18 S2/S3/S5 are thin calls into new modules (`knowledge_receipt.rs`) |
| `GW/src/sse_events.rs` | WS15, WS27 | WS18 only adds an optional field to an existing variant; no new variant |
| `FPG/src/domains/spaces.rs` | Space session | Space session edits membership/decision logic first; WS18 touches the append branch and one new private fn |
| `FP/.../use-chat-controller.ts`, `chat-client.ts`, `ChatMessages.tsx`, `ChatPage.tsx` | WS09, WS14, WS15, WS16 | live-only hunks land first; WS18 logic goes in new modules, diff in the controller limited to the lines named in S4 |
| `EC/.../agent.rs`, `grpc.rs` | WS02, WS03, WS05, WS24, WS25 | WS02 S7 and WS03 first; S5 last |

## 6. Cross-session interfaces

**(a) Sibling Verevon Space session (heavy overlap).**
- Shared pieces: Space page (instructions, knowledge, members), `spaces.rs`, Control Space decisions, `Space-scoped Do runs` (overlap-space rows 7, 17).
- Owner split: Space session owns Space identity, membership UX, `project`/`case` kinds, shared-Space capability decisions, non-owner append, audience-safe memory. WS18 owns the /chat entry (picker, chip, project thread list), the knowledge receipt and the retrieval scoping of Ask/Do turns.
- Assumes the Space session delivers: (1) the shared-Space capability decision (without it D5's notice stays); (2) non-owner append and memory containment before collaboration; (3) no fork of `compose_authored_instructions`. If the Space composer starts sending `agentic`, it consumes S1/S5 unchanged.
- Space page "Knowledge" tab and the new badge share vocabulary; keep the labels from one i18n source.

**(b) Quarry v2 / browser session.** No shared piece. Project chats expose no browser grant; "Model proposes, Quarry executes" untouched. Assumption: none. Conflict watch only on `execution.proto`/`agent.rs`, which WS18 does not edit (metadata only).

## 7. Regression guards (audit section 8)

| Strength | Touch | Gate |
|---|---|---|
| Two loops kept separate | S5 adds a metadata argument to the Do loop only; no gateway dispatch code is imported into execution-core | `EC/tests/cross_service_loop_contract.rs`, `compaction_parity_contract.rs`; review rule: no `model-gateway` import |
| ZDR by construction | New request-only credentials; badge frame; picker disabled in temporary chat | `GW/tests/zdr_feature_table.rs` (live), `bash scripts/tests/zdr-contract-sync.test.sh`, `api/contracts/zdr-posture-matrix.json`; new assertions listed per slice |
| Typed ChatEvent taxonomy | `source_scope_applied` payload grows | `gen-sse-event-taxonomy.py --check`; old-frame fixture parses; BFF relay stays byte-verbatim (`streams.rs`) |
| Credential isolation | Space decision tokens stay in BFF-minted headers; never in the SPA or logs | BFF test strips forged fields (`FORGED_SPACE_AUTHORITY_FIELDS`); S2/S5 tests assert no token in logs/bodies |
| Research honesty | Receipt never invents `in_context`; counts only when retrieval ran | `knowledge_receipt` table test; existing `source_scope_applied` tests (`use-chat-controller.retrieval-scope.test.ts`) |
| Honest outcomes | `not_entitled`/unavailable degrade visibly, never fall back to org-wide | S2 and S5 negative tests |
| Multi-tenant runtime / session-core single writer | No SQL, no new writer; no cross-plane DB | no migrations in this stream |
| Capability gate | No new tool path; Do tools still go through the signed gate | existing `capability_policy` tests unchanged |

Also protected: ADR-0003 precedence (platform, org, Space, persona, agent persona) via existing `all_four_layers_compose_in_platform_org_space_agent_order` (`sse.rs:10784`); Space never widens an owner ACL (`space_scope.rs:8-9`).

## 8. Effort and risks

**Total:** about 24-34 engineer-days (S0 1, S1 2-3, S2 3-4, S3 4-5, S4 8-12, S5 6-9), audit "L" confirmed. **Critical path:** D1/D2 ruling -> S2 -> S3 -> S4 (S1 runs parallel once WS02 S6 lands; S5 after D8 and the WS02/WS03 `agent.rs` work). Wave W2; S2 and S0 can start in W1 because they need no other workstream.

| Risk | Mitigation |
|---|---|
| Follow-up retrieval narrowing surprises users or the Space room (S2) | Flag with off-switch; explicit D7 sign-off; fail-closed semantics identical to turn 1; staging evidence with Space-only vs org-only documents |
| Shared-room audience exposure from /chat (D3/D4) | Org room excluded, creator-only rooms by default, audience chip + first-send confirm, server re-decision on every turn |
| Do in room projects loses sandbox tools (gap C) | D5 visible notice; personal Space unaffected; fix tracked to Space session's Control decision family |
| Do retrieval token expiry mid-run (D8) | Typed degrade, never unscoped fallback; TTL to be measured before enabling S5 |
| Merge conflicts in live-modified, high-traffic files (`sse.rs`, `use-chat-controller.ts`, `ChatMessages.tsx`) | WS01 lands live hunks first; new modules; named-line-only diffs; rebase-before-merge rule in section 5 |
