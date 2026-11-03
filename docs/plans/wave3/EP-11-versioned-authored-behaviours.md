# EP-11 (Wave 3 part) - One versioned lifecycle for authored behaviours

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `CC` = `MP/go/services/capability-core`; `OC` = `MP/go/services/orchestrator-core`; `FE` = `apps/Frontend Plane/verevonv3`; `BFF` = `FE/apps/gateway/src`; `FES` = `FE/src`. Every `path:line` below was opened in worktree `epic-shaw-4d613b` (branch `model-plane`, HEAD `70e67c8d7`) on 2026-11-02. `[INF]` = inference, not read in code. `[verify live]` = only a running stack can prove it. The audit card (`C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md:1112-1123`) is older than the tree; where they differ, the code wins and the row is marked **stale**.

Sibling Wave 3 designs read: EP-6 (agents hub, versioned agent definitions in Convex), EP-8 (scoped memory, tone presets), and `00-integration-round-a.md` (reservations). EP-3 (artifact canvas and composer, owner of "who authors slash templates" per plan 3.3.2 EP-3) has no design file yet; section 3 D6 states what this design assumes from it.

## 1. Scope, items resolved, owner

**Item:** F-t3-058, one versioned lifecycle for authored behaviours: skills, slash templates and presets, with a `/learn` command and Save-as-skill (Recommended, Partial, audit effort XL). Audit dependencies: F-t3-014 (WS19 skill bundles), F-t3-007 (WS07 learning governance).

**Owner:**
- Model Plane owns the authored-behaviour rows and their versions. The rows stay in the one table that already holds skills, `agent_skills` in the `session_core` database (ledger section 4.5, `MP/docs/capability-ownership-matrix.md:122-123`: one registry; DEC-30). Two writers already exist and stay: session-core's RPCs (`SC/src/grpc.rs:7204`) and capability-core's registry SQL (`CC/internal/api/registry_apis.go:237-516`). capability-core connects to the same database (`MP/deploy/docker-compose.yml:1776`, `DATABASE_URL .../session_core`), so no cross-plane database access is added.
- model-gateway owns injection, the skill cache, version pins on the turn and the `/learn` route.
- capability-core owns the HTTP registry routes, bundles (`skill_bundles`, WS19-S6), the lint on its write paths, import and the learning reviewer (`CC/internal/learning`, `CC/internal/sessionreview`).
- Frontend Plane owns the Skills page, the library view, the composer `/` menu items, Save-as-skill, the BFF pass-through and the action contracts.
- Control Plane: no change in this design. It keeps identity, org roles and Space roles; S9 reads a Space role through an existing Control decision (D7).

| Slice | Tag | In | Startable now |
|---|---|---|---|
| S1 | [BUILD] | Save as skill from an answer (FE + BFF only), authoring guidance in the editor | yes (admins); members after D4 |
| S2 | [BUILD] | Immutable versions with digests, version shown on the Skills page, read-only history | yes |
| S3 | [BUILD] | Pin the versions a turn used on its run; recovery inside a run reads the pinned version; feedback names the version | after S2 |
| S4 | [BUILD] | One lint on every write path (session-core RPC, capability-core POST/PATCH, bundle SKILL.md) from one shared rule file; closes D2-509 | yes (report mode); enforce after D8 |
| S5 | [BUILD] | Draft, publish and restore; bundles kept per version | after S2; D2 |
| S6 | [BUILD] | Slash templates and presets as authored kinds in the same table and lifecycle | after S2, S4; D1, D6 |
| S7 | [BUILD] | `/learn`: the owner asks the reviewer to distil this conversation into a proposal | after S4; D5 |
| S8 | [BUILD] | Import a skill from a `SKILL.md` or zip file | after S4, S5; D2-379/D2-383 fixed; D9 |
| S9 | [BUILD] | Libraries: Space scope, org library view, `ext.skills` enforcement | after S6; D7 |
| S10 | [FINISH] | Flag flips, ledger entries, proof script, evidence | after the slices it flips |

**Explicitly OUT:**
- F-t3-059 plugins (W4 member of this epic): manifest loader, `plugin_packages` digest/source/kill-list columns, supply-chain pins, cross-vendor adapters, the org-curated catalogue, admin consent records in Control. Nothing here touches `plugin_packages` (`CC/migrations/0003_capabilities_registry.up.sql:240-259`).
- Import from a URL or a git repository (needs the EP-9 egress proxy and the F-t3-059 supply-chain rules). S8 is file upload only.
- Named agents, persona, tool allow-lists and autonomy caps (EP-6 S7-S10, Convex `agentDefinitionVersions`). Presets here are composer defaults only (D6).
- The curated preset action sets (`FES/shared/actions/preset-agents.ts`) and their admission rows: EP-6 S11.
- Typed composer tokens and the token wire format: EP-3 (F-t3-052). A template here inserts plain text.
- Tone presets: EP-8 S3 (a user-core setting, not an authored behaviour).
- A "project lock" that pins a whole Space to fixed skill versions (Manus-style). Deferred; D3.
- Teach mode (F-t3-081, W5), the self-maintained wiki (F-t3-094, W5), the LLM reviewer as a permission gate (F-t3-097, DEC-40).
- Per-user GDPR erasure as a general session-core path (register D1-643). S2 only makes the new rows erasable by owner; the user-erasure route itself is not built here.
- Auto-accept of learned skills (DEC-30: none until WS07-S10 evidence; nothing here changes that).

## 2. Verified current state

**The table and its keys.**
- `agent_skills` was created in `SC/migrations/0004_tasks_cron_hooks_skills_memory.sql:190-202` (id, org_id, name, description, content, three JSONB lists, enabled, timestamps). Later columns: `origin` with CHECK `('user','background_review')` (`0006_agent_skills_origin.sql`), `scope` CHECK `('org','user')`, `owner_user_id`, `shared_with` (`0031_agent_skills_ownership.sql`), `review_state` CHECK `('proposed','accepted','rejected')`, `source_run_id`, `source_thread_id`, `proposal_reason` (`0058_agent_skills_proposals.sql`).
- The unique key is `(org_id, scope, owner_user_id, name)` (`0058`, `agent_skills_scope_owner_name_uq`); the old `(org_id, name)` index is dropped. **Stale audit claim:** "agent_skills is unique on (org, name)".
- There is no version, digest, kind or draft column. Every content write changes the row in place.

**The revision log is not a version store.**
- `skill_revisions` (`SC/migrations/0064_skill_revisions.sql:32-52`) is written by trigger `agent_skills_revision_trg` (`:178-180`) for every insert, update and delete, from either writer, with the actor read from `mp.*` transaction settings (`:127-135`). It stores before/after snapshots (`:61-98`).
- It keeps only the newest 50 revisions per skill (`:172-173`, `DELETE ... WHERE revision <= next_revision - 50`) and an over-64 KB snapshot is stored as a `truncated` marker without the body (`:84-95`). Undo (`UndoLearningEntry`) restores a snapshot into the live row. So a revision can disappear and can be rewritten into the live row: it cannot be what a run pins. A separate immutable version row is needed (D2).

**Write paths.**
- session-core `upsert_agent_skill` (`SC/src/grpc.rs:7204-7400`): `origin` `""|"background_review"` is forced to a user-scoped, disabled, `proposed` row (`:7226-7236`); a human write (`origin = "user"`) keeps `enabled` (`:7237-7261`). It names the actor for the revision trigger (`:7278-7290`), resolves the memory/learning gate for learned writes (`:7291-7303`), runs the WS26 content policy `write_policy::gate_skill` (`:7319-7325`), then upserts with `ON CONFLICT (org_id, scope, owner_user_id, name)` (`:7339`, `:7361`). The human branch overwrites content in place (`:7361-7368`). **Partly stale audit claim:** "UpsertAgentSkill overwrites in place" is still true for human writes; learned writes only refresh a still-proposed row (`:7349-7350`).
- **Open P0 (register D2-509, WS26-S5):** the RPC trusts `origin = "user"` from any caller holding `session:skills:write`; a service caller can write an accepted, enabled org-wide skill (`:7237-7239` takes `scope` and `enabled` from the request when the caller has no user id). Today only the learning path calls the RPC and it sends `background_review` [INF from register text and `CC/internal/learning/learning.go:148` cited there]. S4 closes it.
- capability-core POST `/api/v1/skills` (`registry_apis.go:280-372`) inserts by SQL; org scope requires admin (`:306-309`); owner is bound to the verified caller (`:312-316`); `promoted_from` copies a shared, accepted user skill to an org skill (`:322-358`). PATCH (`:403-503`) edits description/content/enabled/shared_with in place, refuses enabling a non-accepted row in the statement itself (`:474-480`, 409 `proposal_requires_decision` at `:495-497`). DELETE (`:505-516`). All three run in `mutateSkill` (`:155`) so the revision carries the actor. Neither POST nor PATCH runs the content policy or any lint (register D2-511, open).
- BFF: `/api/v1/skills` routes (`BFF/domains/agent_actions.rs:40-44`) are gated by `can_author_skills` = org `owner`/`admin` (`:67-70`), for every scope (`:161-169`). A member cannot create even a private skill through the SPA, although capability-core would accept it.

