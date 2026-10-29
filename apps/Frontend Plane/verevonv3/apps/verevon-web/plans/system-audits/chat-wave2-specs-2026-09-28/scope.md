# Source-scope filter for a chat turn — implementation spec

Repo root: `C:/dev/CoresSystem`. Evidence gathered read-only; no files were edited, no build/test/git commands were run.

## 1. What exists today

**The axis that exists is binary and privacy-scoped, not a Perplexity-Focus source filter.** `ChatSourceScope = 'workspace' | 'conversation'` (`src/shared/actions/chat-source-scope.ts:5`). It is a data-retention/isolation switch ("Kunnskap + nett" vs "Kun samtalen"), not a source-*selection* control. There is no "org only" or "web only" value anywhere in the stack.

**Composer UI** — `src/features/dashboard/home/DashboardComposer.tsx`:
- State: `internalSourceScope` signal, default `'workspace'` (`:813`); `sourceScope()`/`conversationOnly()` accessors (`:814-815`).
- The only picker is a 2-option `<select>` inside the "context scope" popover: `workspace` = "Tillatte arbeidsområdekilder" / `conversation` = "Kun samtalen og vedlegg" (`:2680-2687`).
- The pill button shown pre-send reads `conversationOnly() ? "Kun samtalen" : browseWeb() ? "Kunnskap + nett" : "Kunnskap"` (`:2654,2669`) — i.e. "web" is really just the pre-existing, independent `browseWeb` toggle rendered next to the scope word, not part of the scope enum.
- Everything that touches sources (Browse-web button `:3196-3199`, Deep-research button `:3170-3172`, Plan mode `:2726-2728,2763-2765`) is simply `disabled={... || conversationOnly()}` — a UI-side all-or-nothing gate, no partial states exist.
- On submit: `payload.sourceScope = submittedSourceScope; if (submittedConversationOnly) payload.tools = []` (`:2236-2238`).
- `sourceScopeLocked` is passed from `ChatPage.tsx:207` as `state.turns.length > 0` — the picker is disabled after the first turn.

**Wire contract** — `src/shared/api/chat-client.ts`:
- `buildChatWireBody` (`:917-1000+`): `const conversationOnly = request.sourceScope === 'conversation'` (`:918`). It then:
  - adds/removes the **feature flag** `'conversation_only'` in the `features[]` array (`:924-925`) — this is the only place the scope reaches the wire; there is no dedicated `source_scope`/`retrieval_scope` field.
  - force-clears `browse_web`, `generate_image`, `plan_mode`(`agentic`), `deep_research` to `false` whenever `conversationOnly` (`:947,968,969,975,980`).
  - `tools` (explicit tool specs) still get built normally by `buildToolSpecs` (`:855-869`) even under conversation-only; filtering happens server-side, not here.
- Test coverage of today's behavior: `chat-client.test.ts:469-470`.

**model-gateway** — the actual enforcement lives entirely behind one boolean, `conversation_only`, derived once per request from the wire feature list:
- Derivation and hard reject of incompatible combos: `sse.rs:669-674` (`conversation_only && (deep_research || generate_image || agentic) → error "source_scope_conflict"`).
- What it turns off, all gated by the same boolean:
  - RAG/Data-Plane grounding: `sse.rs:735` (`!conversation_only && wants_grounding(...)`), the best-effort fallback path `:1352`, and the "say nothing was found" honesty notice `:1427`.
  - Session-core context assembly (workspace docs pulled into the prompt): `sse.rs:1207` (`if conversation_only { None } else { ... }`).
  - Long-term memory recall for the turn: `sse.rs:1302`.
  - Skill-catalogue fetch: `sse.rs:1471`.
  - `web_search` tool availability: `sse.rs:1577` (`web_search_available = !conversation_only && (...)`).
  - **The actual tool allowlist**, `tool_loop.rs:4151-4153`: `conversation_tool_allowed(name)` — a hardcoded allowlist of exactly `create_artifact | read_artifact | update_artifact | count_words | code_interpreter | reattach_context`. Applied once, at `sse.rs:1628-1629`: `tool_defs.retain(|tool| conversation_tool_allowed(&tool.name))`, i.e. it prunes the *advertised* tool list.
  - A system message is injected: `sse.rs:1630-1634`, text = `source_validation::CONVERSATION_SCOPE_NOTICE` (`source_validation.rs:11`).
