# Persona / Always-On Custom Instructions — Implementation Spec

Scope: per-user standing instructions applied to every new chat, with a per-thread off switch, visible in the context inspector, interacting correctly with ZDR/temporary chats and org policy. Read-only research; no files were changed to produce this.

---

## 1. What exists today (file:line evidence)

**ADR-0003's authored-instruction hierarchy already does 3/4 of the layering persona needs — org and Space instructions are the direct precedent to extend, not a new pattern.**

- The composer that builds the system message: `authored_instructions_message()` in `apps/Model Plane/rust/services/model-gateway/src/sse.rs:5791-5824`. It concatenates **platform → org → Space → per-agent persona**, each wrapped in "may add to, but must not override the layer(s) before it" framing, and is called once per turn at `sse.rs:1463` into `PromptContext.authored_instructions`.
- `PromptContext` (the ordered message slots): `apps/Model Plane/rust/services/model-gateway/src/prompt_context.rs:196-229`. `authored_instructions` is rendered first, ahead of `identity`, `grounding`, `context_assembly`, `history` (`prompt_context.rs:270-286` legacy order; `:320-329` stable-prefix order).
- Where org/Space text is resolved and injected server-side, stripping any client-supplied value first: `inject_authored_instructions()` in `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4280-4313`, calling `fetch_org_instructions`/`fetch_space_instructions` (`spaces.rs:4220-4255`). Called from all three chat entry points: `domains/chat/json_handlers.rs:99-108`, `domains/chat/streams.rs:57-78`, `domains/ag_ui.rs:93-106`.
- Storage for the org layer: Convex `organizations.instructions` (`apps/Application Plane/convex-core/convex/schema.ts:32-37`), read/written via `organizations:instructionsForGateway` / `organizations:setInstructionsForGateway` (`convex/organizations.ts:196-244`), fronted by the gateway at `GET/PATCH /api/v1/orgs/:id/instructions` (`domains/orgs/instructions.rs`, admin-gated, 4000-char cap at `instructions.rs:23,78`).
- Settings UI precedent: `OrgInstructionsSection.tsx` (`src/features/settings/components/OrgInstructionsSection.tsx`), calling `organization-client.ts:186-207`. Wired into the settings shell at `WorkspaceSettingsPage.tsx:8,473-475` and the sidebar nav list at `CoreSidebarSettingsPanels.tsx:43-58` (`settingsSidebarSections`).
- **Gap already present today, not introduced by persona:** the context inspector does *not* show org/Space/agent-persona layers. `ChatPanels.tsx:520-630` (`ContextWindowPanel`) renders whatever `getThreadContext()` (`chat-client.ts:1849-1890`) returns, which is a pure proxy (`domains/chat/json_handlers.rs:318-350`) to model-gateway's `get_thread_context` (`http_routes.rs:7499-7547`), which calls **only** `session_client.get_context_assembly(...)` with empty `policy_id`/`workspace_id`/`agent_id` (`http_routes.rs:7512-7527`). Org/Space/agent instructions are composed by a *separate* code path (`authored_instructions_message`, built from `InvokeRequest` fields the BFF injects at invoke time) that the read-only inspector endpoint never touches. Persona needs to close this specific gap for itself; it does not pre-exist.
- Segment assembly itself: `assemble_segments()` in `apps/Model Plane/rust/services/session-core/src/grpc.rs:6439-6558`, fed by `AssemblyInputs` (`grpc.rs:6340-6363`) built in `get_context_assembly_inner` (`grpc.rs:3829-3926`). Segment kinds today: `policy, workspace, agent, user, thread, episodic, skill_index, skill_expansion, retrieval, knowledge, graph, prompt` (`grpc.rs:6444-6555`; proto in `apps/Model Plane/proto/model_plane/v1/sessions.proto:727-730`). `ContextSegment.kind` is a free-form string — the inspector renders it with no allow-list (`ChatPanels.tsx:610`, raw `{segment.kind}`), so a new kind needs zero frontend change to display.
- Per-user settings precedent, self-owned (not admin-gated): `/api/v1/preferences` and the generic `/api/v1/settings/{key}` both proxy to a *third* backend, **user-core**, keyed off the caller's own identity (`apps/gateway/src/domains/settings/preferences.rs`, `settings_handlers.rs`). Separate from Convex (org) and from session-core (Postgres).
- Per-thread durable toggle precedent (title/preview/pin, tri-state so "unset" ≠ "false"): `SaveThreadRequest` (`apps/gateway/src/domains/chat/history.rs:66-79`) → `update_durable_presentation` (`history.rs:390-427`) → model-gateway `POST /v1/threads/:id/presentation` → session-core `update_thread_presentation` (`session-core/src/grpc.rs:5030-5144`), which **rejects ZDR callers outright** (`grpc.rs:5038-5042`, "ZDR callers cannot persist thread presentation") and emits a `THREAD_PRESENTATION_UPDATED` event. Column-level precedent for a per-thread flag: migration `session-core/migrations/0038_thread_source_scope.sql`.
- ZDR / temporary chat mechanics: client toggle in `DashboardComposer.tsx` (`temporaryChat` memo, `:1128-1136`, sent as `zdr: temporaryChat()` at `:2220`); server-side posture resolution `apps/gateway/src/zdr.rs` (`request_zdr`, `org_zdr_enabled`) and `apply_org_zdr_posture` (`domains/chat/shared.rs:95-117`); local-storage purge-on-ZDR in `src/features/chat/lib/chat-retention.ts`. **Org instructions already fire unconditionally regardless of the `zdr` flag** — `inject_authored_instructions` has no ZDR check — establishing the precedent that authored-instruction layers are not part of the ZDR data-minimization boundary; they are standing configuration, not conversation content.
- Convex `users` table already exists and already mixes sync fields with app-owned fields on the sibling `organizations` table (`schema.ts:13-44` — `instructions`/`updatedByExternalAuthId` sit right next to `syncStatus`/`lastSyncedAt`). `users` (`schema.ts:55-79`) is keyed by `externalAuthId` with a `by_external_auth_id` index — the exact anchor persona needs, and nothing else uses it for app-owned data yet.