**Learning (WS07) as built.**
- Learned skills are disabled, owner-scoped proposals (WS07-S7 done, `d1e53e998`). **Stale audit claim:** "the learning loop persists candidates enabled with no proposal step" (`skillsink/sink.go:41-62`).
- `LearningGovernanceService` exists with `DecideSkillProposal`, `UndoLearningEntry`, `ListLearningLedger` (`MP/proto/model_plane/v1/sessions.proto:303-316`; handlers `SC/src/grpc.rs:10530`, `:10620`, `:10775`). The inbox UI and actions `learning.accept_skill`, `learning.reject_skill`, `learning.undo` exist (`FES/shared/actions/action-registry.ts:2860-2884`).
- Learning candidate dedup uses `SkillCandidate.ContentHash()` = sha256 of normalized name + content (`CC/internal/learning/learning.go:86-92`). It is a dedup key, not a version digest (it ignores triggers and tool restrictions).
- Quarantine: orchestrator-core rates skills by `skill_id` (`OC/migrations/0001_feedback_ratings.up.sql:22-34`) and quarantines by flipping `enabled` through `SetAgentSkillEnabled` (`OC/cmd/activities/quarantine.go:42-50`), which cannot tell a quarantine from a human disable and so recovery is manual (`:47-50`). Ratings attach to the skill, not to a version.

**Injection and the cache (model-gateway).**
- `fetch_skill_context` (`MG/sse.rs:7133-7240`) re-pulls the org's enabled skills from session-core when the cache is stale (`:7150-7180`; TTL `SKILL_CACHE_TTL_SECONDS`, default 60, `MG/skills.rs:45-55`), resolves explicit `/` picks first (`:7186-7193`), then keyword matches (`:7196-7212`), then bundle footers (`:7214-7232`) inside the 8,000-char budget. Only `review_state` accepted (or empty) rows reach a matcher (`MG/skills.rs:366-369`).
- Gateway `Skill` carries `id, name, body, tags, source_path, min_score` (`MP/proto/model_plane/v1/gateway.proto:1027-1039`): no version, no digest. `agent_skill_to_skill` (`MG/skills.rs:342-361`) drops everything else.
- Which skills a turn used is held only in an in-process, TTL-bounded map keyed by request id (`MG/chat_turn_registry.rs:1-40`, record `:80-91`), lost on restart. Nothing durable records which skill text steered a run. Recovery inside a run (`reattach_skill`, `MG/tool_loop.rs:4173` lists skills again) reads the current row, so an edit mid-run changes what recovery returns.
- The run-context status seam exists: `RecordRunContext` merges `metadata.run_context` on the run row and refuses ZDR and waived callers (`SC/src/grpc.rs:7575-7621`); it is called for Do runs from `run_context::prepare` (`MG/run_context.rs:628-660`, call at `:742`) and not from the Ask chat path (only call site found by grep). `RunContextStatus` uses fields 1-3 (`MP/proto/model_plane/v1/runs.proto:372-379`).

**Bundles (WS19-S6/S7/S8/S9).**
- `skill_bundles` is unique on `(org_id, skill_id)` (`CC/migrations/0021_skill_bundles.up.sql`, `skill_bundles_org_skill_uq`); blobs are content-addressed per org (`skill_bundle_blobs` PK `(org_id, sha256)`). `Store.Put` deletes the previous bundle of the skill and garbage-collects blobs no bundle names (`CC/internal/skillbundle/store.go:69-130`, GC SQL `:183-194`). So a pinned version cannot read its old bundle after a re-upload unless retention changes (S5).
- The read RPCs take a `skill_id` only (`MP/proto/model_plane/v1/capabilities.proto:321-367`; `GetSkillBundleManifestRequest` uses field 1 only).
- cmd wiring is in the tree (`CC/cmd/main.go:292`, `:472`) and Go bindings contain the new types (`MP/go/gen/model_plane/v1/capabilities.pb.go`, 13 matches of `ListSkillBundlesRequest`). Register rows D2-385/D2-386 still read "open"; the code says they are done. The register needs a triage line, not new work.
- Open bundle risks that matter here: D2-379 and D2-383 (any org member can read every bundle body through the gRPC reads; HTTP GET is admin-only), D2-382 (deleting a skill leaves its bundle), D2-384 (ZDR posture of a bundle upload undecided).
- First-party Office skills are image-baked and overlaid at read time (`MG/builtin_skills.rs`, WS19-S9, `BUILTIN_OFFICE_SKILLS_ENABLED` off). These are the "vendor" tier of DEC-30.

**The unused versioned schema.** `skill_packages` and `skill_resources` (`CC/migrations/0003_capabilities_registry.up.sql:133-182`) have a `version` column and a `pinned_version`, but no Go or Rust code reads or writes them (grep over `MP` for `skill_packages` hits only migration files). The audit is right that it is unused. This design does not revive it (D1).

**Slash commands and presets in the SPA.**
- The composer `/` menu lists built-ins (`__file`, `__image`, `web_search`) plus skills, capabilities and connectors fetched from the backend (`FES/shared/api/chat-actions-client.ts:1-60`; menu built in `FES/features/dashboard/home/DashboardComposer.tsx:1705-1745`; the old dead `slashCommands` array is gone, `:509`). A picked skill becomes an explicit skill id on the turn (`MG/sse.rs:7183-7193`). There is no authored text template and no `/learn` (grep: none).
- Presets: four hard-coded entries in `FES/shared/actions/preset-agents.ts:1-60` (goal template + launch defaults, `planMode: true` fixed, `actionIds` empty), used by `FES/features/agents/components/AgentRunConsole.tsx:45`, `:1259`. Still true as the audit says.
- Answer actions: `MessageAction` "Copy" in `FES/features/chat/components/ChatMessages.tsx:807` and `:1192`. No Save-as-skill.
- Skills page: `FES/features/settings/components/SkillsSection.tsx` (386 lines; admin check `:38`, hides proposed/rejected rows `:59`, create `:85`, admin promotion `:105-118`).

**Org policy seam.** `ext.skills` and `ext.plugins` are reserved allow-list keys in `MP/api/contracts/org-agent-policy-schema.json:38-53` (`none | approved_only | any`, default `any`, status `reserved`). Nothing enforces them yet. WS25-S7 (policy document) is done.

**Prerequisite WS slices (queue status from `.plan-work/execution/queue.json`).**

| Slice | Status | What matters here | Can EP-11 start anyway? |
|---|---|---|---|
| WS07-S7 proposals | done | proposal shape, unique key | yes |
| WS07-S8 revisions, decide, undo | partial | D1-643 (no per-user erasure, open), D1-645 (writers without `mp.*` log as `unattributed`), D1-648/D1-649 (DB tests and 0064 on PG16; D1-646 says the four tests passed 2026-10-28) | yes. S2's version trigger sits beside the revision trigger and must not change it; S2 tests run with the same harness |
| WS07-S9 inbox UI | partial | reviewer fixes in shared files not applied (queue note) | S7 reuses the inbox; S7 waits for S9's live proof [verify live] |
| WS07-S10 eval and cost caps | done | caps that S7 reuses | yes |
| WS19-S6 bundle store | partial | D2-379, D2-383 (P1, read authz), D2-382, D2-384, D2-380 (PG tests not run) | S2-S4 yes; S5 bundle retention and S8 import wait for D2-379/D2-383 |
| WS19-S7 `skill_file` | partial | D2-556 (live DoD not run), D2-558 | S5's pinned-bundle read extends it; S5 can build, live proof waits |
| WS19-S9 builtin skills | partial | flag off | S3 pins builtin skills by their embedded hash; no wait |
| WS26-S5 skill content policy | done | D2-509 (P0 open), D2-511, D2-512 | S4 closes D2-509 and decides D2-511/D2-512 |
| WS25-S7 org policy | done | `ext.skills` reserved | S9 enforces it |
| WS25-S11 Customize hub | partial | Skills section is mounted there | S9 UI lands inside it |
| WS02-S4 run context | done | `RecordRunContext` seam | S3 uses it |
| WS05-S4 | done | tool class rows | no dependency |