- **This is genuinely server-enforced, not prompt text**, because of a second, independent gate: `tool_loop.rs:6815` — every model tool call is checked against `offered_tools` (the same post-filter list) immediately before dispatch: `if !offered_tools.iter().any(|tool| tool.name == call.name) { return Ok(err_outcome(call, "Tool is not available for this turn's source scope.")); }`. A hallucinated call to a non-offered tool is rejected here even if the model never should have been able to name it. This is the exact mechanism the new modes must reuse.
- Builtin tool catalogue (what exists to bucket): `tool_loop.rs:4173+` — `count_words, reattach_context, reattach_skill, web_search (:4191), get_weather, get_statistics, code_interpreter, create_artifact, inbox_search (:4216), inbox_get_conversation, read_artifact, update_artifact`, plus (elsewhere) `knowledge_search` (dispatch arms at `tool_loop.rs:2298,3299`; def at `:4251`), `knowledge_graph_search`, `knowledge_wiki_search`, `fetch_url`, `result_query`, `mcp_call`, `action_execute`, `browser_agent` — enumerated together as the source-restricted set in the existing test `tool_loop.rs:10018-10025`.
- Mid-run queued input inherits the same boolean: `queued_input.rs:75,91,108,116,178-182` (`EnqueueOutcome::Queued{ conversation_only, ... }`), consumed at `http_routes.rs:7065-7066,7129`.

**session-core** — persistence and immutability:
- Schema: `session-core/migrations/0038_thread_source_scope.sql` — `threads.source_scope TEXT NOT NULL DEFAULT 'workspace' CHECK (source_scope IN ('workspace','conversation'))`.
- Immutability enforced under the thread lock in the message-write path, `grpc.rs:2367-2389`: first user message can set `conversation`; every later message must match what's stored (`stored_isolation`/`requested_isolation` compared, mismatches rejected with `failed_precondition`); the derived boolean is written back into `messages.metadata.source_scope`.
- Blocks long-term memory creation: `memory_grpc.rs:461-467` (reads `threads.source_scope = 'conversation'`, rejects with `"Conversation-only tasks cannot create long-term memories."`).
- Blocks dreaming/learning extraction: `dreaming.rs:142,216-219` (`record_dream_run(..., "skipped_conversation_scope", ...)`), and `grpc.rs:2453` (`candidates = if conversation_only { Vec::new() } else { extract_memory_candidates(...) }`).

**BFF (gateway, Rust)**:
- `apps/gateway/src/domains/chat/support.rs:28-32` strips `support_context_query` when `conversation_only` is in `features[]` — a passthrough consumer, not a source of truth.
- `apps/gateway/src/domains/chat/history.rs:518,629` relays `source_scope` from session-core into the wire shape as `turn.sourceScope`, `'conversation'`-only (line 629 only sets the key when the value is `"conversation"`).

**Controller/state** — `use-chat-controller.ts`:
- The scope is **pinned per thread**, read off the first user turn: `sourceScope = () => state.turns.find(t => t.role==='user')?.sourceScope ?? (state.turns.length ? 'workspace' : selectedSourceScope())` (`:276`). Threaded into every send path: `:1851, 2210, 2432, 2757`.

**Citations — no origin tag exists.** `ChatEvent::Citation` (`sse_events.rs:49-54`) carries only `{ id, title, url, snippet }`; client `Citation` type (`chat-types.ts:89-100`) adds `claimId`/`sourceGroupId`/`start`/`end` but nothing distinguishing web vs. org-knowledge provenance. `chat-normalizers.ts:1295-1307` (`normalizeCitation`) is the exact function that would need a new field threaded through. This confirms the plan's own citation-chip gap note (`CHAT_PAGE_GAP_PLAN_2026-09-28.md` row "Inline citation chips").

**No receipt UI exists for scope at all.** `ChatMessages.tsx` has zero matches for `sourceScope`/`source_scope`; the assembled-context inspector (`ChatPanels.tsx:535-630`, tooltip copy at `:555`) shows token budget, not source scope. The only place scope is visible to the user today is the composer's own pre-send pill (`DashboardComposer.tsx:2654-2670`), which disappears once the turn is sent.