---

## 2. Exact change list per file

### 2.1 Storage — Convex (`apps/Application Plane/convex-core`)

**`convex/schema.ts`** — add to the `users` table (mirrors `organizations.instructions` at `schema.ts:32-37`):
```ts
personaInstructions: v.optional(v.string()),
personaEnabled: v.optional(v.boolean()),      // undefined/absent == true (enabled)
personaUpdatedAt: v.optional(v.number()),
```

**`convex/users.ts` (new file)** — mirrors `organizations.ts:196-244`, but self-service (no admin/membership gate — a user only ever touches their own row):
```ts
export const personaForGateway = query({
  args: { externalAuthId: v.string(), serviceKey: v.string() },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const user = await ctx.db.query("users")
      .withIndex("by_external_auth_id", q => q.eq("externalAuthId", args.externalAuthId))
      .first();
    if (!user) return { instructions: null, enabled: true };
    return { instructions: user.personaInstructions ?? null, enabled: user.personaEnabled ?? true };
  },
});

export const setPersonaForGateway = mutation({
  args: { serviceKey: v.string(), externalAuthId: v.string(),
          instructions: v.optional(v.string()), enabled: v.optional(v.boolean()) },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const user = /* find-or-fail by externalAuthId, same lookup as above */;
    const instructions = args.instructions?.trim() || undefined;
    if ((instructions?.length ?? 0) > MAX_PERSONA_INSTRUCTIONS_LENGTH) {
      throw new Error(`Persona instructions must be ${MAX_PERSONA_INSTRUCTIONS_LENGTH} characters or fewer`);
    }
    await ctx.db.patch(user._id, {
      ...(args.instructions !== undefined ? { personaInstructions: instructions } : {}),
      ...(args.enabled !== undefined ? { personaEnabled: args.enabled } : {}),
      personaUpdatedAt: Date.now(),
    });
    return { instructions: instructions ?? null, enabled: args.enabled ?? user.personaEnabled ?? true };
  },
});
```
`MAX_PERSONA_INSTRUCTIONS_LENGTH = 1500` (not 4000 like org instructions — see §5 on why the smaller cap matters, given `authored_instructions_message` does no runtime trimming).

### 2.2 Rust BFF (`apps/Frontend Plane/verevonv3/apps/gateway`)