**Migration and proto heads (verified).** session-core head is `0071_run_question_consume`; round A reserved 0072-0076 (`docs/plans/wave3/00-integration-round-a.md` section 1.1), so the next free is 0077. The `space` branch has its own `0071_message_post_kind`, `0072_space_event_log`, `0073_thread_space_kind`, `0085_messages_space_artifact_index` (`git ls-tree space`): a collision the integrator must already resolve; not EP-11's. capability-core head is `0030_tool_policy_expiry`, nothing reserved. orchestrator-core head is `0002_feedback_implicit_source`.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | One store for skills, templates and presets ("choose one owner and table first", audit) | (a) `agent_skills` gains `kind` (`skill`, `template`, `preset`) and `settings`; (b) one table per kind sharing a version table; (c) revive capability-core `skill_packages` | **(a).** One row shape gives one revision trigger (already there), one version trigger, one lint, one Skills/Library UI and one erasure statement. Old consumers stay safe because `ListAgentSkills` defaults to `kind = 'skill'` (new field, empty = skills only) and the capability-core list defaults to `?kind=skill`. (c) is a second registry in a different shape with TEXT versions and no owner columns; the ownership matrix forbids a second registry. Retire `skill_packages`/`skill_resources` by a ledger note (drop later, not here) | S6 (and S9 index) | Yes: S1-S5, S7 |
| D2 | Version semantics | when a version is cut; what the digest covers; immutability; delete | A version is cut when the **digest of an accepted row changes** (S2), and after S5 only on publish. The digest is `sha256` (hex) over the UTF-8 text of `jsonb_build_object(name, description, content, trigger_keywords, trigger_file_patterns, tool_restrictions, kind, settings, bundle_sha256)::text`, computed in SQL by the trigger so both writers are covered (jsonb text output is deterministic). Proposals have version 0 until accepted (accept = v1). Version rows are never updated; a delete is allowed only in erasure (org purge, owner erasure) or with the skill's own delete. Restore writes an old body as a **new** version, never re-points to the old one. Deleting a skill deletes its versions; runs keep only `(skill_id, version, digest)`, which then read as `version_gone` | S2, S3, S5 | No for S2 (owner/MPO must accept the semantics); default proposed here |
| D3 | Pin semantics | (a) runs record the versions they used; recovery inside a run reads the pinned version; new runs take the latest accepted/published; (b) also let a Space or thread lock a version | **(a) in W3.** (b) is the Manus "project lock"; it needs a lock store and UI and is not in the plan's W3 list. The pin is content-free (ids, numbers, hashes), so it is allowed wherever the run record is allowed | S3 | Yes |
| D4 | Who may author a private skill through the SPA | (a) keep BFF admin-only for every scope (today); (b) any member for `scope = user`, admins for `org`; (c) (b) but only when org policy `ext.skills` is `any` | **(c), with `any` as the default** (schema default, `org-agent-policy-schema.json:42`). capability-core already binds the owner to the verified caller (`registry_apis.go:312-316`) and checks owner-or-admin on PATCH/DELETE. A private skill only steers its author's turns. An org that wants control sets `approved_only` (S9 enforces it). Behind `SKILL_MEMBER_AUTHORING` (BFF) so nothing changes until the flip | S1 (members), S7, S8 | Yes: S1 ships for admins first |
| D5 | `/learn` and the review model route (DEC-30 OD4, still open) | (a) refuse until OD4 closes; (b) same rule as the background review: only when the org opted in (`learningReview = proposals`) and the run is not ZDR/waived; (c) always allowed because the user asked | **(b).** The user asking does not change which model reads the transcript; the org opt-in is the existing control (DEC-30 defaults review DISABLED). `/learn` is cheaper than the background loop to reason about: one thread, one user, a visible request, the same caps (`LEARNING_REVIEW_MAX_PER_USER_PER_DAY`) | S7 | Yes: everything except S7 |
| D6 | What a template and a preset are, and who owns them (EP-3, EP-6 overlap) | template: (a) plain text inserted into the composer, `{{input}}` = rest of the line; (b) a typed token sent to the server. preset: (a) composer defaults the user sees (mode, web, skills, template); (b) a named agent with persona/tools | **Template (a), preset (a).** A template is text the user sees before sending, so it carries no authority and needs no token wire (EP-3 keeps the token format; a template may later expand into tokens once EP-3 defines them). A preset is a saved set of visible composer controls; it can only select things the user could select by hand (`planMode` stays true for console presets, models still go through WS10 entitlement at send). Anything with persona, tool allow-list or autonomy is an EP-6 agent definition (Convex), not a preset. The four curated presets stay code and EP-6 S11 owns their action sets; authored presets list beside them. EP-3 and EP-6 designers must acknowledge this split | S6 | Yes: S1-S5, S7, S8 |
| D7 | Space library authority (overlap-space row 14: "one table, scope gains `space`") | (a) BFF checks the Space role via Control and forwards; capability-core trusts the BFF; (b) capability-core requires a Control-signed Space decision ref, as Space thread writes do; (c) no Space library in W3 | **(b).** capability-core must not trust a role the client or BFF asserts (CLAUDE.md: no forged scoping). Injection reads `space_id` from the session-core thread row (`origin = 'space'` iff `space_id` set, EP-8 section 2), never from the request. If the Space session cannot supply a decision kind for "Space skill write" in time, ship S9 without the Space half (org library view + `ext.skills`) | S9 Space half | Yes: S9 org half |
| D8 | Lint rule set and default | rules; `report` vs `enforce`; tool-restriction rule (D2-512); person's re-enable of learned text (D2-511) | Rules in section 4 S4, as one closed code list in a contract file. Ship `SKILL_LINT_MODE` off, then `report`, then `enforce` after one week of report counts show < 1% of existing rows would fail [verify live]. Adopt the D2-512 rule (one plain tool name) as `tool_restriction_invalid`. Lint runs on PATCH too, so a person's edit or re-enable of learned text is linted (answers D2-511 for lint; content policy stays WS26's) | S4 enforce | Yes (report) |
| D9 | ZDR posture of bundle upload and import (D2-384) | refuse for ZDR orgs; allow (config, not conversation) | **Allow; it is org configuration, not conversation content.** capability-core already refuses any non-read call from a ZDR principal (`CC/internal/authz/authz.go:81`), so a ZDR-token session cannot upload; a non-ZDR admin of a ZDR org can. Record as a ledger entry | S8 enable | Yes (switch stays off) |
| D10 | Where the version table lives and who reads it | session-core migration + RPC for the gateway; capability-core SQL for the HTTP routes (same database) | **As stated.** It follows the existing two-writer pattern; the trigger is the single writer of versions, so neither service writes the table directly | S2 | Yes |

Ledger entries this design adds (newest first, `MP/docs/decisions/ledger.md`, CRLF): "Authored behaviours are one table with a kind; skill_packages is retired" (D1); "Skill versions are immutable, cut by digest change, pinned on runs" (D2, D3); "A private skill may be authored by any member unless ext.skills says otherwise" (D4); "/learn follows the background review's opt-in" (D5); "Templates are text, presets are visible composer defaults; agents are EP-6" (D6). Nothing in the ledger is reversed. Section 4.5 of the ownership matrix ("capability-core owns the skills registry (`agent_skills`)") is restated, not changed: the table lives in the shared `session_core` database and capability-core remains the HTTP registry owner.

## 4. Slices

Test commands (repo): Rust from `MP/rust`: `cargo test -q -p session-core --all-targets`, `cargo test -q -p model-gateway --all-targets`, `cargo test -q -p mp-contracts --lib`; real-PG tests are `#[ignore]` and run through `bash "MP/scripts/tests/session-core-db-tests.sh"` (`--ignored --test-threads=1`). On Windows use `CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0` and `cargo clean -p` the touched crates first (memory: stale warm target). Go from each service: `go test ./... && go vet ./...` (capability-core integration tests need Docker, lead-only). Proto in `MP/proto`: `buf generate`, `buf lint` (ratchet), `buf breaking --against proto/baseline.binpb`. FE from `FE`: `pnpm exec vitest run <files>`, `pnpm lint`, `pnpm exec tsc -p tsconfig.app.json --noEmit` (memory: `tsconfig.json` checks nothing). BFF: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`. ZDR contracts: `bash scripts/tests/zdr-contract-sync.test.sh`.

Flag vocabulary: every new switch is "only the literal `true` enables" (the WS19/WS25 pattern), default off, declared in `MP/deploy/docker-compose.yml` (or the FE stack compose for BFF) with `:-false`.

### S1 - Save as skill from an answer [BUILD]

**Goal.** The cheapest real value: a person turns a good answer into a private skill in two clicks, with guidance that steers them to write rules, not transcripts. No schema change.

**Files.**
- `FES/shared/actions/action-registry.ts` (+ test): NEW action `skills.save_from_message` (ownerPlane `model`, risk `low`, human-only, `requiresApproval: false`; input `{ threadId, messageId, name, description, content, enabled }`). Contract before wiring (CLAUDE.md).
- `FES/features/chat/components/ChatMessages.tsx`: a `MessageAction` "Lagre som ferdighet / Save as skill" beside Copy (`:807`, `:1192`), shown only on assistant messages of persisted, non-ZDR, non-temporary threads (the composer already knows temporary/ZDR, `DashboardComposer.tsx:3343` copy).
- NEW `FES/features/skills/components/SaveAsSkillDialog.tsx` (+ test): prefilled name (first heading or first 60 chars), content = the answer text, a "Use in my chats" switch (default on), and fixed guidance text: "Write what to do next time, not what happened. Keep it short; long skills are cut to fit." (`i18n.tr` pairs).
- `FES/shared/api/skills-client.ts:69-92` `createSkill`: pass `scope: 'user'`.
- `BFF/domains/agent_actions.rs:67-70, 161-169`: with `SKILL_MEMBER_AUTHORING=true`, `create_skill` lets a non-admin through only when the body's `scope` is `user`; `update_skill`/`delete_skill` let a non-admin through (capability-core still enforces owner-or-admin, `registry_apis.go:470`, `:509`). Org scope stays admin-only. The org id is never taken from the body (unchanged).
- `FES/features/settings/components/SkillsSection.tsx`: members see "My skills" (their `scope = user` rows) with edit/delete when the BFF reports authoring allowed; same guidance text in the editor.

**Contract/proto/migration.** None.

**Flag.** `SKILL_MEMBER_AUTHORING` (BFF, default off). Off: the button shows for admins only and members get today's 403. Turned on in S10 after the two-user proof.

**Tests.** FE: `SaveAsSkillDialog.test.tsx` (`prefills_name_and_content`, `hides_on_temporary_chat`, `sends_user_scope`), `ChatMessages` test (`save_as_skill_absent_on_zdr_thread`), `action-registry.test.ts`. BFF: `member_can_create_user_skill_when_switch_on`, `member_cannot_create_org_skill`, `switch_off_keeps_admin_only`, `org_id_in_body_is_ignored`. Commands as above.

**Live proof.** Member A saves an answer as a skill; A's next matching turn injects it (SSE/skill log shows the id); member B's turn does not; an org-scope create from A returns 403. Evidence: screenshots, BFF access log lines, `psql` row with `scope='user'`, `owner_user_id=A`. [verify live]

**Effort.** S, 2-3 d. **Dependencies.** None for admins; D4 for members. **Risks.** More private skills before versions exist: harmless to others (private), and S2 backfills v1 for them. Large pasted answers: the 8,000-char budget truncates; S4 lint warns.

### S2 - Immutable versions with digests [BUILD]

**Goal.** Every accepted change to a skill gets a numbered, immutable version with a digest; the Skills page shows "v3" and a read-only history. This removes "an edit silently changes every run" as an invisible event: the change is now a visible new version.

**Files.**
- NEW `SC/migrations/0096_agent_skill_versions.sql` (head 0071; number assigned by round B) + `revert/0096_agent_skill_versions.down.sql`:
  - `agent_skills` ADD `version INTEGER NOT NULL DEFAULT 0`, `digest TEXT NOT NULL DEFAULT ''`, `bundle_sha256 TEXT NOT NULL DEFAULT ''`.
  - NEW `agent_skill_versions (org_id TEXT NOT NULL, skill_id TEXT NOT NULL, version INTEGER NOT NULL, digest TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'), body JSONB NOT NULL, bundle_sha256 TEXT NOT NULL DEFAULT '', scope TEXT NOT NULL, owner_user_id TEXT NOT NULL DEFAULT '', actor_kind TEXT NOT NULL, actor_id TEXT NOT NULL DEFAULT '', created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY (skill_id, version))`, index `(org_id, owner_user_id)`.
  - Function `agent_skill_digest(agent_skills)` (D2 field list; `kind`/`settings` read as `'skill'`/`'{}'` until S6 adds the columns, then the function is replaced in S6's migration, which changes no existing digest because the values are the defaults [verify in S6 test]).
  - BEFORE INSERT OR UPDATE trigger `agent_skills_version_bump_trg`: if `review_state = 'accepted'` and the new digest differs from `OLD.digest` (or `OLD.review_state <> 'accepted'`), set `NEW.digest`, `NEW.version = COALESCE(OLD.version, 0) + 1`; otherwise keep both. AFTER trigger `agent_skills_version_trg` inserts the version row when `NEW.version > COALESCE(OLD.version, 0)`, with actor from `mp.actor_kind`/`mp.actor_id` (same settings the revision trigger reads, `0064:127-131`).
  - BEFORE UPDATE OR DELETE trigger on `agent_skill_versions` raising `skill_version_immutable` unless `current_setting('mp.version_erasure', true) = 'on'`. The AFTER DELETE path on `agent_skills` sets that setting locally and deletes the skill's versions.
  - Backfill: `UPDATE agent_skills SET digest = '' WHERE review_state = 'accepted' AND version = 0` fires the trigger and gives every accepted row v1 with actor `system` (session-core migrations run once under sqlx's ledger; still guarded by `version = 0`).
- `SC/src/gdpr.rs:405-420`: org purge deletes `agent_skill_versions` (setting `mp.version_erasure`) after `agent_skills` and `skill_revisions`; the summary struct (`:159-160`) gains `agent_skill_versions`. Note for D1-643: the table carries `owner_user_id`, so the future owner erasure deletes by it.
- `MP/proto/model_plane/v1/sessions.proto`: `AgentSkill` add `int32 version = 19; string digest = 20;` (18 is the last, `:1079`), comment tag `EP-11 S2`. NEW service appended after `LearningGovernanceService`: `SkillVersionService { rpc ListAgentSkillVersions; rpc GetAgentSkillVersion; }` with messages `ListAgentSkillVersionsRequest {org_id=1, skill_id=2, limit=3, before_version=4}`, `AgentSkillVersion {skill_id=1, version=2, digest=3, name=4, description=5, content=6, trigger_keywords=7, trigger_file_patterns=8, tool_restrictions=9, bundle_sha256=10, actor_kind=11, actor_id=12, created_at=13}`, `ListAgentSkillVersionsResponse {versions=1}`, `GetAgentSkillVersionRequest {org_id=1, skill_id=2, version=3}`, `GetAgentSkillVersionResponse {version=1, gone=2}`. A separate service keeps `SessionCore` mocks source-compatible (the `LearningGovernanceService` precedent, `:300-303`).
- NEW `SC/src/skill_versions_grpc.rs` (handlers; visibility = the `list_agent_skills` rule; service callers may read for a named user like `for_user_id`); `SC/src/grpc.rs` and `main.rs` get only the registration line (round A hot-file rule). `list_agent_skills` (`:7688`) selects and returns `version`, `digest`.
- `CC/internal/api/registry_apis.go`: `skillRow` gains `Version`, `Digest`; list/get SELECTs (`:248-259`, `:382-391`) add the two columns; NEW `GET /api/v1/skills/{id}/versions` (same visibility predicate as `get`, `:388-391`), behind `SKILL_VERSIONS_API`. Bundle `Put`/`Delete` (`CC/internal/skillbundle/store.go:69`, `:148`) also `UPDATE agent_skills SET bundle_sha256 = $manifest` in the same transaction with `mp.*` set (actor = uploader), so a bundle change is a new version.
- BFF `agent_actions.rs`: GET `/api/v1/skills/{skill_id}/versions` pass-through. FES `skills-client.ts` `listSkillVersions`; `SkillsSection.tsx` shows `v{n}` and a "History" drawer (date, actor, short digest, text diff against the previous version). Action contract `skills.versions.list` (read).

**Flag.** `SKILL_VERSIONS_API` (capability-core, default off) for the route; the FE hides the drawer when the route answers 404. The triggers have no switch (like `skill_revisions`); rollback is the revert file together with the image (both writers keep working without the columns because they never name them).

**Tests.** Ignored PG (session-core): `version_increments_only_when_digest_changes`, `toggling_enabled_does_not_cut_a_version`, `proposal_has_no_version_until_accepted`, `accepting_a_proposal_cuts_v1`, `capability_core_patch_cuts_a_version_with_actor` (raw SQL with `mp.actor_kind='user'`), `bundle_sha256_change_cuts_a_version`, `versions_are_immutable`, `skill_delete_removes_its_versions`, `org_purge_removes_versions`, `backfill_gives_every_accepted_skill_v1`, `revision_trigger_still_records_every_mutation` (regression for 0064). Unit: `digest_field_list_is_pinned` (source-text test that the SQL function names exactly the D2 fields). Go: `TestSkillVersionsRouteVisibility` (owner, admin on shared, stranger 404), `TestBundlePutCutsVersion` (integration, lead). FE: `SkillsSection.versions.test.tsx`. `buf lint`, `buf breaking`.

**Live proof.** Edit an org skill twice through the Skills page; `psql` shows v1-v3 with digests; `UPDATE agent_skill_versions ...` fails with `skill_version_immutable`; the drawer lists three entries. Evidence: `psql` output, screenshot. [verify live]

**Effort.** M, 4-5 d. **Dependencies.** None (WS07-S8 trigger exists). **Risks.** Two triggers on one table: order is BEFORE (bump) then AFTER (revision, version), and the revision snapshot does not include `version`/`digest` (the snapshot function names its fields, `0064:65-83`), so revisions do not double-count. Undo (`UndoLearningEntry`) restoring old text cuts a new version: correct by D2. The `ON CONFLICT` upserts do not name the new columns, so the BEFORE trigger is the only writer of them.

### S3 - Pin the versions a run used [BUILD]

**Goal.** Each run records `(skill_id, version, digest)` for every skill it injected; recovery inside the run returns that version even if the skill changed mid-run; feedback names the version.

**Files.**
- `MP/proto/model_plane/v1/gateway.proto` `Skill` add `int32 version = 7; string digest = 8;` (6 is the last, `:1038`).
- `MP/proto/model_plane/v1/runs.proto`: NEW `message SkillPin { string skill_id = 1; int32 version = 2; string digest = 3; string source = 4; /* "matched" | "requested" | "builtin" */ }`; `RunContextStatus` add `repeated SkillPin skills = 4;` (1-3 used, `:372-379`).
- `MPR/crates/mp-contracts/src/run_context.rs:145` `status_json`: add `"skills": [{id, version, digest, source}]` (content-free).
- `MG/skills.rs:342-361` `agent_skill_to_skill` copies version/digest; `MG/builtin_skills.rs`: builtin skills get `version = 0`, `digest` = the embedded SKILL.md sha256 that `tests/builtin_skills_contract.rs` already compares [verify the hash helper name].
- `MG/sse.rs:7133-7240` `fetch_skill_context` returns pins in `SkillContext`; the Ask path (`:2939` call site) records them with one `run_context::record_status` call carrying a status with only `skills` set (the Ask run row exists, `chat_turn_registry` records `session_run.run_id` at `:2955-2962`). The Do path adds pins to the status it already records (`MG/run_context.rs:742`). Whether the Do loop injects skills itself in execution-core is not verified here [verify at slice start: `EC/runtime_loop/mod.rs:2007-2018` `reattach_skill`]; if it does, it pins through the same field.
- `MG/tool_loop.rs:4173` (recovery): if the turn has a pin for the skill and the cached digest differs, call `SkillVersionService.GetAgentSkillVersion` (S2) and recover that body; `gone` returns the existing "skill not available" text.
- `MG/chat_turn_registry.rs:80-91`: `skill_ids` become `skill_refs` (`id@version`), and the feedback envelope carries them (string form, so the feedback HTTP body shape stays `skill_ids: [..]` with an `@n` suffix [INF: check orchestrator-core accepts any string]).
- Part (b), optional, may be split to its own slice: `OC/migrations/0003_feedback_skill_version.up.sql/.down.sql` adds `skill_version INTEGER NOT NULL DEFAULT 0` to `feedback_ratings` and to its key; `OC/internal/feedback` parses `id@version`; `OC/internal/quality` evaluates evidence per `(skill_id, version)` and quarantines only when the **current** version is below the bar. This is the audit's "quarantine re-keyed by version": a fixed v4 is not punished for v3's ratings.

**Flag.** `MODEL_GATEWAY_SKILL_PINS` (model-gateway, default off). Off: no pins recorded, recovery reads the current row (today). Part (b) behind `FEEDBACK_SKILL_VERSIONS` (orchestrator-core, default off).

**Tests.** MG: `pins_recorded_for_matched_and_requested_skills`, `builtin_skill_pin_uses_embedded_hash`, `recovery_reads_pinned_version_after_mid_run_edit`, `recovery_reports_gone_version`, `switch_off_records_nothing`; `run_context` golden for `status_json` with skills; ZDR: `zdr_turn_records_no_pins` (structural: the ZDR stream returns before `fetch_skill_context`, D2-557). SC: `record_run_context_merges_skills_without_dropping_other_keys` (extend `:13578`). OC (b): `TestQuarantineUsesCurrentVersionEvidence`, `TestRatingKeyIncludesVersion`. `buf lint/breaking`.

**Live proof.** Turn 1 injects skill X v2; edit X to v3 while a Do run that matched v2 is still running; the run's `skill_file`/recovery returns v2 text; `psql` `runs.metadata->'run_context'->'skills'` shows `{id, 2, digest}`; a new turn pins v3. Evidence: `psql` rows, tool result capture. [verify live]

**Effort.** M, 4-5 d (+ M 3 d for part b). **Dependencies.** S2; WS02-S4 (done). **Risks.** One more best-effort RPC per Ask turn: it is fire-and-forget with the existing 2 s style timeout in `record_status`; a failure loses the pin, never the turn (log + counter `skill_pin_record_failed_total`). Ask-path bearer may not hold `session:write` for `RecordRunContext` [verify live]; fallback is to record from the run's completion path.

### S4 - One lint on every write path [BUILD]

**Goal.** Every skill write, from any writer, passes the same rules and returns the same codes. Closes the P0 origin hole.

**Files.**
- NEW `MP/api/contracts/authored-behaviour-lint.json` (+ `.sha256`, the contracts convention): rules with `code`, `field`, `severity` (`error`|`warn`), limits. Initial closed codes: `name_empty`, `name_too_long` (> 64), `name_charset` (control chars, leading/trailing space), `description_missing` (warn), `description_too_long` (> 300), `content_empty`, `content_too_long` (> 32,000, error), `content_exceeds_budget` (> 8,000 = `SKILL_CONTEXT_BUDGET_CHARS`, warn: it will be cut), `trigger_keyword_count` (> 20), `trigger_keyword_too_long` (> 40), `tool_restriction_invalid` (D2-512 rule: one plain tool name `[A-Za-z0-9_.-]+`), `transcript_shape` (3+ lines starting `User:`/`Assistant:`/`Bruker:`, warn: "lessons, not logs"), `frontmatter_name_mismatch` (bundle `SKILL.md` front matter `name` differs from the skill). S6 adds `settings_*` codes for presets.
- NEW `MP/api/contracts/authored-behaviour-lint-golden.json`: inputs with expected code lists, read by both implementations.
- NEW `MPR/crates/mp-contracts/src/skill_lint.rs` (pure, std-only) + tests over the golden file.
- `SC/src/grpc.rs` `upsert_agent_skill` (`:7304-7325`, beside `gate_skill`): call the lint; in `enforce` an error returns `skip_reason = "skill_lint:<code>"` (the closed-token style already used for `skill_policy:<rule>`, `sessions.proto:1023-1031`); in `report` count only. **D2-509 fix:** refuse `origin = "user"` from a caller without a verified user id with `skip_reason = "origin_requires_person"` (one check at `:7217-7223`).
- NEW `CC/internal/skilllint/lint.go` (+ `lint_test.go` over the same golden file); `registry_apis.go` POST (`:280`) and PATCH (`:403`) call it; `enforce` answers 422 `{error:{code:"skill_lint_failed", message, details:[{code, field, severity}]}}`, `report` adds `warnings` to the 2xx body. `CC/internal/skillbundle/validate.go`: front-matter name check against the skill.
- FES `SkillsSection.tsx`, `SaveAsSkillDialog.tsx`: show `details` inline next to the field; warnings as a soft note.

**Flag.** `SKILL_LINT_MODE` = `off` | `report` | `enforce` (session-core and capability-core, default `off`; unknown values = `off`). The D2-509 check has no switch: it is a bug fix (only the learning path calls the RPC and it sends `background_review`; S4's test proves no current caller is refused).

**Tests.** `cargo test -q -p mp-contracts --lib skill_lint`; SC: `lint_report_mode_counts_and_writes`, `lint_enforce_refuses_with_token`, `service_caller_cannot_write_origin_user` (ignored PG), `learning_path_still_writes_proposals`. Go: `TestLintGoldenParity`, `TestPostLintEnforce422`, `TestPatchLintsReenableOfLearnedText`, `TestBundleFrontmatterMismatch`. Cross-service: a Rust test and a Go test that both hash the golden file and compare against `.sha256`, so a rule change in one language without the other fails (CLAUDE.md cross-service pattern).

**Live proof.** `report` for a week on dev: metric `skill_lint_findings_total{code}` from both services; then `enforce`: a 40,000-char skill is refused with 422 and the same code from both paths. Evidence: metric scrape, curl transcripts. [verify live]

**Effort.** M, 4-6 d. **Dependencies.** D8 for `enforce` only. **Risks.** Existing org skills that fail `content_too_long` would be refused on their next edit under `enforce`: the report week measures it; the error message tells the author the limit.

### S5 - Draft, publish and restore; bundles kept per version [BUILD]

**Goal.** Editing an accepted skill no longer changes live behaviour until the author publishes. Old versions can be restored as a new version. A pinned version can still read its own bundle.

**Files.**
- NEW `SC/migrations/0097_agent_skill_drafts.sql` (+ revert): `agent_skills` ADD `draft JSONB` (nullable; same field set as the digest body), `draft_updated_at TIMESTAMPTZ`, `draft_by TEXT NOT NULL DEFAULT ''`. Draft edits do not touch the digest fields, so they cut no version; the revision trigger records them as `edited` (snapshot does not include `draft` [verify: snapshot function lists fields, `0064:65-83`; add `draft` presence as a boolean only, not the text, to keep revisions bounded]).
- `CC/internal/api/registry_apis.go`: with `SKILL_DRAFT_PUBLISH=true`, PATCH of content fields on an accepted row writes `draft`, not the live fields; NEW `POST /api/v1/skills/{id}/publish` (copies `draft` into the live fields and clears it in one statement; the S2 trigger cuts the version; `mp.skill_action` stays empty so the revision reads `edited`); NEW `POST /api/v1/skills/{id}/versions/{v}/restore` (writes that version's body into `draft`; publishing is a second, explicit click). Owner-or-admin as PATCH. Proposals are unchanged: accepting a proposal is publishing v1 (`DecideSkillProposal`).
- NEW `CC/migrations/0034_skill_bundle_retention.up.sql/.down.sql` (round B: 0031-0032 are on `mp-w2w3`, 0033 is EP-5): drop `skill_bundles_org_skill_uq`, add `state TEXT NOT NULL DEFAULT 'live' CHECK (state IN ('live','draft','superseded'))`, unique `(org_id, skill_id) WHERE state = 'live'` and unique `(org_id, skill_id, manifest_sha256)`. Idempotent (Go services re-run every up file at boot).
- `CC/internal/skillbundle/store.go`: `Put` marks the previous live bundle `superseded` instead of deleting it (`:79`), or writes `draft` in draft mode; GC (`:183-194`) keeps blobs of any bundle whose `manifest_sha256` appears in `agent_skill_versions.bundle_sha256` for that skill, and drops `superseded` bundles no version names. `Delete` of a skill removes all its bundles (closes D2-382 for new deletes).
- `MP/proto/model_plane/v1/capabilities.proto`: `GetSkillBundleManifestRequest` add `string manifest_sha256 = 2;` (empty = live); `ListSkillBundlesRequest` unchanged. `CC/internal/server/server.go` honours it.
- `MG/tool_loop.rs` `skill_file` arm and `EC/src/skill_mounts.rs`: read the pinned version's `bundle_sha256` when S3 pins exist.
- FES `SkillsSection.tsx`: "Draft" badge, "Publish" and "Discard draft", "Restore as draft" in the history drawer; actions `skills.draft.save`, `skills.publish`, `skills.version.restore` (contracts first).

**Flag.** `SKILL_DRAFT_PUBLISH` (capability-core, default off). Off: PATCH edits live as today (still versioned by S2). Bundle retention behind `SKILL_BUNDLE_RETENTION` (capability-core, default off; off = today's delete-and-GC).

**Tests.** Go: `TestPatchWritesDraftWhenOn`, `TestPublishCutsOneVersion`, `TestRestoreWritesDraftNotLive`, `TestPublishRequiresOwnerOrAdmin`, `TestBundleSupersededNotDeleted`, `TestGCKeepsBlobsOfPinnedVersions` (integration, lead), `TestSkillDeleteRemovesBundles`. SC ignored PG: `draft_edit_cuts_no_version`, `publish_from_capability_core_sql_cuts_version_with_actor`. MG: `skill_file_reads_pinned_bundle`. FE: drawer and badge tests.

**Live proof.** Edit a live skill: turns keep using v2; publish: next turn pins v3; restore v1 as draft, publish: v4 with v1's digest body (digest equal to v1's, version 4). Re-upload a bundle: a run pinned to the old version reads the old file through `skill_file`. Evidence: `psql`, tool results. [verify live]

**Effort.** M, 5-6 d (two services; still M because each half is small). **Dependencies.** S2; S3 for the pinned-bundle read; D2-379/D2-383 fixed before `SKILL_BUNDLE_RETENTION` is turned on (old bundles become readable longer). **Risks.** Storage growth from retained bundles: bounded by 8 MiB per bundle and by the GC rule; add `skill_bundle_retained_bytes` gauge.

### S6 - Slash templates and presets as authored kinds [BUILD]

**Goal.** People author `/` templates and run presets with the same lifecycle as skills (versions, drafts, lint, history), without them ever being injected as skills.

**Files.**
- NEW `SC/migrations/0098_authored_behaviour_kind.sql` (+ revert): `agent_skills` ADD `kind TEXT NOT NULL DEFAULT 'skill'` with CHECK `('skill','template','preset')` (`NOT VALID` then `VALIDATE`, the 0058 pattern), `settings JSONB NOT NULL DEFAULT '{}'`, `scope_id TEXT NOT NULL DEFAULT ''` (used by S9). Replace the unique index with `(org_id, kind, scope, scope_id, owner_user_id, name)` (weaker than today's for every existing row; create new, then drop `agent_skills_scope_owner_name_uq`). Replace `agent_skill_digest` to read the real `kind`/`settings` (equal to the S2 defaults for existing rows, so no digest changes; test proves it).
- `SC/src/grpc.rs` upserts (`:7339`, `:7361`): conflict target becomes the new index; update the source-text test (`:17196`). `list_agent_skills` (`:7688`): NEW `ListAgentSkillsRequest.kinds = 4` (repeated string; empty = `['skill']`); `AgentSkill` add `kind = 21`, `settings_json = 22`. Every existing consumer (`MG/sse.rs:7155`, `MG/grpc.rs:2038`, `MG/tool_loop.rs:4173`, `CC/internal/sessionreview/transcript.go` listing) sends no kinds and so sees only skills: test pins that.
- `CC/internal/api/registry_apis.go`: list/get accept `?kind=` (default `skill`); POST accepts `kind` and `settings` only when `AUTHORED_KINDS_ENABLED=true` (else 422 `kind_not_enabled`). Lint (S4) adds: `template_placeholder_invalid` (only `{{input}}` allowed), `preset_settings_invalid` (closed schema: `modeId` string, `browseWeb` bool, `planMode` must be `true` for console presets, `skillIds` <= 4 (`MAX_PICKED_SKILLS`, `skills-client.ts:47`), `templateId` optional), `preset_unknown_key`.
- FES `shared/api/chat-actions-client.ts:54-60`: `loadSpecializedActions` adds kind `template` items (from `GET /api/v1/skills?kind=template`); `DashboardComposer.tsx` `/` menu (`:1705-1745`): picking a template replaces the draft with the template text, `{{input}}` filled from the rest of the line; nothing is sent until the user sends. NEW `FES/features/skills/components/TemplateEditor.tsx`, `PresetEditor.tsx`. `AgentRunConsole.tsx:1259`: authored presets listed under the curated `presetAgents`; applying one sets the console's visible controls only. Actions `templates.*`, `presets.*` (contracts first).
- `MP/api/contracts/authored-behaviour-lint*.json`: new codes and golden cases.

**Flag.** `AUTHORED_KINDS_ENABLED` (capability-core, default off). The SPA learns availability from the list route (an empty result plus a `kinds_enabled: false` field), not a `VITE_` flag (flags-and-config 8.4).

**Tests.** SC ignored PG: `existing_digests_unchanged_by_kind_migration`, `template_never_listed_for_matching`, `same_name_skill_and_template_coexist`, `upsert_conflict_uses_new_index`. MG: `templates_never_injected_as_skills` (list call sends no kinds). Go: `TestKindFilterDefaultsToSkill`, `TestPresetSettingsClosedSchema`, `TestPresetPlanModeMustBeTrue`. FE: `DashboardComposer.templates.test.tsx` (`template_fills_input_placeholder`, `template_inserts_text_and_does_not_send`), `PresetEditor.test.tsx`, `AgentRunConsole.presets.test.tsx`.

**Live proof.** Create template `/ukesrapport` with `{{input}}`; type `/ukesrapport uke 45`; the draft shows the expanded text; send; the model sees ordinary user text (request capture or SSE). A preset sets mode and web search; a preset trying `planMode:false` is refused 422. Evidence: screenshots, curl. [verify live]

**Effort.** M, 5-6 d. **Dependencies.** S2 (versions cover the new kinds automatically), S4 (lint), D1, D6; EP-3 and EP-6 acknowledge D6. **Risks.** The index swap on a live table: `CREATE UNIQUE INDEX` without `CONCURRENTLY` locks writes briefly; the table is small (per-org skills) [verify live row count]. Composer hot file (`DashboardComposer.tsx`, 6,382 lines): one `Match` arm, no restructure.

### S7 - `/learn` [BUILD]

**Goal.** The owner asks Verevon to distil the current conversation into a skill proposal, with an optional note. The result arrives in the existing learning inbox; nothing activates without an accept click.

**Files.**
- FES `chat-actions-client.ts:20-43`: builtin `__learn` ("Lær av denne samtalen / Learn from this conversation"), hidden on ZDR/temporary threads and when the capability reports learning off. Action `learning.learn_from_thread` (input `{ threadId, note? }`, human-only).
- BFF `domains/learning.rs` (exists from WS07-S9): `POST /api/v1/learning/learn` pass-through, user bearer.
- `MG/http_routes.rs` (end of router, round A rule): `POST /v1/learning/learn` → capability-core with the user's capability bearer; 202 `{ data: { state: "queued" } }`; refusals as typed envelopes.
- `CC/internal/api`: NEW `POST /api/v1/learning/learn` (user principal; `authz.go:81` already refuses ZDR principals for non-read). Hands `{org, user, thread_id, note}` to the sessionreview pipeline (`CC/internal/sessionreview/consumer.go` `handleRunCompleted` path) with trigger `learn`: same gates in the same order (thread origin `chat`, `GetMemoryControls` before any transcript read, org `learningReview = proposals`, waived run refused), skips `LEARNING_REVIEW_MIN_USER_TURNS`, keeps the per-user and per-org daily caps and `budget_unavailable` fail-closed. The note is passed to the reviewer prompt as quoted user data, never as instructions, and capped at 500 chars. The sink writes through `UpsertAgentSkill` with `origin = background_review` (no CHECK change; the revision reason is `learn:` + the reviewer's reason), so every proposal guard applies unchanged.
- FES `LearningSection.tsx`: a "requested by you" chip on proposals whose reason starts `learn:`.

**Flag.** `LEARN_COMMAND_ENABLED` (capability-core, default off; route 404 when off; the gateway route mirrors it through the capability catalogue).

**Tests.** Go: `TestLearnRefusedWhenOrgReviewDisabled`, `TestLearnRefusedForMemoryPaused`, `TestLearnRefusedForNonChatThread`, `TestLearnRespectsDailyCap`, `TestLearnNoteIsQuoted` (prompt fixture), `TestLearnWritesProposalNotEnabledSkill`; extend `CC/internal/learning/eval` seeded cases with a `/learn` case (deterministic replay). MG route test with the mock. FE composer test (`learn_hidden_on_temporary_chat`). ZDR rows (section 5).

**Live proof.** Org opted in; user runs `/learn` on a thread with a repeatable procedure; within the review time one proposal appears in Settings > Learning with "requested by you"; accept; a later turn injects it with a v1 pin. With the org opted out: typed refusal, no model call (inference-core cost line absent). Evidence: screenshots, cost-core line, `psql`. [verify live]

**Effort.** M, 4-5 d. **Dependencies.** S4 (lint applies to proposals), D5, WS07-S9 live proof [verify live]. **Risks.** A user spamming `/learn`: the daily cap and the 202 queue absorb it. Model cost is the reviewer's existing budget.

### S8 - Import a skill from a file [BUILD]

**Goal.** Bring in an existing `SKILL.md` (Agent Skills format) or a zip with `SKILL.md` + files, as a draft the author reviews and publishes.

**Files.**
- `CC/internal/api`: NEW `POST /api/v1/skills/import` (multipart; a single `SKILL.md` or a zip under the WS19-S6 caps, `<= 256 files / 8 MiB / 1 MiB per file`). Parses front matter (`name`, `description`) with the existing `skillbundle` validator (zip-slip, symlinks, exec bit only under `scripts/`); creates a `scope = user` row (org scope admin-only) with `draft` = the body (S5) and the bundle as `state = 'draft'`; lint runs (S4). Nothing is live until publish.
- FES `SkillsSection.tsx`: "Import" button; action `skills.import`.

**Flag.** `SKILL_IMPORT_ENABLED` (capability-core, default off; requires `SKILL_BUNDLES_ENABLED` and `SKILL_DRAFT_PUBLISH`).

**Tests.** Go: `TestImportSkillMdCreatesDraft`, `TestImportZipCreatesDraftBundle`, `TestImportRefusesZipSlipAndSymlink` (reuse validator cases), `TestImportOrgScopeAdminOnly`, `TestImportLintsBody`. FE test.

**Live proof.** Import a public sample skill folder zipped by hand; it shows as Draft; publish; a turn pins v1 and `skill_file` lists its files. Evidence: curl, screenshots. [verify live]

**Effort.** S-M, 3-4 d. **Dependencies.** S4, S5; D2-379 and D2-383 fixed (bundle reads limited to who may see the skill) before the flag is on; D9. **Risks.** Third-party scripts in a bundle: they only ever run inside the sandbox through `cap.command.sandbox` with the read-only `/skills` mount (WS19-S8), never widened (audit 10.1 #10). Import never fetches anything.

### S9 - Libraries: Space scope, org library, `ext.skills` [BUILD, decision-gated D7]

**Goal.** One Library view (Mine, Shared with me, Space, Organisation, Verevon) and the org's `ext.skills` control. Space skills apply only in that Space's threads.

**Files.**
- NEW `SC/migrations/0099_agent_skills_space_scope.sql` (+ revert): extend `agent_skills_scope_chk` to `('org','user','space')` (drop and re-add `NOT VALID`, then `VALIDATE`); CHECK `(scope = 'space') = (scope_id <> '')`.
- `SC/src/grpc.rs` `list_agent_skills`: NEW `ListAgentSkillsRequest.space_id = 5`; Space rows are returned only when the caller passes a `space_id` that session-core resolves from the caller's thread (the gateway sends `thread_id`; session-core reads `threads.space_id`) [INF: exact field; decide at slice start]. Space rows are never returned for a non-Space thread.
- `MG/sse.rs` `fetch_skill_context`: for a Space thread, pull and cache Space rows under a `space:<id>` bucket (the WS07 bucket design in `MG/skills.rs`/`ownership.rs`); never mix them into the org bucket.
- `CC/internal/api/registry_apis.go`: POST/PATCH/DELETE with `scope = space` require a Control-signed Space decision ref for "Space skill write" (D7), verified like other Space decisions [verify which verifier capability-core can call; EP-6 lists `UC/internal/spaces` decisions]. Without the decision kind, the Space half stays off.
- `MG/skills.rs` `handle_match_skills` / `resolve_requested_skills`: read `ext.skills` from the org policy document (WS25-S7): `none` = no authored skills (builtin only); `approved_only` = org and builtin only (user and Space rows skipped); `any` = today. Fail closed to `none` when the policy read fails (schema `fail_closed_value`). Flip the schema row to `enforced`.
- FES NEW `FES/features/skills/components/SkillLibrary.tsx` inside the Customize hub (WS25-S11): tabs per tier; versions and drafts from S2/S5; admin "Publish to organisation" = the existing promotion copy (`registry_apis.go:322-358`).

**Flags.** `SKILL_SPACE_SCOPE` (session-core and capability-core, default off), `SKILL_EXT_POLICY_ENFORCED` (model-gateway, default off).

**Tests.** SC ignored PG: `space_skill_needs_scope_id`, `space_skill_not_listed_outside_its_space`, `space_skill_not_listed_for_non_member_thread`. MG: `ext_skills_approved_only_skips_user_skills`, `ext_skills_policy_unreadable_fails_closed`, `space_bucket_never_leaks_into_org_bucket`. Go: `TestSpaceSkillWriteNeedsDecision`. FE library tests.

**Live proof.** Two Spaces, one Space skill each; a turn in Space A injects only A's; a personal thread injects neither; `ext.skills = approved_only` removes a member's private skill from injection (turn log), and it reappears on `any`. Evidence: SSE/skill logs, policy PUT transcript. [verify live]

**Effort.** M, 6-7 d (org half S, 2 d alone). **Dependencies.** S6 (`scope_id` column and index), D7, Space session (overlap-space row 14). **Risks.** Space membership changes: injection reads membership at turn time through the thread's Space decision [verify live]; a removed member's threads stop seeing Space skills on the next cache refresh (60 s TTL).

### S10 - Flip, document, prove [FINISH]

- Order of flips in dev, each after its proof: `SKILL_VERSIONS_API` (S2) → `MODEL_GATEWAY_SKILL_PINS` (S3) → `SKILL_LINT_MODE=report`, then `enforce` after the report week (S4) → `SKILL_MEMBER_AUTHORING` (S1, after D4) → `SKILL_DRAFT_PUBLISH`, `SKILL_BUNDLE_RETENTION` (S5) → `AUTHORED_KINDS_ENABLED` (S6) → `LEARN_COMMAND_ENABLED` (S7) → `SKILL_IMPORT_ENABLED` (S8) → `SKILL_EXT_POLICY_ENFORCED`, `SKILL_SPACE_SCOPE` (S9). `FEEDBACK_SKILL_VERSIONS` (S3b) last, after a week of version-keyed ratings.
- NEW `MP/scripts/verify-authored-behaviours.sh` (pattern `verify-learning-governance.sh`): versions immutable, pins present, lint parity, draft/publish, `/learn` refusal paths, ZDR rows. It fails on any `unattributed` version row (register D1-645).
- Ledger entries (section 3), `docs/capability-ownership-matrix.md` row for authored behaviours, depth register rows closed, plan Build ledger entry, register triage line for D2-385/D2-386 (code shows them done).
- Effort S, 2 d.

## 5. ZDR clause

**New stores and egress.**

| Store / egress | Slice | Content? | ZDR run / token | Waived run | Erasure |
|---|---|---|---|---|---|
| `agent_skill_versions` | S2 | yes (authored text, same class as `agent_skills`) | not reachable: capability-core refuses non-read from a ZDR principal (`authz.go:81`); learned versions derive only from reviewed, non-ZDR chat runs (WS07) | same as ZDR for learned text (DEC-32); a person's own edit is config, not run content | org purge (`gdpr.rs`), skill delete; owner erasure via `owner_user_id` (D1-643 path) |
| `agent_skills.draft`, `kind`, `settings`, `scope_id`, `bundle_sha256` | S5, S6, S9 | draft yes; others no | as above | as above | with the row |
| `runs.metadata.run_context.skills` (pins) | S3 | no (ids, numbers, hashes) | refused: `RecordRunContext` rejects ZDR and waived callers (`SC/src/grpc.rs:7583-7588`); the ZDR stream injects no skills | refused (same check) | with the run |
| `feedback_ratings.skill_version` | S3b | no | ratings already follow the run's rating rules (unchanged) | unchanged | unchanged |
| retained `skill_bundles`/blobs | S5 | yes (bundle files) | upload refused for ZDR principals; org config (D9) | n/a | org purge, skill delete |
| `/learn` model call (egress to the review model) | S7 | reads a transcript | **refused** before any read | **refused** (DEC-32: background and content-derived jobs refuse waived content) | proposals erased like WS07 proposals |
| Import | S8 | file bytes from the user | refused for ZDR principals | n/a | as bundles |

**Refusal behaviour.** `/learn` on a ZDR or temporary thread is hidden in the SPA; a direct call is refused by capability-core with the typed envelope `zdr_feature_not_supported` and feature `learn_command` before any transcript read or model call; a waived run's thread is refused with the same code (the waiver receipt check WS07 uses). Save-as-skill is hidden on ZDR threads; a ZDR-token write is refused by `authz.go:81` (existing `zdr_durable_mutation_forbidden`). Picking a template under ZDR is client-side text and sends nothing extra; the existing `skills` feature refusal (`zdr-refusals.json` feature `skills`, "picked skills") still applies to explicit skill picks.

**Golden matrix cases.**
- `MP/api/contracts/zdr-refusals.json` features: add `learn_command` (label_en "learning from this conversation") and `skill_authoring` (label_en "saving or editing skills"); regenerate `.sha256` and run `zdr-contract-sync.test.sh`.
- `MP/api/contracts/zdr-posture-matrix.json` `enforcement_rows`: `e_learn_command_refused_under_zdr`, `e_learn_command_refused_for_waived_run`, `e_zdr_turn_pins_no_skill_versions`, `e_skill_authoring_refused_for_zdr_token`, `e_skill_import_refused_for_zdr_token`, `e_template_pick_under_zdr_sends_text_only`.
- Rust golden: extend `MG/tests/zdr_feature_table.rs` with `skill_file`/pins absent under ZDR (closes D2-557 in the same edit).

## 6. Proof plan

1. Unit and contract: per slice commands above; lint golden parity in both languages; `buf lint` (ratchet) and `buf breaking` for `sessions.proto`, `gateway.proto`, `runs.proto`, `capabilities.proto`; `zdr-contract-sync`.
2. Database: every `#[ignore]` test through `session-core-db-tests.sh` on a disposable Postgres 16, plus apply-and-revert of each new session-core migration once (the D1-649 lesson). capability-core integration tests (testcontainers) by the lead.
3. Live (dev stack, rebuilt images via `MP/scripts/compose.sh`; after a worktree build use `recreate-with-live-env.sh`, memory note): flows A-F, each with screenshots, `psql` and log evidence in `docs/plans/wave3/evidence/EP-11-*.md`:
   - A. Save as skill (member) → injected for A, not for B (S1).
   - B. Two edits → v1-v3; immutability error (S2).
   - C. Mid-run edit → recovery returns the pinned text; `runs.metadata` pins (S3).
   - D. Lint report counts, then enforce parity from both writers (S4).
   - E. Draft → publish → restore; old bundle readable by a pinned run (S5).
   - F. Template expansion; preset applies visible controls; `/learn` proposal and refusals; import as draft; Space isolation and `ext.skills` (S6-S9).
4. `MP/scripts/verify-authored-behaviours.sh` green; no `unattributed` version rows.
5. Regression: WS07 two-user proof still holds (A's private skill absent from B's matches), WS19 `skill_budget_contract` and `builtin_skills_contract` green, learning eval seeded cases unchanged.

## 7. Risks and owner decisions

**Risks.**
1. Two writers, one table: any new writer that does not set `mp.*` logs `unattributed` revisions and versions. Mitigation: S10 script fails on it; S4 makes every writer pass the same lint.
2. Hot files: `SC/src/grpc.rs` (20,778 lines), `MG/sse.rs`, `MG/http_routes.rs`, `registry_apis.go`, `DashboardComposer.tsx`, `action-registry.ts`. New logic goes into new modules (`skill_versions_grpc.rs`, `skill_lint.rs`, `skilllint/`); the hot files get call sites and registrations only. CRLF preserved.
3. Index swap (S6) and CHECK changes (S6, S9) on `agent_skills`: older images cannot run the new `ON CONFLICT` target; deploy session-core with its revert file ready (the 0058 lesson, D1-619).
4. Cache lag: a publish reaches a running gateway after up to `SKILL_CACHE_TTL_SECONDS` (60 s). Pins make the lag visible but do not remove it.
5. Bundle read authorization (D2-379, D2-383) is a P1 already open; retention (S5) and import (S8) widen what is stored, so their switches wait for that fix.
6. Scope creep from EP-6 (presets vs agents) and EP-3 (templates vs tokens): D6 draws the line; if either sibling rules otherwise, S6 changes, nothing else does.

**What the owner must decide (and what each blocks).**
- D1 one table with `kind` (blocks S6, S9 index; S1-S5, S7 do not wait).
- D2 version semantics (blocks S2, and through it S3, S5; recommendation given so S2 can start on acceptance).
- D4 member authoring under `ext.skills` (blocks members in S1; S7, S8 for members).
- D5 `/learn` follows the org opt-in, OD4 stays open (blocks S7 only).
- D6 template and preset meaning, acknowledged by EP-3 and EP-6 (blocks S6).
- D7 Space library authority via a Control decision (blocks the Space half of S9).
- D8 lint rules and enforce threshold (blocks S4 `enforce` only).
- D9 ZDR posture of bundle upload and import (blocks turning on S8; D2-384).
- Not decisions but prerequisites the owner should schedule: D2-509 fix (in S4, P0), D2-379/D2-383 (WS19-S6 depth), D1-643 per-user erasure (general session-core gap).
