# EP-8 - Scoped memory and personalisation (Wave 3 design)

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `SC` = `MPR/services/session-core`; `CC` = `MP/go/services/capability-core`; `FE` = `apps/Frontend Plane/verevonv3`; `BFF` = `FE/apps/gateway/src`; `FES` = `FE/src`; `UC` = `apps/Control Plane/user-core`; `CVX` = `apps/Application Plane/convex-core/convex`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` at HEAD `c18196af2` (branch `model-plane`) on 2026-11-02, unless marked `[INF]` (inference) or `[verify live]` (only a running stack can prove it). The audit cards (`C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`, F-t3-060 at line 1137, F-t3-061 at line 1149) are older than the tree; where they disagree, the code wins and the row says so.

Status: design only. No code, migration, proto or flag in this document exists yet.

## 1. Scope, items, owner

**Items (W3):**
- F-t3-060 Scoped memory for Spaces, Projects and named agents with isolation (Recommended, Partial, audit effort L).
- F-t3-061 Personalisation that tailors itself: a standing user profile in the cached prefix, tone presets, a personal first-run step and trustworthy settings (Recommended, Partial, audit effort L, was M).

**Owner:** Model Plane owns the memory index (`agent_memory` in session-core), the memory gate (`SC/src/memory_control.rs`), Dreaming, the profile store and prompt assembly (model-gateway). Control Plane owns per-user settings (timezone, language, tone preset, first-run state) in user-core and, later, the declassify decision for Space memory. Frontend Plane owns the settings and onboarding UI and the BFF injection. Application Plane (Convex) keeps the persona text it already holds; this epic does not move it. Data Plane is not touched: authored documents and retrieval stay there, and nothing here computes embeddings (CLAUDE.md "No independent embeddings").

**"Projects" are Spaces.** DEC-28 (plan 2.1, row DEC-28) makes Spaces the chat project container. So F-t3-060's "project scope" is the Space scope. No separate `project` scope kind is added.

| Slice | Tag | Item | What | Effort | Startable now |
|---|---|---|---|---|---|
| S1 | BUILD | 061 | Timezone and language in the prompt | S | yes |
| S2 | FINISH | 060 | Memory containment, completed: the gate refuses Space threads on every reader and writer (K3-5 only fixed the Dreaming batch loader) | S | yes |
| S3 | BUILD | 061 | Tone presets (closed enum, Control setting, persona layer) | S | yes (D6) |
| S4 | BUILD | 061 | Standing user profile: store, deterministic builder, editor | M | yes (after S2 merges) |
| S5 | BUILD | 061 | Profile in the cached prefix (Ask and Do) | S | after S4 |
| S6 | BUILD | 060 | Space memory scope: migration and writers, dark | M | no: D3, D5, D11 + D2-480 |
| S7 | BUILD | 060 | Space memory recall, Space-owner list/delete, erasure on Space deletion | M | after S6 |
| S8 | BUILD | 061 | Personal first-run step (name, role, language, timezone, tone) | S | after S1, S3 |
| S9 | BUILD | 061 | Settings search | S | yes |
| S10 | BUILD, decision-gated | 060 | Agent scope for named agents, dark | M | no: D12 (EP-6 / DEC-14) |

S1 is first because it is the cheapest change a person can see: "what is the date tomorrow" and "remind me at 9" are answered in their own time zone and language. S2 is the same size and has no dependency; it should run in parallel with S1 and must merge before any of S4-S7 and before the chat-projects switches go on (section 7).

**Explicitly OUT:**
- Dreaming exclusion of Space and `agent_run` threads from the batch extractor (K3-5). Already committed in Wave 2: `4488f874f`, predicate at `SC/src/dreaming.rs:1319`, test `space_thread_messages_never_reach_user_memory` at `SC/src/dreaming.rs:3053-3101`. S2 builds on it; it is not re-planned.
- LLM synthesis of the profile (a second paid-model pass in Dreaming). Deferred: W3 ships a deterministic profile (D7). It needs OD4 (DEC-30, still open) and an evaluation first. Backlog.
- Effective-privacy view and effective-configuration provenance (F-t3-068, EP-9).
- Persistent named agents, the agent hub and agent definitions (F-t3-019, EP-6 phases 2-3). S10 only prepares the memory key; EP-6 phase 3 consumes it.
- Learned skills scope and the `space` skill scope (DEC-30, overlap-space row 14; Space session and EP-11).
- Self-maintained team wiki (F-t3-094, W5), memory provenance redesign (already built: `MG/memory_provenance.rs`), fine-tuning.
- Fixes that belong to other streams but gate us: D2-480/D2-481/D2-483 (WS26-S4 memory-write bypasses), D2-461 (WS18-S2 follow-up without a Space decision). They are named as prerequisites, not designed here.
- Channel Plane (docs-only).
- W4/W5 members of this epic: none. EP-8 has only the two W3 items; the W5 items that touch it (F-t3-094 wiki, F-t3-095 cache levers) stay in the W5 backlog (plan 3.3.4).

## 2. Verified current state

### 2.1 Memory store and scope vocabulary

- `agent_memory` has a free-text `scope` column with a comment listing `run | thread | session | user | org | global` and no CHECK (`SC/migrations/0004_tasks_cron_hooks_skills_memory.sql:214-232`). Thread rows reuse `session_id` to hold the thread id. Uniqueness is `(org_id, scope, owner, key)` (`SC/migrations/0011_identity_scoping.sql:9-15`). Lineage columns `supersedes_id`, `is_latest` came in `0037_agent_memory_lineage.sql:66-74`. The audit card's citation is still right.
- Writers map topics to scopes: `USER` to `user`; `POLICY`/`AGENT`/`WORKSPACE` to `org`; everything else to `thread` (`SC/src/dreaming.rs:2265-2271`). Inline extraction makes `user` candidates from user turns and `thread` candidates from assistant turns (`SC/src/dreaming.rs:747-766`; scopes at 2065, 2082, 2095, 2126).
- capability-core's own scope enum is `run, thread, workspace, session, user, org, global` (`CC/internal/api/workplane_apis.go:215-222`). It cannot write `space` or `agent` today, and its owner read filter (`workplane_apis.go:210-212`) would not list them. This is the "third vocabulary" of overlap-space C5. It still writes `agent_memory` by direct SQL (`workplane_apis.go:526, 604, 630`), which is open P0 D2-480.
- Thread origins are closed: `chat, space, agent_run, support, system, mail_task` (`SC/migrations/0041_mail_task_thread_origin.sql:20-21`), and `(origin = 'space') = (space_id IS NOT NULL)` is enforced (`SC/migrations/0034_thread_origin.sql:44-47`). So `origin` alone identifies a Space thread; the K3-5 predicate checks both anyway.
- Space threads already carry `space_decision_ref`, `recipient_audience_ref`, `recipient_audience_revision`, `recipient_audience_hash` (seen in the K3-5 test insert, `SC/src/dreaming.rs:3062-3066`). Messages copy `space_id` and the audience columns at append (`SC/src/grpc.rs:3605-3612`). The audience predicate of overlap-space row 6 therefore has its inputs already.

### 2.2 The one memory gate

- `memory_control::decide` is the pure decision table (ZDR, then org, then user) (`SC/src/memory_control.rs:189-232`). `MemoryReason` is a closed set: `on, user_paused, user_off, thread_off, org_disabled, zdr, policy_unavailable` (`:116-139`). `MemoryAccess` is `Full | OrgOnly | None` (`:106-111`).
- `resolve_for_thread` folds the thread's `memory_mode` in (`:409-424`); `resolve_in_tx_for_thread` does the same under the user's row lock (`:430-445`). Both return early when `controls_enabled()` is false (the `MEMORY_CONTROLS_ENABLED` kill switch, default ON, open decision D1-528). `thread_mode` reads only `memory_mode` (`:384-403`).
- Callers of the thread gate: `SearchMemory` (`SC/src/memory_grpc.rs:385-397`), `IndexMemory` (`:506-512`, re-asked at `:538`), context assembly (`SC/src/grpc.rs:6321`), inline extraction on append (`SC/src/grpc.rs:3705-3720`).

### 2.3 Containment today: what K3-5 fixed and what it did not

- Fixed (Wave 2, `4488f874f`): the Dreaming batch loader excludes `t.space_id IS NOT NULL OR t.origin IN ('space','agent_run')` (`SC/src/dreaming.rs:1319`), with the ignored DB test (`:3053-3101`) that passed through the harness per the commit message.
- **Not fixed, verified gap 1 (write):** inline extraction on every append. `append_message_in_tx` reads `space_id` (`SC/src/grpc.rs:3437`) but extracts candidates whenever the thread is not conversation-only (`:3698-3700`), gated only by `resolve_in_tx_for_thread` (`:3705-3720`), which has no origin rule. A user turn "Jeg heter Ima" in a Space room becomes a `scope='user'` row in the speaker's personal memory (`extract_user_candidates`, `dreaming.rs:2050-2100`) and is mirrored to Letta (`grpc.rs:3755-3760`). The plan's K3-5 text names only Dreaming; this path is the same leak.
- **Not fixed, verified gap 2 (write):** `IndexMemory` (the `save_memory` tool and the user's "remember this") checks conversation-only and the thread gate only (`SC/src/memory_grpc.rs:495-512`), so a Space turn can write `user` or `org` rows.
- **Not fixed, verified gap 3 (read):** recall into Space threads. `load_agent_memory_context_rows` returns org, thread and the owner's `user` rows for any thread (`SC/src/dreaming.rs:1415-1460`); `SearchMemory` does the same plus Letta (`memory_grpc.rs:385-430`). On the Ask path the gateway prefetch runs for every non-trivial turn unless conversation-only or web-only (`MG/sse.rs:2724-2748`); there is no Space condition. So a room reply can quote the asking member's personal memory to every member of the room. On the Do path WS02 already skips memory for Space-scoped runs (`MG/run_context.rs:263-271`, test `space_scoped_run_skips_memory` at `:1132`), which shows the intended rule; the Ask path and session-core do not follow it.
- Learning: the skill-learning announcement is already limited to `origin = 'chat'` threads (`SC/src/learning_events.rs:331`). No change.
- The WS07 design promised a shared helper `memory_control::eligible_thread_origin` (`.plan-work/designs/WS07-memory-and-learning-governance.md:201`). It does not exist (grep of `SC/src`: none); K3-5 inlined the predicate. S2 creates the helper.
- Space deletion already erases `thread`-scope rows of the Space's threads with durable semantic receipts (`SC/src/grpc.rs:8668-8700`, tables from `0026`/`0027`). A new `space` scope would not be covered (S7).

### 2.4 Personalisation today

- Persona: text and on/off live in Convex `users.personaInstructions/personaEnabled` (`CVX/schema.ts:88-90`), read by `users:personaForGateway` (`CVX/users.ts:278-296`). The BFF fetches it on each turn and injects `persona_instructions`, discarding any client copy (`BFF/domains/spaces.rs:4432-4486`). model-gateway composes it as the "personal instructions" layer below org and Space (`MG/run_context.rs:146-180`); request field `persona_instructions` (`MG/http_routes.rs:8877`).
- Verbosity exists (`concise | balanced | detailed`, FE `FES/shared/api/chat-client.ts:145, 1612-1615`; MG `http_routes.rs:8771`, mapped at `MG/sse.rs:3003-3013`). No tone preset exists anywhere (grep `tone_preset|tonePreset`: none).
- Identity: `identity_context_message` names the org and the signed-in user (`MG/sse.rs:7959-7984`), filled by the BFF `with_identity_context` (`BFF/domains/chat/shared.rs:134-146`, called at `BFF/domains/chat/streams.rs:142`).
- Time: `temporal_awareness_message` states today's date in UTC only (`MG/sse.rs:7847-7857`). No timezone or locale reaches the model (grep of `MG/sse.rs` for `timezone`: only UTC conversions). The audit is right.
- Control already stores a profile `timezone` and a `language` settings category (`UC/internal/http/handlers.go:240-282, 837-882`; note line 882 returns `"timezone": langSettings["region"]`, a naming bug [INF], not used here). `AISettings` has an unused `personalizationEnabled` (default true) (`handlers.go:1405-1422`).
- The SPA language is client-side: `Locale = 'no' | 'en'`, default `no`, stored in `localStorage` key `verevon.locale` (`FES/shared/i18n/locales.ts:1-4`).
- Prompt tiers (WS02): `PromptContext` has slots for identity, verbosity, memory and others (`MG/prompt_context.rs:228-265`). Under the stable order, `identity` and `verbosity` are `Context` tier and `memory` is `Volatile` (`:366-409`). The stable order is behind `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER` (`:29`, default off). There is no profile slot. The audit's "per-turn memory block changes every turn" is now handled by tiers, but nothing stable about the user exists to cache.
- Do runs receive `RunContext {authored_instructions=1, history=2, memory_block=3, status=4}` (`MP/proto/model_plane/v1/execution.proto:333-343`); the comment at `:332` reserves 5 and up, and plan 4.3 assigns 5 (WS08) and 6 (WS20).
- Settings UI: `FES/features/settings/components/PersonaSection.tsx`, `MemorySection.tsx`, `LearningSection.tsx`. Onboarding (`FES/features/onboarding/*`) is the org workspace setup flow; there is no personal step.

### 2.5 Prerequisite queue status (`.plan-work/execution/queue.json`)

| Slice | Status | Open register rows that matter | Effect here |
|---|---|---|---|
| WS07-S1 memory gate | partial | D1-532 (six gate DB tests not run on Postgres), D1-528 (kill switch default ON, decision) | S2 extends the gate; S2 runs the DB harness for its own tests and re-runs D1-532's six. Can start. |
| WS07-S2 reset | done | D1-557 live proof open | S4 extends reset to the profile. Can start. |
| WS07-S3 routes and controls UI | partial | D1-630 live e2e open | S4 adds routes next to them. Can start. |
| WS07-S5 old paths closed | partial | D1-563/564/568 live proofs, D1-565 Go gen stale | No direct dependency. |
| WS07-S6 thread off | done | D1-571/572 (mode not exposed, toggle unbuilt) | none |
| WS02-S5/S6/S7 tiers, composer, Do seeding | done | live proofs are lead steps | S1/S3/S5 add slots. Can start. |
| WS02-S9 Anthropic tiered system | partial | D1-444 (Context tier churn cost), D1-450 staging proof | S5's cache benefit on Anthropic waits for it; S5 can build. |
| WS02-S10.code cache scope key | partial | lead edit open | S5 measurement on Azure waits. |
| WS18-S1..S5 Spaces as projects | partial | D2-461 P0 (follow-up turn carries no Space decision), D2-460 P0, D2-769 P0, D2-464/467 ZDR rows | S6/S7 need a Space decision on the turn; D2-461 must be fixed before S6's flag goes on. S2 does not depend on it (it reads thread columns). |
| WS18-S4 project picker | partial | ledger 2026-10-31 (`MP/docs/decisions/ledger.md:5558`) lists K3-5 as a blocker of the switch | S2 removes the remaining memory half of that blocker. |
| WS26-S4 one policy on every memory write | partial | **D2-480 P0**, **D2-481 P0**, D2-483 P1 | S6 must not go live while capability-core and the orchestrator can write `agent_memory` around the gate. S2 and S4 are unaffected (they narrow, never widen). |
| WS17-S5 recall_conversations | done | - | S2 checks that tool too (it reads threads, not `agent_memory`; see S2 tests). |

### 2.6 Stale audit claims

1. "Dreaming loads pending messages across all threads with no Space predicate (`dreaming.rs:1263-1285`)": stale. Fixed by K3-5 at `dreaming.rs:1319`.
2. The audit treats containment as one Dreaming fix. Code shows three more paths (inline append extraction, `IndexMemory`, recall). Not stale, incomplete.
3. "Projects are client-only localStorage": stale. WS18-S4 built server-backed project threads on Spaces (flag off).
4. "Anthropic caches one joined system string (`anthropic.rs:284-306`)": partly stale. WS02-S9 added tiered system blocks behind a switch (partial).
5. "The per-turn memory block changes every turn (`sse.rs:1655-1680`)": the line range moved (now `sse.rs:2719-2748`) and the block is now `Volatile` tier under the stable order; the claim about cache defeat holds only with the stable order off.
6. "Persona (1,500 characters) ... no presets, timezone": still true.

Count of stale claims: 3 (items 1, 3, 4).

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | What an `agent_run` thread may read | (a) nothing, like Space; (b) read the owner's memory, never write | **(b).** An `agent_run` thread is the owner's own run (scheduled or delegated); recalling the owner's memory into it does not leave the owner. Writing from it is what K3-5 forbids ("the run's record"). One new reason value `agent_run_scope` for refused writes | S2 | Yes: (b) is the narrower change; (a) is one line if the owner prefers it |
| D2 | What a Space thread may read before Space memory exists | (a) org rows only; (b) nothing from `agent_memory` | **(b).** Non-org rooms are in scope (DEC-28), so org rows can reach outside members. Reason `space_scope`, returned as `skip_reason` so the UI can say "memory is not used in shared Spaces". Space rows come back in S7 under their own rule | S2 | Yes |
| D3 | Scope key (overlap-space C5, plan 2.6 row 13) | (a) rename `scope` to `scope_kind` and add `scope_id`; (b) keep `scope` as the kind with a closed CHECK, add `scope_id`; (c) a side table | **(b).** No rename keeps every reader and capability-core working. Closed set: `run, thread, session, workspace, user, org, global, space, agent`, added `NOT VALID` then validated after a read-only check of existing values. `scope_id` = `space_id` for `space`, `<space_id>:<agent_id>` for `agent` (D12), NULL otherwise (old kinds keep `session_id`/`owner`). Same migration adds the audience predicate columns `audience_hash`, `audience_revision`, `source_label` (overlap-space row 6). capability-core keeps its enum and never gains `space`/`agent` (its writes go through session-core per WS26 D2-480). Needs Space session acknowledgement | S6, S7, S10 | No for S6+; S1-S5, S8, S9 proceed |
| D4 | Isolation and parent scopes | (a) Space memory isolated: no user/org rows in, no Space rows out; (b) parent composition (user rows into a personal Space) | **(a) only, in W3.** It is ChatGPT's project-only contract and needs no audience reasoning. A per-Space isolation flag is not needed while isolation is the only mode. Parent composition for single-member personal Spaces is a later slice once the Space session supplies "audience = owner only" | S7 | Yes |
| D5 | Audience change after a row was written | (a) recall any row of the Space; (b) recall only rows whose `audience_hash` equals the thread's current one; others counted as `withheld` | **(b).** A member added later does not see memories distilled while they were absent until Control signs a declassify decision (overlap-space section "Memory": explicit `withheld` state, Control declassify). Room history visibility for new members is the Space session's rule [INF]; memory must not be wider than it | S7 | No for S7 |
| D6 | Where the tone preset lives | (a) Control user-core `AISettings.tonePreset`; (b) Convex next to persona | **(a).** CLAUDE.md gives user settings to Control; flags-and-config 8.6 says per-user settings go into the user-core typed struct; the epic lists Control as owner. Cost: one more BFF read per turn, cached per user for 60 s on `AppState` (never `std::env`, see memory note on AppState). Persona stays in Convex (no migration of existing data) | S3, S8 | Yes for every other slice |
| D7 | How the profile is made | (a) deterministic: ranked accepted `user` rows, bounded; (b) LLM synthesis in Dreaming | **(a) in W3.** No new paid-model egress, no OD4 exposure, testable byte for byte, provenance per line is exact. (b) is backlog after an eval. The person can edit; an edited profile is kept as written until they choose "rebuild" | S4 | Yes |
| D8 | Profile under memory states, ZDR and waivers | per-state table | **Follow the turn's memory decision exactly.** Paused: listed and editable, not injected. Off / org disabled: hidden, not injected. Reset all memory: deleted. Thread off: not injected on that thread. Space threads: not injected (D2). ZDR: not injected, profile routes refuse writes. Waived run (DEC-32): not injected (counts as ZDR for new stores and new prompt content). One function decides both recall and profile | S4, S5 | Yes (defaults refuse) |
| D9 | Timezone and language source | (a) browser per turn; (b) Control profile; (c) both, browser wins | **(c), browser first.** The browser knows the current zone (travel); Control `timezone`/`language` is the fallback for Do runs started without a browser (schedules) [INF: schedule fallback is a later use]. The values are not authority, but the BFF validates them (IANA name `[A-Za-z_]+(/[A-Za-z0-9_+-]+){0,2}`, offset in [-840, 840] minutes, language in a closed set) and never stores them | S1 | Yes |
| D10 | First-run state | (a) user-core settings category `onboarding`; (b) Convex; (c) localStorage | **(a).** `{personalStep: 'pending'|'completed'|'skipped', version: 1, updatedAt}`. Versioned so a later step can re-ask once (DeepSeek-style ledger) | S8 | Yes |
| D11 | ZDR and waived handling of Space memory (card: "needs a decision") | refuse / persist like Ask | **Refuse.** No Space memory extraction, write or recall on a ZDR or waived turn, or for a Space whose ZDR applies once Space ZDR exists (today `d_space_zdr_not_applied_org_off` in `MP/api/contracts/zdr-posture-matrix.json` says Space ZDR is not applied). Background Space extraction skips threads with a waived run (same SQL as `dreaming.rs:1325-1330`) | S6, S7 | No for S6/S7 |
| D12 | Agent scope: whose memory, which authority | (a) per (Space, agent) binding; (b) per agent definition across Spaces | **(a), decided with EP-6/DEC-14.** Agent memory never carries authority: it is recalled as text with `source_label='agent_memory'`, never as instructions above the persona layer, and never grants a tool. Across-Space agent memory (b) would carry one room's content into another and is refused | S10 | No for S10 |
| D13 | Containment vs the `MEMORY_CONTROLS_ENABLED` kill switch | (a) inside the controls path; (b) always on | **(b).** Containment is a data-boundary rule, not a user control. The early return at `memory_control.rs:416-418` must not skip it (D1-528 stays the owner's call for the controls) | S2 | Yes |

Ledger entries this epic adds (newest first, `MP/docs/decisions/ledger.md`): "Space threads read and write no personal or org memory; agent-run threads read but do not write" (D1, D2, D13, S2); "One scope key: `scope` is the kind, `scope_id` the instance; Space memory is isolated" (D3, D4, S6); "The standing profile is deterministic and follows the memory decision of the turn" (D7, D8, S4). No ledger ruling is reversed. Grep of the ledger for `scope_kind`, `profile`, `tone`, `F-t3-060/061`, `EP-8` found none.

## 4. Slices

Commands used below (from the repo root unless stated):
- Rust: `cd "apps/Model Plane/rust" && cargo test -q -p session-core --lib <filter>`, `-p model-gateway --lib <filter>`, `-p mp-contracts`; `cargo fmt --all -- --check`; `cargo check --workspace --all-targets`. Two targets fail pre-existing on Windows; use `--lib` (memory note). Real-Postgres tests are `#[ignore]` with `DATABASE_URL` and run through the session-core DB harness (as in `c18196af2`, 1031 passed).
- Proto: in `apps/Model Plane/proto`: `buf lint`, `buf breaking --against proto/baseline.binpb`, `buf generate` (Go bindings under `MP/go/gen/model_plane/v1/` are checked in; regenerate with them).
- BFF: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets <filter>`.
- SPA: in `apps/Frontend Plane/verevonv3`: `pnpm exec vitest run <files>`, `pnpm lint`, `tsc6 -p tsconfig.app.json` (memory: `tsc -p tsconfig.json` checks nothing).
- Control: `cd "apps/Control Plane/user-core" && go test ./...` (and `make test-coverage`).
- ZDR contract: `bash scripts/tests/zdr-contract-sync.test.sh`.
- Live: `scripts/run-control-plane.sh`, `apps/Model Plane/scripts/compose.sh`, `recreate-with-live-env.sh` after a worktree build (memory notes); a second Vite server for the worktree.

### S1 - Timezone and language in the prompt [BUILD] (F-t3-061)

**Goal.** The model knows the person's local date, time, weekday and language. "Hva er datoen i morgen?" at 00:30 in Oslo answers with Oslo's date.

**Files.**
- `FES/shared/api/chat-client.ts` (body builder near `:1605-1620`): add `client_timezone` (`Intl.DateTimeFormat().resolvedOptions().timeZone`), `client_utc_offset_minutes` (`-new Date().getTimezoneOffset()`), `ui_locale` (the `Locale` from `FES/shared/i18n`). Sent on every turn; tiny, no PII beyond what the browser already exposes.
- `BFF/domains/chat/shared.rs`: new `with_locale_context(body)` that validates the three fields (D9), drops invalid ones, never adds missing ones; called in `streams.rs` next to `with_identity_context` (`:142`) and in the agentic and JSON entry points that call `with_identity_context` (grep the three call sites). When absent, BFF may fill from Control `GET /api/v1/settings/language` and the profile timezone only if a cached value exists (no new blocking call in S1) [INF: keep S1 browser-only if the cache does not exist].
- `MG/http_routes.rs` (`InvokeRequest`, near `:8771-8877`): `client_timezone: Option<String>`, `client_utc_offset_minutes: Option<i32>`, `ui_locale: Option<String>`, all `#[serde(default)]`.
- NEW `MG/locale_context.rs`: pure `local_date_notice(now_utc, offset, tz_name) -> String` and `language_line(locale) -> Option<String>`. Uses `chrono::FixedOffset` from the offset (no new crate; DST is right at send time, which is the only moment that matters for one turn).
- `MG/sse.rs`: `temporal_awareness_message` (`:7847`) takes the optional local context; with the switch on and a valid offset it says "Today's real date is 2026-11-02 (Monday) and the local time is 09:14 in Europe/Oslo (UTC+01:00)". The language line goes into the `identity` slot text (Context tier, stable per person): "The person's app language is Norwegian (Bokmål); answer in the language they write in, and use Norwegian date and number formats when you are unsure". Calendar slot (`calendar_context::reference_calendar`) uses the local date as "today" when present.
- Do path: `MG/run_context.rs` passes the same date line into the Do preamble the same way WS02-S6 carries the date [verify which field carries the date into `RunAgentRequest`; if none, the line is appended to `authored_instructions`' trailing platform note, which is Context tier].

**Contract/proto/migration.** None (JSON fields on the HTTP body only). Action contracts: none (no new user operation).

**Flag.** `MODEL_GATEWAY_USER_LOCALE_CONTEXT` (vocabulary B, default off, `pub const` in `locale_context.rs`, compose `:-0`). Off: the date notice is byte-identical to today (golden test). The BFF passes the fields regardless; MG ignores them when off.

**ZDR.** No store. The values go to the model in the prompt only. Allowed under ZDR (the ZDR direct stream builds the same date notice; [verify] that `zdr_direct_stream` calls `temporal_awareness_message`, and pass the local context there too).

**Tests.**
- MG unit: `local_date_notice_crosses_midnight_in_oslo`, `invalid_offset_falls_back_to_utc_notice`, `switch_off_notice_is_byte_identical`, `language_line_is_closed_set`, `identity_slot_stays_context_tier` (in `prompt_context.rs` tests). `cargo test -q -p model-gateway --lib locale_context`.
- BFF: `locale_fields_are_validated_and_never_invented`, `bogus_timezone_is_dropped`. `cargo test --manifest-path ... locale`.
- SPA: `chat-client.locale.test.ts` (fields present, offset sign). `pnpm exec vitest run src/shared/api/chat-client.locale.test.ts`.

**Live proof.** Rebuild model-gateway and BFF, switch on, set the browser to `Pacific/Auckland` (DevTools sensors), ask "what is today's date and weekday?" at a time when Auckland is a day ahead of UTC; screenshot plus the `MG` debug log of the rendered notice. Then switch on in dev compose. Evidence file `docs/plans/wave3/evidence/EP-8-S1.md`.

**Effort.** S. **Dependencies.** WS02-S5/S6 (done). **Risks.** Cache: the date line is Volatile already; the language line joins the Context tier and changes only when the person switches language. Over-eager language forcing: the line says "answer in the language they write in".

### S2 - Memory containment, completed [FINISH] (F-t3-060 step 1; finishes K3-5)

**Goal.** A Space thread reads and writes no personal or org memory, on every path. An `agent_run` thread writes none (D1, D2). This is the memory half of the blocker the ledger lists for the chat-projects switch (`ledger.md:5558`).

**Files.**
- `SC/src/memory_control.rs`:
  - NEW `pub(crate) fn thread_containment(origin: &str, has_space: bool) -> Containment` with `Containment::{None, SpaceScope, AgentRunScope}`. This is the helper the WS07 design promised (`eligible_thread_origin`); one place states the rule, and `dreaming.rs:1319` gets a source-scan test that its SQL predicate matches the helper's set.
  - `MemoryReason` gains `SpaceScope` (`"space_scope"`) and `AgentRunScope` (`"agent_run_scope"`). Wire strings are additive; `SearchMemoryResponse.skip_reason` and `MemoryControls.reason` already carry free strings (`memory.proto:95, 313`).
  - `thread_mode` becomes `thread_facts` reading `memory_mode, origin, space_id` in the same query (`:392-396`). `resolve_for_thread` and `resolve_in_tx_for_thread` apply containment **before** the `controls_enabled()` early return (D13): Space: `denied(SpaceScope)`; agent_run: `generation_allowed = false`, reads unchanged.
  - `list_visible` is unchanged (the person's list is theirs, not the thread's).
- `SC/src/grpc.rs`: no logic change needed at `:3698-3720` and `:6321` (they already call the thread gate); add a comment pointing to containment. Check `context assembly` `MemoryAccess::None` path returns no rows (`dreaming.rs:1422-1424`) and skips Letta (`grpc.rs:6359-6362`).
- `SC/src/memory_grpc.rs`: `IndexMemory` refusal text for the new reasons is the existing `memory_not_enabled:<reason>` (`memory_control.rs:181-184`); the model-facing tool result says "Memory is not saved in shared Spaces".
- `MG/sse.rs` (`:2724`): no gate logic in the gateway; it already passes `skip_reason` through `fetch_chat_memory_context`. Add the outcome label `space_scope` to `record_memory_prefetch_outcome`'s allowed set (`:6700-6710`) so metrics do not log "unlisted".
- `FES/features/chat/...` memory recall chip: map `space_scope` to "Personlig minne brukes ikke i delte Spaces" / "Personal memory is not used in shared Spaces" (find the component that renders `memory_recall` skip reasons; `FES/shared/i18n/errors.ts` holds the reason strings [verify]).

**Contract/proto/migration.** None. Two new reason strings documented in `memory.proto` comments at `:95` and `:313` (comment-only change, buf-safe).

**Flag.** None, like K3-5 (a fail-closed fix; rollback is a revert). Stated explicitly because the epic rule is "every new switch defaults off": this slice adds no switch.

**ZDR.** Narrows only. A ZDR decision keeps its own reason (ZDR check precedes containment in `decide`; containment is applied after `resolve`, and `apply`-style precedence keeps `zdr` when access is already `None`, same as `apply_thread_mode` `:224-230`).

**Tests** (session-core DB tests are `#[ignore]`, run through the harness):
- unit: `containment_table` (origin x space x controls on/off), `containment_ignores_the_controls_kill_switch`, `dreaming_predicate_matches_containment_helper` (source scan of `dreaming.rs:1319`).
- DB: `space_turn_creates_no_personal_memory_inline` (append "Jeg heter Ima." to a Space thread; zero `agent_memory` rows, zero `dream_runs`), `agent_run_turn_creates_no_memory_inline`, `index_memory_refuses_in_space_thread_with_space_scope`, `space_thread_context_assembly_has_no_user_or_org_rows`, `space_thread_search_memory_returns_skip_reason_space_scope`, `agent_run_thread_still_recalls_owner_memory`, `chat_thread_unchanged` (regression).
- Re-run the six D1-532 gate tests and `space_thread_messages_never_reach_user_memory`.
- MG: `space_scope_prefetch_outcome_is_listed`.
- Commands: `cargo test -q -p session-core --lib memory_control`, harness for the DB set, `cargo test -q -p model-gateway --lib memory_prefetch`.

**Live proof.** With chat projects still off: create a personal Space room via the existing Space UI, send "Jeg heter Ima og jobber med anbud" in the room, then in a normal chat ask "hva heter jeg?". Expect no recall from the room; `psql` count of `agent_memory` rows for the user before and after equal; the room reply's SSE `memory_recall` frame shows `skipped: space_scope`. Evidence `docs/plans/wave3/evidence/EP-8-S2.md`. Then record in the ledger that the memory half of the WS18-S4 switch blocker is closed (the switch itself stays the lead's call; D2-461 and the other WS18 P0 rows still block it).

**Effort.** S. **Dependencies.** None. **Risks.** A Space room reply loses personal recall the user may have liked; this is the intended rule until S7 (Space memory) and D4's later parent composition. `agent_run` threads used by WS20 scheduled results: confirm the scheduled result thread origin [verify live; D2-120 says a scheduled thread may keep `origin='chat'`, in which case its answers are still mined; that is WS20's row, noted here].

### S3 - Tone presets [BUILD] (F-t3-061)

**Goal.** The person picks a tone once (for example Professional, Friendly, Direct, Warm, Neutral) and every chat and Do run follows it. Separate from verbosity (length), which stays per composer.

**Files.**
- `UC/internal/http/handlers.go` (`AISettings` `:1405-1410`, defaults `:1418-1421`): add `TonePreset string \`json:"tonePreset"\`` default `"default"`; PATCH validates the closed set `default|professional|friendly|direct|warm|neutral`. Tests in `UC/internal/http/*_test.go` (`ai_settings_tone_preset_is_closed_set`, `default_is_default`).
- `BFF/domains/persona.rs` (or `settings.rs`, whichever owns `/api/v1/settings/ai` today [verify]): pass-through; and NEW `fetch_tone_preset(state, user)` with a 60 s per-user cache on `AppState`. `BFF/domains/spaces.rs::inject_persona_instructions` (`:4463-4486`) also sets `persona_tone` (removing any client copy first, same trust class as persona).
- `MG/http_routes.rs` `InvokeRequest`: `persona_tone: Option<String>`.
- `MG/run_context.rs`: `AuthoredLayers` gains `tone: Option<Tone>`; `compose_authored_instructions` (`:146-180`) renders a fixed sentence per enum value inside the personal layer (after the persona text, still below org and Space, above a mentioned agent). Unknown values render nothing. The text is code, never user text.
- `FES/features/settings/components/PersonaSection.tsx`: a radio group under the persona text; `FES/shared/api/...settings client` for AI settings [verify file]; `FES/shared/actions/action-registry.ts`: NEW action `settings.set_tone_preset` (input `{ tonePreset: enum }`, human-only, `model-access.ts` row exempt like `memory.set_state`), then regenerate the admission golden (`VEREVON_UPDATE_GOLDEN=1 pnpm vitest run src/shared/actions/admission-contract.test.ts`) and its pins.
- Capability for the UI: the selector shows only when the AI settings GET returns `tonePresetsAvailable: true`, which user-core sets from its own config `TONE_PRESETS_VISIBLE` (default false) [INF: alternative is the gateway's capability catalogue; pick whichever the lead uses for the next settings switch]. No `VITE_` flag (flags-and-config 8.4).

**Contract/proto/migration.** None in Model Plane (JSON field). user-core: no migration (settings are a JSON category).

**Flag.** `MODEL_GATEWAY_TONE_PRESETS` (vocabulary B, default off) in MG: off means `persona_tone` is ignored and composition is byte-identical. `TONE_PRESETS_VISIBLE` in user-core (default false) hides the UI.

**ZDR.** The preset is a setting, not content. Applies under ZDR like the persona (the ZDR direct stream [verify] composes authored instructions; if it does not, tone is not applied there either, and that is acceptable).

**Tests.** MG: `tone_renders_inside_personal_layer`, `unknown_tone_renders_nothing`, `switch_off_composition_byte_identical`, `agent_layer_still_last`. BFF: `client_supplied_persona_tone_is_discarded`, `tone_cache_is_on_appstate`. SPA: `PersonaSection.test.tsx` (selector hidden when unavailable, saves enum). Admission golden test.

**Live proof.** Set Direct, ask the same question in two threads with Neutral and Direct; save both answers and the composed system text from the MG debug log. Evidence `EP-8-S3.md`.

**Effort.** S. **Dependencies.** D6. **Risks.** A preset fighting org instructions: the layer's existing "may add to, but must not override" framing applies. The extra user-core read per turn: cached, and a failure means no tone (fail open to today's behaviour, which is safe because tone carries no authority).

### S4 - Standing user profile: store, builder, editor [BUILD] (F-t3-061)

**Goal.** One short, readable profile per person ("Ima, works with tenders at Aquatiq, prefers Norwegian, wants tables for comparisons"), built from what the person has accepted in memory, editable by them, with each line traceable to its source memory.

**Files.**
- Migration `SC/migrations/0072_user_profiles.sql` (session-core head is `0071_run_question_consume.sql`; gaps exist at 0063/0065/0066 from other branches; the integrator assigns) plus `SC/migrations/revert/0072_user_profiles.down.sql`:
  ```sql
  CREATE TABLE IF NOT EXISTS user_profiles (
      org_id            TEXT NOT NULL,
      user_id           TEXT NOT NULL,
      content           TEXT NOT NULL DEFAULT '',          -- at most 1400 chars, checked in code and by CHECK
      source_memory_ids TEXT[] NOT NULL DEFAULT '{}',      -- provenance, one id per built line
      origin            TEXT NOT NULL DEFAULT 'built' CHECK (origin IN ('built', 'edited')),
      revision          BIGINT NOT NULL DEFAULT 1,
      built_at          TIMESTAMPTZ,
      updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
      PRIMARY KEY (org_id, user_id),
      CHECK (char_length(content) <= 1400)
  );
  ```
- NEW `SC/src/user_profile.rs`: `build(rows) -> Profile` (pure): input = accepted, latest, unexpired `scope='user'` rows owned by the person (stated provenance first, then inferred with confidence >= 0.8), deduplicated by key, ordered by kind (`preference`, `fact`, `instruction`), one line each, cut at 1,400 chars on a line boundary. `rebuild(pool, org, user)` runs under the user's row lock (`memory_control::lock_user`), refuses when `generation_allowed` is false, and does not overwrite `origin='edited'` unless called with `force` (the person's "Rebuild from my memories" button).
- Triggered rebuild: at the end of a Dreaming cycle for each user whose `user` rows changed in the cycle (cheap: one SELECT per user, no model), and after `DeleteMemory`/`ResetMemory`. A deleted memory removes its line on the next rebuild; an edited profile shows "1 source deleted since you edited" instead of silently keeping it.
- `SC/src/memory_grpc.rs`: three RPCs (below). `ResetMemory` also deletes the profile row (WS07-S2 extension; test).
- `MP/proto/model_plane/v1/memory.proto`: append to `MemoryService` (after `ResetMemory` at `:43`): `rpc GetUserProfile(GetUserProfileRequest) returns (UserProfileResponse);`, `rpc SetUserProfile(SetUserProfileRequest) returns (UserProfileResponse);`, `rpc RebuildUserProfile(RebuildUserProfileRequest) returns (UserProfileResponse);`. New messages appended at file end (after `ResetMemoryResponse`, last field 4 at `:355`): `UserProfile {string content = 1; repeated string source_memory_ids = 2; string origin = 3; uint64 revision = 4; google.protobuf.Timestamp updated_at = 5; uint32 sources_deleted_since_edit = 6;}`, `GetUserProfileRequest {string org_id = 1; string user_id = 2;}`, `SetUserProfileRequest {string org_id = 1; string user_id = 2; string content = 3; uint64 expected_revision = 4;}` (stale revision refused, `failed_precondition profile_revision_stale`), `RebuildUserProfileRequest {string org_id = 1; string user_id = 2; bool force = 3;}`, `UserProfileResponse {UserProfile profile = 1; string skip_reason = 2;}`. All new messages, so no existing tag moves; comment each field (buf lint ratchet).
- `MG/http_routes.rs`: `GET/PUT /v1/memories/profile`, `POST /v1/memories/profile/rebuild` next to the controls routes (`:313-321`), user bearer only, org and user from the claims (same shape as WS07-S3).
- `BFF/domains/memory.rs`: proxy `/api/v1/memory/profile*`.
- `FES/shared/api/memory-client.ts`: `getProfile`, `saveProfile`, `rebuildProfile`. `FES/shared/actions/action-registry.ts`: `memory.profile.save`, `memory.profile.rebuild` (human-only; `model-access.ts` exempt rows); admission golden regenerated.
- `FES/features/settings/components/MemorySection.tsx`: a "Profile" card above the list: the text (editable textarea, 1,400 char counter), "Built from N memories" with each line linked to its memory row, "Rebuild from my memories", paused/off states per D8.
- Capability-core: no change; its readers never see `user_profiles` (new table, not `agent_memory`).

**Flag.** `SESSION_USER_PROFILE_ENABLED` (session-core, vocabulary B, default off): off = RPCs return `unimplemented`-style `failed_precondition profile_not_enabled`, Dreaming never rebuilds. The UI shows the card only when `GetUserProfile` does not return that code.

**ZDR.** New durable store of user-derived content. ZDR caller: `Get` returns empty with `skip_reason=zdr` (like `list_is_a_noop_not_an_error_for_a_zdr_caller`, `memory_grpc.rs:1355`), `Set`/`Rebuild` refused `zdr_feature_not_supported` feature `user_profile`. Waived: same as ZDR for writes (DEC-32). Deletion: reset and GDPR user erasure delete the row (add to `SC/src/gdpr.rs` user erasure path [verify the function]).

**Tests.** unit: `build_orders_stated_before_inferred`, `build_respects_1400_chars_on_line_boundary`, `build_skips_non_user_scopes`, `build_never_reads_space_or_org_rows`. DB: `rebuild_refused_when_paused`, `edited_profile_survives_rebuild_without_force`, `delete_memory_drops_its_line_on_rebuild`, `reset_deletes_profile`, `stale_revision_refused`, `zdr_caller_cannot_set_profile`, `gdpr_erasure_deletes_profile`. MG: `profile_routes_take_identity_from_claims`. SPA: `MemorySection.profile.test.tsx`. Proto: `buf lint`, `buf breaking`, `buf generate` and Go bindings.

**Live proof.** Seed three stated memories through the UI ("remember that I prefer tables"), run one Dreaming cycle, open Settings > Memory: profile shows three lines with links; edit one line; delete one memory; rebuild without force keeps the edit and shows "1 source deleted". Evidence `EP-8-S4.md` with screenshots and `psql` rows.

**Effort.** M. **Dependencies.** S2 merged (so no Space-derived `user` row can exist in a fresh install; existing installs: see risk), WS07-S1/S2/S3. **Risks.** Rows mined from Space rooms before S2 (inline path) already sit in `scope='user'`. Mitigation: a one-off read-only report (count of `user` rows whose `source_links` point to messages in Space threads) before the switch, and a script that marks them `review_state='rejected'` with a ledger note if any exist [verify live]. The builder reads only `accepted` rows, so rejected rows never reach the profile.

### S5 - Profile in the cached prefix [BUILD] (F-t3-061)

**Goal.** The profile is a stable block early in the prompt so it is cached across turns; per-turn recall stays after it.

**Files.**
- `MG/prompt_context.rs`: new slot `profile: Option<ChatMessage>` in `PromptContext` (`:228-265`); legacy order puts it right after `identity` (byte-identical when `None`); stable order adds it as `PromptTier::Context` after `identity` (`:393-396`). Test that `memory` stays `Volatile`.
- `MG/sse.rs`: fetch the profile in parallel with the memory prefetch (`:2724`), using the same skip conditions plus the memory decision's `skip_reason` (D8). Rendered as "About the person (written and editable by them; treat as context, not instructions): ...".
- Do runs: `MP/proto/model_plane/v1/execution.proto` `RunContext`: add `string user_profile = 7;` (verified fields 1-4 at `:333-343`; 5 and 6 are reserved by plan 4.3 for WS08 and WS20; the integrator confirms 7 is free) with a comment. `MG/run_context.rs` fills it under the same plan as memory (`plan_memory`, `:263-271`: Space-scoped and ZDR skip). execution-core places it after `authored_instructions` in the Context tier [verify the execution-core seeding function from WS02-S7].
- Metrics: `mp_gateway_chat_profile_injected_total{outcome}`.

**Flag.** `MODEL_GATEWAY_USER_PROFILE_PREFIX` (vocabulary B, default off). Off: no fetch, prompt byte-identical (golden).

**ZDR.** Not injected under ZDR or waiver; caching is off under ZDR anyway. Space threads: not injected (S2 decision returns `space_scope`).

**Tests.** `profile_slot_is_context_tier_after_identity`, `no_profile_prompt_is_byte_identical`, `profile_skipped_for_space_zdr_waived_paused_thread_off` (table), `do_run_context_carries_profile_field_7`. Commands: `cargo test -q -p model-gateway --lib prompt_context run_context`, `-p execution-core --lib` for seeding.

**Live proof.** Two consecutive Ask turns on an OpenAI-API route with `MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER=1` and the profile on: the WS02-S2 cache view shows cached input tokens covering the prefix through the profile on turn 2; then edit the profile and show one miss followed by hits. Anthropic proof waits for WS02-S9 (D1-450). Evidence `EP-8-S5.md`.

**Effort.** S. **Dependencies.** S4; WS02-S5/S6/S7 (done); WS02-S9 for the Anthropic benefit. **Risks.** D1-444: if the Context tier changes often, cache writes are paid every turn. The profile changes at most once per Dreaming cycle or edit; measure before the switch.

### S6 - Space memory scope: migration and writers, dark [BUILD] (F-t3-060 step 2)

**Goal.** Space rooms get their own memory: facts the room states ("we deliver to Bergen on Fridays") are kept for the room, never for a person.

**Files.**
- Migration `SC/migrations/0075_agent_memory_scope_key.sql` (+ revert): 
  ```sql
  ALTER TABLE agent_memory
      ADD COLUMN IF NOT EXISTS scope_id          TEXT,      -- space_id for scope 'space'; '<space_id>:<agent_id>' for 'agent'; NULL otherwise
      ADD COLUMN IF NOT EXISTS audience_hash     TEXT,      -- thread recipient_audience_hash at write
      ADD COLUMN IF NOT EXISTS audience_revision BIGINT,
      ADD COLUMN IF NOT EXISTS source_label      TEXT;      -- 'room_statement' | 'member_save' | 'agent_memory'
  ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_chk CHECK (
      scope IN ('run','thread','session','workspace','user','org','global','space','agent')) NOT VALID;
  ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_id_chk CHECK (
      (scope IN ('space','agent')) = (scope_id IS NOT NULL)) NOT VALID;
  CREATE INDEX IF NOT EXISTS agent_memory_scope_id_idx
      ON agent_memory (org_id, scope, scope_id, review_state, is_latest) WHERE scope_id IS NOT NULL;
  ```
  `VALIDATE CONSTRAINT` in a follow-up statement after a pre-check query (`SELECT DISTINCT scope FROM agent_memory`) is run on staging [verify live]; the design assumes no other values exist. The uniqueness index `(org_id, scope, owner, key)` stays; Space rows use `owner = ''` and the key is prefixed with the `scope_id`, so two Spaces never collide [INF: alternatively extend the unique index; the integrator picks, since it touches overlap-space row 14's skill index too].
- `SC/src/space_memory.rs` (NEW): `write_space_candidate(tx, thread, candidate)` used by:
  - inline append extraction (`SC/src/grpc.rs:3698`): for a Space thread with `SESSION_SPACE_MEMORY_ENABLED` on, user and assistant candidates are re-scoped to `space` (never `user`), stamped with the message's audience columns, `source_label='room_statement'`;
  - Dreaming: a second loader `load_pending_space_messages` with the inverse of the K3-5 predicate (`t.origin = 'space'`), the same waived-run exclusion (`dreaming.rs:1325-1330`) and org policy filter, writing only `scope='space'`. The paid LLM extractor (`DREAMING_LLM_EXTRACTION`, off in compose) is not used for Space rows in W3; only the deterministic extractor runs [INF: keeps Space content off a new processor until the owner rules].
  - `IndexMemory` in a Space thread: topic `SPACE` (new, `normalize_topic` and `scope_for_topic` `dreaming.rs:2250-2271`) maps to `space`; a model-origin write is allowed only to `space` scope in a Space thread (the WS26 write policy `write_policy::gate` runs first).
- Every Space write requires the thread row to have `space_decision_ref` and the message to carry the audience hash (server truth). A turn without a Space decision (D2-461 case) writes nothing.
- No Letta mirror for `space` rows in W3 (the semantic tier filters by equality only and is an external processor; card note). `index_detailed` is not called for Space scope.
- `SC/src/memory_control.rs`: containment from S2 stays for personal and org rows; a new `space_access(thread)` returns `Write` only when the switch is on, not ZDR, not waived, org memory allowed.

**Contract/proto.** `memory.proto` `MemoryEntry`: `string scope = 9; string scope_id = 10; string source_label = 11;` (verified last field `provenance = 8` at `:130`). `IndexMemoryRequest` unchanged (topic `SPACE` is a value). Additive, commented.

**Flag.** `SESSION_SPACE_MEMORY_ENABLED` (session-core, vocabulary B, default off). Off: S2 behaviour exactly (Space threads write nothing).

**ZDR.** D11: refuse. Background loader skips waived threads; inline writer checks the caller's retention; Space ZDR (when it exists) denies.

**Tests.** unit: `scope_for_topic_space`, `space_candidate_never_user_scope`. DB: `space_turn_writes_space_row_with_audience_stamp`, `space_turn_without_decision_writes_nothing`, `waived_space_thread_not_mined`, `switch_off_space_writes_nothing` (= S2 tests still green), `capability_core_scope_enum_cannot_write_space` (Go test in `CC/internal/api`), `migration_constraints_reject_unknown_scope`, `space_rows_not_mirrored_to_letta`. Commands: harness + `cd "MP/go/services/capability-core" && go test ./internal/api/...`.

**Live proof.** Switch on in dev, two members in a room (requires the Space session's non-owner append [verify live]; with owner only, one member), state a room fact, run a Dreaming cycle, `psql` shows one `scope='space'` row with the room's `scope_id` and audience hash, zero new `user` rows. Evidence `EP-8-S6.md`.

**Effort.** M (L if the unique-index change is needed). **Dependencies.** S2; D3, D5, D11 recorded; D2-480 and D2-481 closed (capability-core and orchestrator writes go through session-core) before the switch goes on; D2-461 fixed (follow-ups carry the decision) before the switch goes on. The code can be built dark before those close. **Risks.** Three vocabularies merging wrongly (epic risk 1): the CHECK closes session-core's set, capability-core's enum is left smaller on purpose, and the audience columns are provenance, not a third vocabulary.

### S7 - Space memory recall, owner list/delete, erasure [BUILD] (F-t3-060 step 2, finish)

**Goal.** A room's reply uses the room's memory and nothing else; the Space owner can see and delete it; deleting the Space deletes it.

**Files.**
- `SC/src/dreaming.rs` `load_agent_memory_context_rows` (`:1415-1460`) and `search_agent_memory`: for a Space thread with the switch on, return only `scope='space' AND scope_id = thread.space_id AND is_latest AND accepted AND audience_hash = thread.recipient_audience_hash`; count rows skipped for audience as `withheld` (D5). No user, org or thread rows (D4 isolation).
- `memory.proto` `SearchMemoryResponse`: `uint32 withheld = 5;` (verified last field `skip_reason = 4` at `:95`).
- Space owner list/delete: `ListMemoryRequest` gains `string space_id = 4;` (verified last field `limit = 3` at `:210`); session-core lists `space` rows only when the caller presents a Control Space decision that names the caller as owner or admin of that Space (the same decision header family WS18 uses, `x-space-retrieval-authorization` per DEC-28) [verify the decision carries a role]. `DeleteMemory` accepts a Space row under the same decision. Members without that role: read through recall only.
- `MG/http_routes.rs`, `BFF/domains/spaces.rs`: `GET /api/v1/spaces/:space_ref/memory`, `DELETE .../memory/:id` (the BFF resolves `space_ref` server-side, never a client `space_id`).
- `FES/features/spaces/...` Space settings: a "Space memory" tab listing rows with source message links, delete, and the `withheld` count explained [verify the Space settings component path].
- Erasure: `SC/src/grpc.rs` Space deletion (`:8668-8700`): also delete `agent_memory WHERE scope IN ('space','agent') AND scope_id LIKE space_id || '%'` in the same transaction (no semantic receipts needed: no Letta copy, S6). GDPR user erasure: delete Space rows whose `source_links` contain only the erased user's messages; rows with mixed sources drop the user's message id and keep the row [INF; owner/DPO to confirm, section 7].

**Flag.** Same `SESSION_SPACE_MEMORY_ENABLED` for recall; `GATEWAY_SPACE_MEMORY_UI` in BFF config (`AppState`, default false) exposes the routes.

**ZDR.** Recall refused on ZDR/waived turns (no rows, `skip_reason=zdr`). List/delete allowed for a ZDR owner (delete is always allowed, as WS07 rules for personal memory).

**Tests.** DB: `space_recall_returns_only_this_space`, `space_recall_never_returns_user_or_org_rows`, `audience_change_withholds_old_rows`, `other_space_rows_never_returned`, `non_owner_cannot_list_space_memory`, `space_deletion_erases_space_rows`, `gdpr_erasure_drops_user_sources`. BFF: `space_memory_route_resolves_space_server_side`. SPA: `SpaceMemoryTab.test.tsx`. e2e: `tests/e2e/space-memory.spec.ts` (not in CI today; run locally).

**Live proof.** Room A states a fact, Room B asks for it: no recall; Room A asks: recall with "from this Space" label. Add a member to A: the old row is withheld and the count shows. Delete Space A: rows gone (`psql`). Evidence `EP-8-S7.md`.

**Effort.** M. **Dependencies.** S6; the Space session's role-bearing decision. **Risks.** Over-withholding after every membership change makes Space memory look empty; the explicit `withheld` count and Control declassify (Space session) are the answer, not widening.

### S8 - Personal first-run step [BUILD] (F-t3-061)

**Goal.** The first time a person opens chat, a short skippable card asks for display name, role, language, time zone (prefilled from the browser) and tone. Answers go to their settings; role optionally becomes a stated memory.

**Files.**
- `UC/internal/http/handlers.go`: settings category `onboarding` with `{personalStep, version, updatedAt}` (D10), GET with defaults `pending`, PATCH closed values; tests.
- `FES/features/chat/components/PersonalSetupCard.tsx` (NEW), shown above the empty chat when `personalStep === 'pending'` and the capability flag is on; writes: user-core profile (name, timezone), language settings, AI settings `tonePreset` (S3), and when the person ticks "Remember my role", `memory.remember` with `origin=user` (existing WS07-S3 action) so it flows into the profile (S4).
- Action contracts: `settings.complete_personal_setup` and `settings.skip_personal_setup` in `action-registry.ts`; admission golden.
- Capability: AI settings GET returns `personalSetupAvailable` from user-core config `PERSONAL_SETUP_VISIBLE` (default false).

**Flag.** `PERSONAL_SETUP_VISIBLE` (user-core, default false).

**ZDR.** Settings are not model content. The optional role memory follows the memory gate (a ZDR caller cannot save it; the card hides that checkbox when memory is not usable).

**Tests.** Go: `onboarding_category_closed_values`. SPA: `PersonalSetupCard.test.tsx` (skip records `skipped`, never shows again; completed writes four settings; role checkbox hidden when memory is paused). Live: new user signs in, sees the card once, completes, the next turn's composed prompt shows the tone and local time (S1, S3).

**Effort.** S. **Dependencies.** S1, S3 (S4 optional). **Risks.** Another modal on first use; it is an inline, dismissible card, never blocking the composer.

### S9 - Settings search [BUILD] (F-t3-061)

**Goal.** A search box on the settings page that finds a setting by name or synonym ("tone", "minne", "tidssone") and jumps to it.

**Files.** `FES/features/settings/lib/settings-sections.ts` (add `keywords` per section in both locales), NEW `FES/features/settings/lib/settings-search.ts` (pure ranking), `SettingsPage.tsx` (input and result list, keyboard navigation). No server change.

**Flag.** None (pure FE, like WS14-S1; rollback by revert). **ZDR.** None. **Tests.** `settings-search.test.ts` (Norwegian and English synonyms, no match state), `SettingsPages.test.tsx` update. **Live proof.** Screenshot with "tone" and "tidssone" queries. **Effort.** S. **Dependencies.** none. **Risks.** Low. The effective-privacy view is F-t3-068 (EP-9), not this.

### S10 - Agent scope for named agents, dark [BUILD, decision-gated] (F-t3-060 step 4)

**Goal.** A Space agent bound to a room has a small memory of its own ("home store"), recalled only on turns addressed to that agent in that room.

**Files.** `SC/src/space_memory.rs`: `scope='agent'`, `scope_id = '<space_id>:<agent_id>'` where `agent_id` is the Convex binding subject the BFF verified (`inject_mentioned_space_agent_persona`, `BFF/domains/spaces.rs:4496-4530`), passed to session-core as a verified field [verify how the subject reaches session-core; likely a new `agent_subject_id` on the append context]. Recall inside the agent layer as quoted text with `source_label='agent_memory'` (D12). Writes only from the agent's own turns via `save_memory` topic `AGENT_SELF` [INF naming]. Erasure follows S7 (Space deletion) and binding revocation (`revoke_space_agent`).

**Flag.** `SESSION_AGENT_MEMORY_ENABLED` (default off). Not switched on in W3; EP-6 phase 3 turns it on with its own live proof.

**ZDR.** As S6/S7 (refuse). **Tests.** `agent_memory_recalled_only_when_agent_addressed`, `agent_memory_never_crosses_spaces`, `revoked_binding_erases_agent_memory`, `agent_memory_cannot_raise_authority` (the text lands below org and Space instructions and grants no tool). **Effort.** M. **Dependencies.** S6, S7, D12, EP-6 phase 2 definition store (DEC-14). **Risks.** Building before EP-6 settles the definition store risks a second key; hence dark and last.

## 5. ZDR clause

New stores and egress, and what each does under ZDR and waiver:

| Store / egress | Slice | ZDR turn | Waived turn (DEC-32) | Erasure |
|---|---|---|---|---|
| Local time/language in the prompt (no store) | S1 | sent (no persistence) | sent | n/a |
| Tone preset (user-core setting, not content) | S3 | applied | applied | user deletion in Control |
| `user_profiles` (session-core) | S4 | read: empty, `skip_reason=zdr`; write/rebuild refused `zdr_feature_not_supported` (`user_profile`) | write refused; never rebuilt from a waived thread (builder reads accepted rows only, and waived threads never produce rows) | ResetMemory, GDPR user erasure, org deletion |
| Profile in the prompt | S5 | not injected; prefix caching off | not injected | n/a |
| `agent_memory` `scope='space'` | S6, S7 | no write, no recall | no write, no recall; background loader skips waived threads | Space deletion, GDPR user erasure (sources), org deletion |
| `agent_memory` `scope='agent'` | S10 | no write, no recall | same | Space deletion, binding revocation |
| Onboarding state (user-core setting) | S8 | allowed | allowed | user deletion |
| Semantic mirror (Letta) for new scopes | S6 | none: Space and agent rows are never mirrored | none | n/a |

Refusal behaviour: refusals use the existing typed envelope `zdr_feature_not_supported` with a feature key. Reads that would be empty under ZDR return empty with a `skip_reason`, never an error (pattern `list_is_a_noop_not_an_error_for_a_zdr_caller`, `SC/src/memory_grpc.rs:1355`). Deletes are always allowed.

Contract changes:
- `MP/api/contracts/zdr-refusals.json` `features`: add `{"key": "user_profile", "label_en": "the standing profile"}` and `{"key": "space_memory", "label_en": "Space memory"}`; regenerate the `.sha256` and re-vendor the FE copy (`FES/shared/api/contracts/zdr-refusals.json`), then `bash scripts/tests/zdr-contract-sync.test.sh`.
- `MP/api/contracts/zdr-posture-matrix.json`: its `decision_rows` decide posture, not per-store behaviour, and no new posture input is added, so no posture row changes. The store cases go into the resolver's golden test `MPR/crates/mp-contracts/tests/zdr_posture_matrix.rs` [verify file name; the WS14 design cites `tests/zdr_posture_matrix.rs`] as named cases: `s_profile_not_injected_under_zdr`, `s_profile_write_refused_under_zdr`, `s_profile_not_injected_on_waived_run`, `s_space_memory_not_written_under_zdr`, `s_space_memory_not_recalled_under_zdr`, `s_space_memory_not_mined_from_waived_thread`, `s_space_thread_reads_no_personal_memory`, `s_local_time_allowed_under_zdr`. If the integrator prefers matrix rows, they become `d_`-prefixed rows with `inputs.space_zdr` and the same names [INF].

## 6. Proof plan

Order of switches (each flip only after its slice's live proof is in `docs/plans/wave3/evidence/`):

1. S2 merged and live-proved (no switch). Then the lead may consider `GATEWAY_CHAT_PROJECTS_ENABLED`/`VITE_CHAT_PROJECTS_ENABLED` against the other WS18 rows (D2-460, D2-461, D2-769 are still P0; this epic does not clear them).
2. `MODEL_GATEWAY_USER_LOCALE_CONTEXT=1` after the S1 Auckland proof.
3. `MODEL_GATEWAY_TONE_PRESETS=1` and `TONE_PRESETS_VISIBLE=true` after S3's paired-answer proof.
4. `SESSION_USER_PROFILE_ENABLED=1` after S4's proof and after the pre-S2 Space-row report (S4 risk) shows zero or the rows are rejected.
5. `MODEL_GATEWAY_USER_PROFILE_PREFIX=1` after S5's cache proof on an OpenAI-API route; Anthropic after WS02-S9's staging proof.
6. `PERSONAL_SETUP_VISIBLE=true` after S8's first-login proof.
7. `SESSION_SPACE_MEMORY_ENABLED=1` and `GATEWAY_SPACE_MEMORY_UI=true` only after D2-480, D2-481 and D2-461 are closed and S6/S7 proofs pass.
8. `SESSION_AGENT_MEMORY_ENABLED` stays off in W3.

Regression gates every slice runs: session-core DB harness full run (compare against `c18196af2`'s 1031 passed; diff failing sets, do not trust the exit code alone), `cargo test -q -p model-gateway --lib`, BFF all targets, `pnpm exec vitest run src/features/settings src/features/chat src/shared/actions`, `tsc6 -p tsconfig.app.json`, admission golden, ZDR contract sync, `buf lint` + `buf breaking` for S4-S7.

End-to-end evidence for the epic (after steps 1-6): one script `MP/scripts/verify-personalisation.sh` (mirrors `verify-hitl.sh`) that, against a live dev stack, (a) sends a Space turn and checks no personal row appears, (b) seeds three memories and checks the profile, (c) runs two Ask turns and prints cached input tokens, (d) checks the date line for a given offset. Written in S5, run by the lead.

## 7. Risks and owner decisions

**Owner (or lead) must decide:**
- D3 scope key, with the Space session's acknowledgement (overlap-space C5, plan 2.6 row 13). Blocks S6, S7, S10 only.
- D5 audience rule and the Control declassify path. Blocks S7 only.
- D6 tone preset in Control vs Convex. Blocks S3 (and S8's tone field) only.
- D11 ZDR/waiver for Space memory (recommended: refuse). Blocks S6/S7.
- D12 agent scope and its authority model, with EP-6/DEC-14. Blocks S10 only.
- GDPR rule for Space rows built from several members' messages (S7 erasure): delete row vs drop the source. Needs the DPO view [INF]. Blocks S7's erasure test only.
- D1 (`agent_run` threads read the owner's memory) and D13 (containment ignores the controls kill switch): recommended defaults are safe; the owner can tighten D1 with one line.

**Slices startable now without any decision:** S1, S2, S4 (after S2 merges), S5 (after S4), S9. S3 and S8 need only D6.

**Risks:**
1. Cross-Space leakage and vocabulary drift (epic risk 1). Mitigated by S2 (all paths through one gate), the closed CHECK (S6), capability-core kept unable to write the new kinds, no Letta mirror for new kinds.
2. Agent memory carrying authority (epic risk 2). Mitigated by `source_label`, placement below org and Space text, quoted as context, never parsed into tool grants; test `agent_memory_cannot_raise_authority`.
3. Profile built from Space content (epic risk 3). S2 closes the inline path; the builder reads only `scope='user'` accepted rows; the pre-switch report catches rows mined before S2.
4. Existing bypass writers (D2-480, D2-481, D2-483) can still write `user` and `org` rows around the gate, and those rows feed the profile. The profile builder reads `accepted` rows only, but D2-480 writes `accepted` by default (`0004` default `review_state='accepted'`). S4 can ship; the switch for S4 should wait for D2-480 or the builder should add `AND source_links <> '{}'` [INF] so only extracted or user-saved rows count.
5. Cache cost: a profile in the Context tier that changes often would pay cache writes every turn (D1-444). The deterministic builder changes only when memory changes; measure in S5.
6. Space follow-up turns without a decision (D2-461) mean Space memory would be written on first turns only; that is fail-closed (nothing written), not a leak.
7. The scheduled-result thread may still be mined if its origin is `chat` (D2-120, WS20). Not this epic's fix; named so the WS20 owner closes it before `SESSION_CORE_SCHEDULED_RESULT_PERSIST` goes on.