**`src/domains/mod.rs`** — add `pub mod persona;`.

**`src/domains/persona.rs` (new file)** — mirrors `domains/orgs/instructions.rs` end to end:
- `GET /api/v1/me/persona` → `{ "data": { "instructions": string|null, "enabled": bool } }`, calling `users:personaForGateway` with `externalAuthId: user.user_id`.
- `PATCH /api/v1/me/persona`, body `{ instructions?: string, enabled?: bool }`, same char-count validation as `instructions.rs:73-86` (character count, not bytes — same Norwegian æ/ø/å rationale), no admin/role gate (self-owned).
- Router registration alongside `settings::router` (wherever `build_router` composes domain routers today), under `require_session`.

**`src/domains/spaces.rs`** — add, next to `fetch_org_instructions`/`fetch_space_instructions` (`spaces.rs:4220-4255`):
```rust
async fn fetch_persona_instructions(state: &AppState, user: &AuthenticatedUser) -> Option<(String, bool)> {
    let value = convex_gateway_call(state, "query", "users:personaForGateway",
        json!({ "externalAuthId": user.user_id })).await.ok()?;
    let text = value.get("instructions").and_then(Value::as_str).map(str::to_owned);
    let enabled = value.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    text.filter(|t| !t.trim().is_empty()).map(|t| (t, enabled))
}
```
New `inject_persona_instructions(state, user, body)`, same shape as `inject_authored_instructions` (`spaces.rs:4280-4313`): strips any client-supplied `persona_instructions`/`persona_enabled` first (same trust-class rule as org/space — "never client-suppliable"), then inserts the server-resolved value only when the **global** `enabled` is true. (The **per-thread** override is applied separately in model-gateway, see 2.3, because it is sourced from session-core's `threads` row, not Convex.) Call it from the same 3 sites as `inject_authored_instructions`: `domains/chat/json_handlers.rs:99-108`, `domains/chat/streams.rs:57-78`, `domains/ag_ui.rs:93-106`.

**`src/domains/chat/history.rs`**:
- `SaveThreadRequest` (`:66-79`) gains `pub(crate) persona_enabled: Option<bool>` (same "omit = leave unchanged" contract as `pinned`).
- `ChatThreadSummary` / `DurableThreadSummary` (`:23-54`, `:107-120`) gain `persona_enabled: bool` (`#[serde(default = "default_true")]`).
- `save_thread` (`:207-256`) includes `persona_enabled` in the `body.title.is_some() || …` dirty-check and forwards it.
- `update_durable_presentation` (`:390-427`) gains a `persona_enabled: Option<bool>` parameter, added to the POST body to model-gateway's `/v1/threads/{id}/presentation`.

**`src/domains/chat/json_handlers.rs`** — `get_thread_context` (`:318-350`): after the proxy call succeeds, if the model-gateway JSON now carries `"persona_enabled": true` (see 2.3), call `fetch_persona_instructions` and prepend a synthetic segment to `segments`:
```json
{ "kind": "persona", "content": "<persona text>", "estimated_tokens": <heuristic> }
```
`estimated_tokens` here is a simple `words.len() * 4 / 3` heuristic (documented as approximate — the BFF has no `mp_tokens` dependency; see §5, "inspector token count is approximate"). When `persona_enabled` is false or absent, no segment is added — zero extra Convex calls on the common "disabled" path.

### 2.3 Model Plane — model-gateway (`apps/Model Plane/rust/services/model-gateway`)

**`src/http_routes.rs`**:
- `InvokeRequest` (`:6693-6704`) gains, mirroring `org_instructions`/`space_instructions`:
  ```rust
  #[serde(default)]
  pub persona_instructions: Option<String>,
  ```
- `get_thread_context` (`:7499-7547`): relay the new `GetContextAssemblyResponse.persona_enabled` field as a top-level `persona_enabled: bool` on `ThreadContextResponse` (default `true` if the RPC didn't set it, i.e. an old session-core binary — fail **open** for display, never fail closed and hide a persona the user turned on).
- The `POST /v1/threads/:id/presentation` handler: parse and forward `persona_enabled: Option<bool>` into `UpdateThreadPresentationRequest.persona_enabled`.

**`src/normalize.rs`** (`:286-287` area) — add `persona_instructions: None` to the default/test-stub `InvokeRequest`.

**`src/sse.rs`**:
- `authored_instructions_message()` (`:5791-5824`): insert a new layer **after Space, before the mention-gated agent persona** (subordinate to org/Space policy, but still able to be overridden by an explicitly-summoned Space agent's own character):
  ```rust
  if persona_enabled {
      if let Some(persona) = req.persona_instructions.as_deref().and_then(non_empty_trimmed) {
          sections.push(format!(
              "--- Your personal instructions (may add to, but must not override, the platform, organization or Space instructions above) ---\n{persona}"
          ));
      }
  }
  ```
  `persona_enabled` is a new `bool` parameter threaded into `authored_instructions_message(state, req, persona_enabled)`, sourced from the **same** `get_context_assembly` response `load_context_assembly_messages` already fetches earlier in the same request path (`sse.rs:4247-4336`) — no extra RPC. Default `true` when that call failed (fail open on the read; the text itself is still gated by the BFF's global `enabled` check in 2.2, so a session-core outage cannot re-enable a globally-off persona — only a per-thread override can be missed, which is the safe direction to fail).

**`proto/model_plane/v1/sessions.proto`**:
- `GetContextAssemblyRequest` — no change needed (persona *text* never reaches session-core in this design; only the *enabled bit* does, read from the thread row it already owns).
- `GetContextAssemblyResponse` (`:718-724`) gains field 3: `optional bool persona_enabled = 3;` (use `optional`, not a bare `bool` — proto3's zero-value for a bare bool is `false`, which would silently mean "disabled" for every thread created before this field existed; `optional` forces session-core to set it explicitly every time, see §5).
- `UpdateThreadPresentationRequest` (`:964-978`) gains field 6: `optional bool persona_enabled = 6;`.
- `ThreadSummary` (`:928-957`) gains field 14: `bool persona_enabled = 14;` (list-level visibility, so the sidebar/composer can show the current per-thread state without opening the inspector).

### 2.4 Session-core (`apps/Model Plane/rust/services/session-core`)

**`migrations/0041_thread_persona_toggle.sql` (new)**:
```sql
ALTER TABLE threads ADD COLUMN persona_enabled BOOLEAN NOT NULL DEFAULT true;
```

**`src/grpc.rs`**:
- `get_context_assembly_inner` (`:3829-3926`): add `SELECT persona_enabled FROM threads WHERE id = $1` (cheap, keyed by `thread_id`, works even without a `run_id`) and set `GetContextAssemblyResponse.persona_enabled = Some(row.persona_enabled)`.
- `update_thread_presentation` (`:5030-5144`): extend the `UPDATE threads SET … pinned_at = CASE …` statement with one more `CASE WHEN $x THEN $y ELSE persona_enabled END` arm (plain boolean, no tri-state NULL semantics needed — simpler than `pinned_at`'s optional-timestamp pattern), bind `req.persona_enabled.is_some()` / `req.persona_enabled.unwrap_or(true)`. Add `"persona_enabled": req.persona_enabled` to the `THREAD_PRESENTATION_UPDATED` event payload (`:5112-5117`).
- `list_threads` handler: include `persona_enabled` in the `ThreadSummary` it returns per row.
- **ZDR guard is inherited for free**: `update_thread_presentation` already rejects ZDR callers (`:5038-5042`), so a temporary chat cannot durably set this column — by design (see §5, "per-thread toggle and ZDR").

---

### 2.5 Frontend (SolidJS, `apps/Frontend Plane/verevonv3/src`)

**`shared/api/persona-client.ts` (new)** — mirrors `organization-client.ts:186-207`:
```ts
export async function getPersonaInstructions(): Promise<{ instructions: string; enabled: boolean }>
export async function updatePersonaInstructions(instructions: string, enabled: boolean): Promise<{ instructions: string; enabled: boolean }>
```
hitting `GET`/`PATCH /api/v1/me/persona`.

**`shared/api/chat-client.ts`**:
- `ChatThreadSession` (`:646-664`) gains `personaEnabled: boolean`.
- `SaveChatThreadSnapshotRequest` (`:692-704`) gains `personaEnabled?: boolean` (omit = leave unchanged, exact same contract as `pinned?`).
- The `saveChatThreadSnapshot` caller (`:1932-…`) passes it through; the thread-summary parse (`:2078` area, where `pinned: item.pinned === true` lives) gains `personaEnabled: item.personaEnabled !== false`.
- **No change needed** to `ContextSegment`/`ThreadContext` (`:1786-1890`) — `kind` is already a free string; `"persona"` flows through untouched.

**`features/settings/components/PersonaSection.tsx` (new)** — structurally a copy of `OrgInstructionsSection.tsx` with three differences: (a) no admin gate — the signed-in user always edits their own; (b) an `enabled` toggle (checkbox/switch) alongside the textarea; (c) `1500`-char cap and persona-appropriate placeholder copy. **Use `i18n.tr(...)` pairs throughout** rather than the hardcoded-Norwegian strings `OrgInstructionsSection.tsx` currently uses (`:69-104`) — don't propagate that inconsistency into a new file, given the owner's explicit Norwegian-first-via-`i18n.tr` direction.

**`features/settings/components/WorkspaceSettingsPage.tsx`** — import `PersonaSection`, add `<Match when={props.section === 'persona'}>` (mirrors `:473-475`).

**`features/core/components/sidebar/CoreSidebarSettingsPanels.tsx`** — add one entry to `settingsSidebarSections` (`:43-58`), e.g. `{ id: 'persona', label: 'Persona', icon: UserCog /* verify export in @/shared/icons, else Wand2 */, href: '/settings/persona' }`, and the matching `i18n.tr` case in the section-label switch (`:270-284`).

**Composer / thread UI** — a small per-thread control (e.g. in the thread's "…" menu, next to rename/delete/pin) that reads `ChatThreadSession.personaEnabled` and calls `saveChatThreadSnapshot(threadId, { personaEnabled: !current })`. **Disable (not hide) this control when the thread is a temporary/ZDR chat** (`temporaryChatLocked` / `props.temporaryChat` in `DashboardComposer.tsx`), with a tooltip explaining the toggle can't be saved for a temporary chat — matching the reason `update_thread_presentation` rejects ZDR callers (2.4).

**Context inspector (`ChatPanels.tsx:535-630`)** — no required change (kind renders raw). Optional polish: a `kind → i18n label` map (e.g. `persona` → "Personlige instrukser" / "Personal instructions") so the panel doesn't show the raw wire string; worth doing in the same PR since it's a two-line addition, but not load-bearing.

---

## 3. Tests to add

**`apps/gateway/src/domains/persona.rs`** (new `#[cfg(test)]` module, wiremock-mocked Convex + user-core auth, same harness as `orgs/instructions.rs:110-253`):
- `a_user_can_read_their_own_persona`
- `a_user_can_write_their_own_persona`
- `overlong_persona_instructions_are_rejected_before_reaching_convex` (>1500 chars)
- `four_hundred_norwegian_characters_are_accepted` (æøå char-count, not byte-count)
- `enabled_false_is_persisted_and_read_back`
- `omitting_instructions_leaves_the_saved_text_unchanged` (PATCH with only `{enabled}`)

**`apps/gateway/src/domains/spaces.rs`** (extend the existing `inject_authored_instructions` test block near `:9204-9299`):
- `persona_instructions_are_injected_on_a_personal_turn`
- `disabled_persona_is_not_injected_even_when_authored`
- `client_supplied_persona_instructions_are_discarded_and_replaced` (forged-value test, mirrors `:9270-9285`)

**`apps/gateway/src/domains/chat/history.rs`**:
- `save_thread_updates_persona_enabled_without_touching_title_or_pin`
- `thread_listing_reports_persona_enabled_default_true_for_legacy_threads`

**model-gateway `src/sse.rs`** (extend the `authored_instructions_message` test block near `:9218-9271`):
- `persona_layer_sits_between_space_and_agent_persona`
- `persona_layer_absent_when_thread_persona_disabled`
- `persona_layer_absent_when_persona_instructions_blank`
- `agent_persona_still_overrides_personal_persona_on_a_mentioned_turn` (compose order regression)

**session-core `src/grpc.rs`**:
- `get_context_assembly_reports_persona_enabled_from_the_threads_row`
- `update_thread_presentation_toggles_persona_enabled_independent_of_pin_and_title`
- `update_thread_presentation_rejects_persona_toggle_for_a_zdr_caller` (reuses the existing ZDR-rejection test shape at `:5038-5042`)
- extend `assemble_segments`'s existing table-driven tests (`:8390-8507` area) with a `persona_segment` case if persona is ever folded into `assemble_segments` directly (see §6, decision 2) — otherwise not needed under the chosen design.

**Frontend**:
- `src/shared/api/organization-client.test.ts`-equivalent new `persona-client.test.ts`: get/update round-trip, error translation.
- `PersonaSection.test.tsx` (mirror whatever `OrgInstructionsSection` test file exists, if any — check for one before assuming none): renders saved text, dirty-tracking, submit, error state, enabled-toggle.
- `ChatPanels.test.tsx` (extend existing context-inspector tests): a `persona`-kind segment renders like any other segment (content + token count), confirming zero special-casing is needed.
- `chat-client.test.ts`: `SaveChatThreadSnapshotRequest` round-trips `personaEnabled`; omitting it leaves the field untouched (mirror the existing `pinned` omission test).

---

## 4. Rollout

Services needing rebuild (per the "build via launcher" rule already in project memory — hand-built `docker compose build` misses the Control Plane env chain):
1. **convex-core** — schema + new `users.ts` functions deploy (Convex deploy, not a Docker rebuild — separate pipeline from the launcher).
2. **model-gateway** — proto regeneration (new `sessions.proto` fields) + `sse.rs`/`http_routes.rs`/`normalize.rs` changes. Needs a full rebuild since it's Rust with generated protobuf types.
3. **session-core** — new migration + `grpc.rs` changes. Migration must run before the new binary starts (standard sqlx migration-on-boot or a migration step in the launcher, whichever this project already uses — verify against how `0038`-`0040` were rolled out).
4. **apps/gateway (BFF)** — new route + injection function + history.rs changes. Rebuild via the launcher's `--from model` path (per project memory: "the launcher build ran at 2026-09-28T01:28–01:41Z" for Wave 1 — same mechanism).
5. **Frontend (verevonv3 SolidJS)** — new settings section, sidebar entry, client, thread-toggle UI. Vite dev container needs a restart to pick up source changes (per project memory: bind-mount watching is disabled).

Flags: no feature flag is strictly required — the feature is additive and off-by-default in effect (no user has authored persona text yet, so `authored_instructions_message` emits nothing new until someone writes one). If a kill switch is wanted for the rollout window, the cheapest option consistent with existing conventions (`context_assembly_budget()`'s `env::var` pattern in `sse.rs:4109-4115`) is a single model-gateway env var, e.g. `MODEL_GATEWAY_PERSONA_LAYER_DISABLED=1`, checked once at the top of the new block in `authored_instructions_message`.

Dependency order: Convex schema/functions → session-core migration/RPC → model-gateway → BFF gateway → frontend. The BFF and frontend can be developed against a stubbed model-gateway response in parallel, but the real end-to-end behavior needs all five in place.

---

## 5. Risks and edge cases

- **No runtime token trimming for this message class.** `authored_instructions_message` (and by extension org/Space/persona) is inserted whole, with no budget check — unlike `context_assembly`'s segments, which are trimmed by `assemble_segments`'s budget logic. This is a pre-existing property (org/Space instructions already have it), not something persona introduces, but persona's write-time cap (1500 chars) is the *only* safety net — set it deliberately smaller than org's 4000, since a persona is meant to be a short style/tone preference, not a policy document.
- **proto3 bool zero-value trap.** `GetContextAssemblyResponse.persona_enabled` must be `optional bool`, not a bare `bool` — a bare bool's unset wire value is `false`, which would read as "disabled" for every request from an unupgraded session-core replica during a rolling deploy, silently turning persona off for everyone mid-rollout. Same reasoning applies to `UpdateThreadPresentationRequest.persona_enabled` (already `optional` by construction, matching `pinned`).
- **Per-thread toggle cannot be durably set for a ZDR/temporary thread** (`update_thread_presentation` rejects ZDR callers outright, `grpc.rs:5038-5042`). The UI must disable, not silently no-op, the per-thread control for a temporary chat, and explain that the *global* enabled switch (Settings → Persona) is the only lever available there. Persona *text* itself, unlike the per-thread toggle, still applies on a ZDR turn by default — consistent with org instructions already doing so — which is a deliberate choice (see decision 1 below) and should be stated in the settings UI copy so it isn't a surprise.
- **Inspector segment is synthetic and its token count is a heuristic**, computed in the BFF (no `mp_tokens` dependency there), while the real prompt's persona text is uncounted entirely (no trimming exists for it either). The displayed "used / budget" ratio in `ContextWindowPanel` will not include the persona segment's tokens in that ratio, since it never passed through session-core's real budget accounting. This is a cosmetic inconsistency worth a one-line code comment at the merge point, not a functional bug.
- **Ordering interacts with the mentioned-agent case.** A user's personal persona sits *before* the mention-gated agent persona in `authored_instructions_message`, so an `@`-mentioned Space agent's own character can still override a user's personal style preference for that turn — verify this is the intended precedence (it mirrors "a bound agent's character wins when explicitly summoned," but an owner could reasonably want the opposite for accessibility/consistency reasons — flagged, not assumed).
- **Race between the two enabled bits.** Effective persona-on-for-this-turn requires the global `users.personaEnabled` (Convex, fetched by the BFF) AND the thread's `persona_enabled` (session-core, fetched by model-gateway) to both be true, computed on different services on the same request — a moment where a user just flipped the global switch off in another tab won't retroactively affect an in-flight turn, same class of staleness the codebase already accepts for `org_zdr_enabled`'s 60-second cache (`zdr.rs:35`).
- **New Space agents inherit no persona interaction contract today** — `agent_persona_message` doesn't know about the concept; confirm no regression by adding the "agent persona still overrides personal persona" test explicitly (§3).

---

## 6. Genuine owner decisions

1. **Does persona text apply on a ZDR/temporary chat by default?** This spec follows the existing org-instructions precedent (fires unconditionally, ZDR only blocks the per-thread *toggle*, not the text). An owner could instead want persona text suppressed entirely on ZDR turns, treating "style preference" as more personal than "org policy." This changes the gating condition in `authored_instructions_message` (add a `!zdr` check) and changes the settings-UI copy.
2. **Is "optionally per-org" a policy knob (org can allow/forbid personal personas, cap their length) or a second org-wide authored-persona text (redundant with the existing `organizations.instructions` field)?** This spec assumes the former (a policy knob, not new content) because a second org-wide free-text field would functionally duplicate org instructions. If the owner actually wants an org-wide *default persona template* members can adopt/customize (distinct from "org rules"), that's a materially larger surface: a third Convex text field, an admin UI, and a "reset to org default" affordance in `PersonaSection.tsx`.
3. **Precedence between a user's personal persona and an `@`-mentioned Space agent's character** (see §5) — confirm the assumed ordering (agent wins) is wanted.
4. **Should the per-thread toggle be visible in the composer at all, or only in a "…" menu / the context inspector itself?** Affects placement but not the underlying data model — not load-bearing for this spec, listed for completeness rather than as a design-changing fork.

---

## 7. Size and dependencies

**Size: M** (matches the plan doc's own Wave-2 grouping alongside "the source-scope filter" and "the attach-from-connector picker" — real backend + storage + UI, but no new external integration, no approval/consent flow, and it reuses four already-proven patterns end to end: ADR-0003's instruction hierarchy, the `pinned`/presentation tri-state RPC, the generic free-string `ContextSegment.kind`, and the Convex `organizations.instructions` precedent).

**Dependencies on other Wave-2 items:**
- **Independent of** the source-scope filter, the attach-from-connector picker, and "forward an email to create a task" — no shared code paths.
- **Touches the same files as** the editable-plan-gate and inline-citation-chips items only incidentally (both also live in `ChatPanels.tsx`/`sse.rs`); sequence them to avoid merge conflicts in `authored_instructions_message` and `ContextWindowPanel`, but neither is a hard prerequisite.
- **Should land after, not alongside, any change to `GetContextAssemblyRequest`/`Response`'s wire shape** that another Wave-2 item might also be making (none identified in the current plan) — coordinate proto field-number allocation if two features touch `sessions.proto` in the same cycle.