## 2. Design and exact change list

### 2.1 Key design decision: keep this orthogonal to the existing privacy axis

The plan item says "for a chat turn" — and Perplexity's Focus is explicitly changeable message-to-message inside one thread. The existing `workspace`/`conversation` axis is a **privacy/retention commitment**, thread-locked by design (session-core immutability, blocks memory/dreaming) — that machinery is correct and load-bearing and should not be touched.

**Recommendation:** add a *second*, orthogonal, per-turn field that only has meaning while the thread is in `workspace` scope, rather than turning `ChatSourceScope` into a 4-value enum:

```ts
// src/shared/actions/chat-source-scope.ts
export type RetrievalScope = 'auto' | 'knowledge_only' | 'web_only'
export const DEFAULT_RETRIEVAL_SCOPE: RetrievalScope = 'auto'
export function normalizeRetrievalScope(value: unknown): RetrievalScope {
  return value === 'knowledge_only' || value === 'web_only' ? value : 'auto'
}
```

`auto` = today's byte-identical behavior (grounding on, web via the existing Browse toggle/auto-signal). This means:
- **No new DB column, no CHECK-constraint migration, no change to the thread-immutability code path** (`grpc.rs:2367-2389` stays untouched). The per-turn value is small enough to live in the existing free-form `messages.metadata` JSON that `grpc.rs:2362-2365` already persists — no schema change needed there either.
- The composer's `sourceScopeLocked` continues to gate only `workspace`↔`conversation`; the new retrieval-scope picker is **never locked** and can change every turn, which is what "Perplexity Focus style" and "for a chat turn" actually mean.
- When `sourceScope === 'conversation'`, `retrieval_scope` is irrelevant and must be forced to `'auto'`/omitted on the wire (conversation-only already disables everything it would touch).

This is presented as the recommended design; §6 flags the one-line alternative (thread-locking the new axis too) as an owner decision, because it is a real fork that changes the DB/migration story.

### 2.2 Frontend changes

**`src/shared/actions/chat-source-scope.ts`** — add `RetrievalScope` type, `DEFAULT_RETRIEVAL_SCOPE`, `normalizeRetrievalScope` as above. No changes to `ChatSourceScope`.

**`src/features/dashboard/home/DashboardComposer.tsx`**:
- New prop pair mirroring the existing `sourceScope`/`onSourceScopeChange` pattern (not `sourceScopeLocked` — this one is never locked): `retrievalScope?: RetrievalScope`, `onRetrievalScopeChange?: (v: RetrievalScope) => void`.
- New signal `internalRetrievalScope` (default `'auto'`), accessor `retrievalScope = () => props.retrievalScope ?? internalRetrievalScope()`, reset to `'auto'` whenever `conversationOnly()` becomes true (mirror the existing `createEffect` pattern at `:1163-1174` that resets voice state on `temporaryChat()`).
- Extend the existing context-scope popover (`:2678-2696`) with a second control, disabled when `conversationOnly()` — either three radio pills ("Kunnskap + nett", "Kun organisasjon", "Kun nett") or a second `<select>` reusing `COMMAND_*`-style inline styles already in the file. Reuse the existing pattern: `disabled={props.sourceScopeLocked || props.submitting || conversationOnly()}` minus the lock.
- Update the pill label builder at `:2654,2669` to read the new value too, e.g. `retrievalScope()==='knowledge_only' ? "Kun organisasjon" : retrievalScope()==='web_only' ? "Kun nett" : conversationOnly() ? "Kun samtalen" : browseWeb() ? "Kunnskap + nett" : "Kunnskap"`.
- When `retrievalScope()==='web_only'`: the Browse-web toggle becomes redundant (web is now unconditional) — disable it and show it as permanently active, same visual treatment already used for `subscriptionBacked()` (`:3196-3199`, `subscriptionDisabledStyle()`/`subscriptionDisabledTitle()` at `:868-876`; add a parallel `webOnlyForcedTitle()`).
- When `retrievalScope()==='knowledge_only'`: disable the Deep-research button (`:3170-3172`) the same way, since deep research is a web pipeline.
- `DashboardComposerSubmitPayload` (`:262-301`): add `retrievalScope?: RetrievalScope`.
- Submit handler (`:2221-2239`): capture `submittedRetrievalScope = retrievalScope()`, set `payload.retrievalScope = conversationOnly() ? undefined : submittedRetrievalScope` (omit when `'auto'` or conversation-only, matching the existing omit-when-default convention used for `effort`/`verbosity`/`minPrivacyTier`).

**`src/features/chat/components/use-chat-controller.ts`**:
- New signal `selectedRetrievalScope`, accessor `retrievalScope = () => /* per-turn, NOT read off the first user turn */ selectedRetrievalScope()` — deliberately **not** mirroring the `sourceScope()` first-turn-pin pattern at `:276`, since this axis is per-turn.
- Thread `retrievalScope` through every `sendContent`/`sendMidRun` call alongside `sourceScope` at `:1851, 2210, 2432, 2757` (same shape as the existing `sourceScope` prop threading).
- Export `retrievalScope`/`setRetrievalScope` from the controller's return object next to `sourceScope`/`setSourceScope` (`:3730-3731`).

**`src/features/chat/components/ChatPage.tsx`**: pass `retrievalScope`/`onRetrievalScopeChange` into `<DashboardComposer>` next to the existing `sourceScope`/`onSourceScopeChange` wiring (`:205-207`) — no `Locked` prop.

**`src/shared/api/chat-client.ts`**:
- `ChatInvokeRequest` (`:51-56` area): add `retrievalScope?: RetrievalScope`.
- `ChatMessage` (`:628-633` area): add `retrievalScope?: RetrievalScope` (for history playback — see history.rs below).
- `buildChatWireBody` (`:917+`): 
  ```ts
  const retrievalScope = conversationOnly ? 'auto' : normalizeRetrievalScope(request.retrievalScope)
  ...
  ...(retrievalScope !== 'auto' ? { retrieval_scope: retrievalScope } : {}),
  ```
  placed alongside the existing `effort`/`verbosity` omit-when-default fields (`:984-992`).
- History deserialization (`:2021` area): add `retrievalScope: (item.retrieval_scope ?? objectValue(item.metadata)?.retrieval_scope) === 'knowledge_only' ? 'knowledge_only' : (...) === 'web_only' ? 'web_only' : undefined` mirroring the existing `sourceScope` line exactly.
- `emitCitation` (`:838-851`): add `origin: str(payload.origin)` passthrough (see server-side citation change below).

**`src/features/chat/components/chat-normalizers.ts`**: `normalizeCitation` (`:1295-1307`) — add `origin?: 'web' | 'knowledge'` passthrough, same `...(event.origin ? { origin: event.origin } : {})` pattern already used for `claimId`/`sourceGroupId`.

**`src/features/chat/components/chat-types.ts`**: `Citation` type (`:89-100`) — add `origin?: 'web' | 'knowledge'`.

**Receipt display (new, closes the "shown in the receipt" requirement)**:
- `ChatMessages.tsx`: near the existing verification-notice block (`:2096-2168`) and memory-recall disclosure (`:2179-2280`), add a small scope badge on assistant turns where `turn.retrievalScope` (or the request's effective scope) is not `'auto'`/`'workspace'` — reuse the existing disclosure-chip visual language. Text: `i18n.tr("Kilder: kun organisasjonskunnskap", "Sources: organization knowledge only")` / `i18n.tr("Kilder: kun nett", "Sources: web only")`.
- `ChatPanels.tsx` assembled-context inspector (`:535-630`): add one line showing the excluded tool classes for this turn (e.g. "Nett-kilder ekskludert" / "Organisasjonskilder ekskludert") sourced from the new SSE frame below — this is the concrete "shown in the receipt" surface, reusing the panel that already exists for exactly this kind of transparency.

### 2.3 model-gateway changes (Rust)

**`http_routes.rs`** — `InvokeRequest` struct (`:6520+`): add, next to `browse_web`/`deep_research` (`:6552-6579`), following the same dedicated-field rationale already documented there ("duplicates as a durable request flag so client/BFF drift can't lose it"):
```rust
/// Per-turn retrieval-source filter (Perplexity-Focus style). Absent/unknown
/// → "auto" (today's behavior, byte-identical). Meaningless and ignored
/// when `conversation_only` is in effect — that mode already excludes both.
#[serde(default, alias = "retrievalScope")]
pub retrieval_scope: Option<String>,
```
New normalizer, colocated with existing wire-normalization helpers (e.g. `normalize.rs`): `fn retrieval_scope(raw: Option<&str>) -> RetrievalScope { match raw { Some("knowledge_only") => KnowledgeOnly, Some("web_only") => WebOnly, _ => Auto } }`.

**`source_validation.rs`**: add two new notice constants beside `CONVERSATION_SCOPE_NOTICE` (`:11`) — deliberately new text, not reusing the existing constant, so `SourceContext::from_messages`'s exact-match gate (`:51-57`, used by the attachment-only fact-checker) stays scoped to true conversation-only and is unaffected:
```rust
pub const KNOWLEDGE_ONLY_SCOPE_NOTICE: &str = "Source scope: organization knowledge only. Public web search and browsing are disabled for this turn. Do not claim to have checked the web, and do not present general/training-data claims as if they came from the organization's knowledge base.";
pub const WEB_ONLY_SCOPE_NOTICE: &str = "Source scope: public web only for this turn. Organization knowledge base, personal memory and learned skills are not consulted. Do not present general/training-data or organizational claims as verified — cite the web sources you actually used.";
```

**`tool_loop.rs`**: replace the single `conversation_tool_allowed` gate's role in the two new modes with an explicit bucket table next to it (`:4151-4153`):
```rust
const WEB_CLASS_TOOLS: &[&str] = &["web_search", "fetch_url"];
const ORG_CLASS_TOOLS: &[&str] = &["knowledge_search", "knowledge_graph_search", "knowledge_wiki_search", "inbox_search", "inbox_get_conversation", "result_query"];

pub fn tool_allowed_for_retrieval_scope(name: &str, scope: RetrievalScope) -> bool {
    match scope {
        RetrievalScope::Auto => true,
        RetrievalScope::KnowledgeOnly => !WEB_CLASS_TOOLS.contains(&name),
        RetrievalScope::WebOnly => !ORG_CLASS_TOOLS.contains(&name),
    }
}
```
`mcp_call`, `action_execute`, `browser_agent` are deliberately left in neither bucket (unrestricted by this filter) — flagged as an owner decision in §6, since they are connected-service tools that arguably belong in one bucket or should be excluded from both restrictive modes entirely.

**`sse.rs`** — the exact call sites already found for `conversation_only`, each extended:
- `:669-674` guard: extend to also reject `retrieval_scope == KnowledgeOnly && req.deep_research` with a scope-specific message ("Org-knowledge-only turns can't run web research. Switch to Kunnskap + nett or Kun nett to use Dyp research."). `web_only` does **not** need this guard — deep research is compatible with it.
- `:735,1175,1207,1302,1352,1427,1471` (grounding / context assembly / memory recall / skill fetch): change each `!conversation_only` condition to `!conversation_only && retrieval_scope != WebOnly` — i.e. `web_only` skips exactly the same set of things `conversation_only` skips for these specific mechanisms, `knowledge_only` does not.
- `:1577` `web_search_available`: 
  ```rust
  let web_search_available = !conversation_only && retrieval_scope != KnowledgeOnly &&
      (retrieval_scope == WebOnly || client_requested_web_search || web_search_signal || deep_research_requested);
  ```
  (`web_only` forces availability on unconditionally, per §1's design note — flagged as owner decision #3 in §6 if the intent is instead to still require the Browse toggle even under `web_only`.)
- `:1628-1635`: generalize the existing `if conversation_only { tool_defs.retain(conversation_tool_allowed); prompt.conversation_scope = Some(CONVERSATION_SCOPE_NOTICE) }` block to:
  ```rust
  match retrieval_scope {
      _ if conversation_only => { /* unchanged existing branch */ }
      KnowledgeOnly => { tool_defs.retain(|t| tool_allowed_for_retrieval_scope(&t.name, KnowledgeOnly)); prompt.conversation_scope = Some(system(KNOWLEDGE_ONLY_SCOPE_NOTICE)); }
      WebOnly => { tool_defs.retain(|t| tool_allowed_for_retrieval_scope(&t.name, WebOnly)); prompt.conversation_scope = Some(system(WEB_ONLY_SCOPE_NOTICE)); }
      Auto => {}
  }
  ```
- **Citations**: `ChatEvent::Citation` (`sse_events.rs:49-54`) gains `origin: String` ("web" | "knowledge"). Every construction site found (`deep_research.rs:1962,1971`; `sse.rs:1802,3031,7702`; `tool_loop.rs:5418,5789,10521`) needs the literal added — deep_research/web_search-tool sites emit `"web"`, `knowledge_search`/grounding sites emit `"knowledge"`. `RecordedCitation` (`sse_events.rs:505`) and `GroundingCitation` (`retrieval.rs:86`) get the same field for the persisted/replay path.
- **Receipt**: emit a new lightweight frame once per turn when `retrieval_scope != Auto`, e.g. `ChatEvent::SourceScopeApplied { scope: String, excluded_tools: Vec<String> }`, alongside the existing `ResultReceipt`/`Grounding` events in the same enum (`sse_events.rs:22-70`) — this is what §2.2's new `ChatPanels.tsx` line and `ChatMessages.tsx` badge consume.

**`queued_input.rs`**: `QueuedEntry`/`EnqueueOutcome` (`:75,91,108,116,178-182`) — add `retrieval_scope: RetrievalScope` alongside the existing `conversation_only: bool`, same threading; consumed at `http_routes.rs:7065-7129` to persist the queued mid-run message under the same scope.

### 2.4 session-core changes

None required for storage/immutability if §2.1's orthogonal, per-turn, metadata-only design is accepted — `retrieval_scope` rides in `messages.metadata` (already a JSON column, already written per-message at `grpc.rs:2362-2365`) exactly like `source_scope` does today, but **without** the immutability check at `:2367-2389` (that check must stay conditioned on the `conversation` value only, not extended to the new values). One new line at `grpc.rs` near `:2388`: pass through `metadata_json["retrieval_scope"]` verbatim from the request if present, no validation beyond `normalize.rs`'s enum check already done upstream in model-gateway.

`memory_grpc.rs:461-467` / `dreaming.rs:142,216-219` / `grpc.rs:2453`: **unchanged** if the recommended design (memory/dreaming keyed only off true `conversation` scope) is adopted — flagged as owner decision #2 in §6.

### 2.5 Gateway BFF (Rust)

`apps/gateway/src/domains/chat/history.rs:518,629`: add `retrieval_scope: Option<String>` to `CanonicalMessage` and relay it into the wire turn shape (`turn["retrievalScope"] = ...`) the same one-line way `source_scope` is relayed today. `support.rs:28-32` unaffected (support-thread logic already keys off `conversation_only` only).

## 3. Tests to add

- `src/shared/api/chat-client.test.ts`: new `describe('buildChatWireBody — retrieval scope')` block next to the existing conversation-only test (`:469-470`):
  - `retrieval_scope` omitted when `retrievalScope` is `'auto'`/undefined (byte-identical default, mirroring the `effort`/`verbosity` omission tests already in this file).
  - `retrieval_scope: 'knowledge_only'` / `'web_only'` present when set.
  - `retrieval_scope` forced absent when `sourceScope: 'conversation'` even if `retrievalScope: 'web_only'` was also passed (conversation wins).
- `src/features/dashboard/home/DashboardComposer.test.tsx` (new or existing composer test file): the new picker disables/re-enables Browse-web and Deep-research per scope; resets to `'auto'` when the user switches to "Kun samtalen"; submit payload carries the right value.
- `src/features/chat/components/chat-normalizers.test.ts`: `normalizeCitation` passes through `origin`.
- `src/features/chat/components/use-chat-controller.test.ts`: `retrievalScope` is per-turn (not pinned to the first user turn, unlike `sourceScope` — a test that explicitly asserts the two behave differently is worth adding since it is the crux of the design).
- Rust, `model-gateway/src/tool_loop.rs`: extend the existing `conversation_tools_exclude_workspace_and_external_sources` test (`:10018-10025`) pattern with two new tests, `knowledge_only_tools_exclude_web_class` and `web_only_tools_exclude_org_class`, asserting `tool_allowed_for_retrieval_scope` against the same tool-name list.
- `model-gateway/src/sse.rs` (or wherever its integration tests live, alongside `e2e_invoke_chain_test.rs`'s existing `run_conversation_only_turn` helper at `:5414`): add `run_knowledge_only_turn`/`run_web_only_turn` helpers and assert: (a) the offered tool list excludes the wrong bucket, (b) a hallucinated wrong-bucket tool call still gets `"Tool is not available for this turn's source scope."` from the `tool_loop.rs:6815` gate, (c) `deep_research: true` + `knowledge_only` → `source_scope_conflict` error, (d) citations carry the right `origin`.
- `model-gateway/src/sse_events.rs`: extend citation serialization tests (near `:1071,1123`) to cover the new `origin` field.
- session-core `grpc.rs`: a test confirming a thread's `retrieval_scope` can differ turn-to-turn (no immutability error), contrasted with the existing immutability test for `source_scope`/`conversation`.
- `apps/gateway/src/domains/chat/history.rs`: extend the existing `CanonicalMessage` fixture-based tests (`:960-1149`) with a `retrieval_scope: Some("web_only".into())` case asserting `turn["retrievalScope"]` appears.

## 4. Rollout

- **Rebuild required:** model-gateway (new field, new gating logic, new SSE event, new citation field), session-core (only if metadata passthrough line is added — trivial, but it's a different binary and needs its own rebuild/restart), gateway BFF (`apps/gateway`, history relay change), frontend (composer, controller, chat-client, normalizers, ChatMessages/ChatPanels). Per the memory note, use the launcher (`--from model` or the frontend-appropriate target) rather than hand-built `docker compose build`, and restart the frontend container afterward since Vite-in-container watching is disabled.
- **No DB migration** under the recommended design (§2.1) — `messages.metadata` already exists and is schemaless. If the owner instead picks thread-level locking for the new axis (§6, decision #1), a new migration widening `threads.source_scope`'s CHECK constraint (or a second column) becomes necessary, plus updating `grpc.rs:2367-2389`'s immutability logic to be scope-value-aware rather than a single boolean comparison.
- **No feature flag needed for the mechanism itself** — the wire field is optional and defaults to `'auto'`, so this ships dark until the composer UI lands; the composer change and the gateway change can ship independently in either order without breaking the other (an old client omitting the field behaves exactly as today; a new client sending the field against an old gateway is silently ignored by `#[serde(default)]`).
- Order of rollout to avoid a window where the UI offers a control the backend ignores: land model-gateway + session-core + BFF first (dark, unused), then the frontend composer/controller/citation-origin/receipt changes.

## 5. Risks and edge cases

- **Tool-bucket ambiguity is the main correctness risk.** `mcp_call`, `action_execute`, `browser_agent`, and arguably `inbox_search`/`inbox_get_conversation` don't cleanly fall into "web" or "org knowledge" — get this table wrong and either mode leaks a tool it shouldn't, or over-blocks a tool users expect (e.g. blocking `inbox_search` under `web_only` is probably right; leaving `mcp_call` unrestricted under both new modes may surprise a user who picked "web only" expecting *nothing* org-connected to run). This must be resolved before implementation (see §6.1).
- **`web_only` forcing `web_search_available = true` unconditionally** bypasses the existing Search-toggle/auto-signal gate (`sse.rs:1577`) that today also feeds paid-provider entitlement logic (`paid_providers_allowed`, `tool_loop.rs:6115`) — verify this doesn't silently grant paid search providers to every `web_only` turn regardless of tier/ZDR, since that entitlement check is tier-based, not scope-based, and the two are currently orthogonal.
- **Citations origin correctness depends on tagging every emission site correctly** (`deep_research.rs:1962,1971`; `sse.rs:1802,3031,7702`; `tool_loop.rs:5418,5789,10521`) — miss one and a `knowledge_only` turn could silently emit an unlabeled citation that looks web-sourced in the UI, undermining the "reflected in citations" guarantee the whole item exists to provide. Worth adding a defensive assertion server-side (reject/relabel a citation whose bucket disagrees with the active scope) rather than trusting every call site to be updated correctly by hand.
- **Deep research under `web_only` still runs the full multi-round pipeline** (plan → search → read → synthesize, `deep_research.rs`) — confirm nothing in that pipeline unconditionally also calls `knowledge_search` for cross-referencing (a quick grep found no evidence of this, but the pipeline is large and wasn't fully read here).
- **The system-scope notice competes for prompt budget** with the assembled-context block exactly as `CONVERSATION_SCOPE_NOTICE` already does — no new risk, but worth confirming token-budget accounting (`compaction.rs`) treats the two new notices identically.
- **Composer UX regression risk:** adding a second scope control next to the existing one in an already-dense popover (`:2678-2696`) needs care not to make "Kunnskap + nett" vs "Kun organisasjon" vs "Kun nett" vs "Kun samtalen" read as four independent axes when only the last is actually a different (privacy) axis — the two should be visually grouped as "retrieval filter" vs. "conversation isolation" so users don't expect `web_only` to also isolate memory/retention.
- **Mid-run queued input**: a message queued mid-stream under one retrieval scope and delivered into a run that started under a different one (if the user changes the picker mid-stream) needs an explicit rule — recommend: queued input always inherits the *stream's* scope (as it already does for `conversation_only`), and the picker's mid-stream change only takes effect on the *next* turn, not the in-flight one.

## 6. Genuine owner decisions

1. **Thread-locked vs. per-turn for the new axis.** Recommended design (§2.1) makes `knowledge_only`/`web_only` freely changeable per turn, stored in message metadata, no migration. The alternative — locking them at the thread level like `conversation` — is more consistent with today's single mental model but requires a DB migration and reworking the immutability check into a richer comparison, and contradicts "for a chat turn" in the item's own title. This changes whether §2.4's "no migration" claim holds.
2. **Should `knowledge_only`/`web_only` also suppress memory writing and dreaming/learning**, the way `conversation` does today (`memory_grpc.rs:465-467`, `dreaming.rs:216-219`)? Recommendation here is no — memory is about what the user said, independent of which sources grounded the answer — but this is a privacy-posture call, not an engineering one.
3. **Tool-bucket assignment for `mcp_call`, `action_execute`, `browser_agent`, `inbox_search`, `inbox_get_conversation`, `result_query`.** Proposed default: the three connector/execution tools are left unrestricted by this filter (org-only and web-only both leave them available); inbox/result-query are bucketed as org-class. Confirm, since getting this wrong either over-blocks (frustrating) or under-blocks (undermines the "enforced, not prompt text" claim for a mode marketed as "web only").
4. **Should `web_only` force web-search tool availability unconditionally**, bypassing the existing Browse-toggle/auto-signal gate (as proposed), or should the user still need to also flip Browse — i.e. is `web_only` a *ceiling* (removes org access, web still opt-in per turn) or a *floor* (guarantees web access every turn)? This affects one line at `sse.rs:1577` but changes the feature's actual behavior materially.
5. **Should `deep_research` be blocked outright under `knowledge_only`** (proposed: yes, same error class as the existing `conversation_only` conflict guard) or degraded to a knowledge-only research mode that doesn't exist today? Proposed default is the simpler outright block.

## 7. Size and dependencies

**Size: M.** Not S, because it touches four services (model-gateway, session-core, gateway BFF, frontend) across ~15 concrete file locations with a new enum threaded through request, tool-gating, citations, receipts and history replay; not L, because the recommended design deliberately avoids a DB migration, avoids touching the delicate thread-immutability code, and reuses an already-proven enforcement chokepoint (`tool_loop.rs:6815`) rather than inventing a new one.

**Dependencies on other Wave-2 items** (from `CHAT_PAGE_GAP_PLAN_2026-09-28.md` §4):
- **Inline citation chips (SSE producer)** is listed as its own Wave-2 item and shares the exact same file (`chat-normalizers.ts:1295-1303`, `sse_events.rs` `Citation` variant) — the `origin` field this spec adds should land in the same PR/commit as that item's chip work, or the two will conflict on the same struct twice. Sequence: do the citation-origin field as part of *this* item first (it's a small, additive change), then the chip-rendering item consumes both `claimId`/`sourceGroupId` (already there) and the new `origin` together.
- **The attach-from-connector picker** and **persona instructions** (both Wave-2) are independent of this item — no shared files found.
- **Raising the subagent ceiling** (Wave-2) is independent.
- No dependency on Wave-3/4 items.