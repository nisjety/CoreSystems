# EP-4 - Inline outputs and MCP Apps (Wave 3 design)

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `SC` = `MPR/services/session-core`; `MPC` = `MPR/crates/mp-contracts/src`; `PROTO` = `MP/proto/model_plane/v1`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`. Every `path:line` was opened in worktree `epic-shaw-4d613b` at HEAD `f1c8c2450` (branch `model-plane`) unless marked otherwise. `[INF]` = inference, `[verify live]` = only a running stack can show it, `[verify at slice start]` = a fact the building agent must re-check before editing. Plan source: `docs/plans/model-plane-chat-build-plan-2026-10-14.md` section 3.3.2 "EP-4" (lines 1556-1564) and the epic row (line 1506). Sibling designs: `EP-3-artifact-canvas-and-composer.md` (prerequisite), `EP-10-connector-suggestions-and-write-confirmation.md` (leaves elicitation and widgets to this epic, its line 37), `00-integration-round-a.md`, `00-integration-round-b.md` (numbering rules, hot files).

## 1. Scope, items, owner

**Items in scope.**
- F-t3-044 "Inline interactive outputs" (audit M): HTML/SVG widgets drawn inside the transcript, in the opaque-origin sandbox, with host theme tokens and a narrow postMessage bridge.
- F-t3-045 "MCP Apps, elicitation forms, hosted artifact runtime" (audit L): MCP `elicitation/create` forms reusing the WS14 question pause; MCP Apps `ui://` resources inside the existing sandboxed frame, JSON-RPC over postMessage; every app-initiated call attributed to the viewer.

**Owner.**
- Frontend Plane: the widget fence renderer, the frame and its bridge, the MCP App host, the question card's origin line, action contracts, BFF pass-through routes.
- Model Plane: model-gateway owns the MCP transport (`MG/mcp_jsonrpc.rs`, `MG/mcp_http.rs`), the held elicitation calls, the `ui://` resource read and the widget prompt notice; execution-core owns the governed MCP call (`EC/mcp_gateway.rs`, `EC/runtime_loop/mod.rs` `execute_mcp`) and the run pause; session-core owns `run_questions` (origin columns).
- Ingestion Plane: no change in W3. The plan's "attributed image results" strip is OUT (below).
- Application Plane and Control Plane: no change in W3. They enter only if a dedicated user-content origin is approved (DEC-40); S11 writes the ADR stub that defers it.
- Channel Plane: docs-only; nothing here runs against it.

| Slice | Tag | In |
|---|---|---|
| S1 | [BUILD] | Bridge-less static widget: a closed ```` ```widget ```` fence in an assistant answer drawn in the opaque-origin sandbox with host theme tokens (FE) |
| S2 | [BUILD] | Widget prompt notice and the `inline_widgets` client feature (gateway `format_guidance`, one FE feature line) |
| S3 | [BUILD] | Narrow postMessage bridge: size, staged follow-up, safe link; rate limits (FE) |
| S4 | [BUILD] | MCP elicitation transport in the gateway: protocol revision, streaming read, held call, answer service, schema-to-question mapping |
| S5 | [BUILD] | Question origin contract: `run_questions` origin columns, proto fields, session-core store |
| S6 | [BUILD] | Origin on the `user_question` event and the question card ("asked by <server>, the answer goes to <server>") |
| S7 | [BUILD] | Elicitation in the governed loop: execution-core asks through a `resume` question, pauses, answers the held call on resume |
| S8 | [BUILD] | MCP Apps resource read: `ui://` discovery and `resources/read` behind a viewer-checked gateway route + BFF pass-through |
| S9 | [BUILD] | MCP App host in the frame: JSON-RPC over postMessage, tool input/result to the app, staged messages (FE) |
| S10 | [BUILD] | App-initiated `tools/call`: viewer-bound, same server, read class only, through `ExecuteStep` and the signed gate |
| S11 | [BUILD] | ADR stub: a dedicated user-content origin is deferred (docs only) |
| S12 | [FINISH] | Flip switches on the proof org, ledger, ZDR rows, ownership row, live proof |

**Explicitly OUT.**
- A dedicated user-content origin, viewer-bound connectors for apps, an app key-value store, hosted artifact runtime with network access (S11 ADR stub defers all four; DEC-40).
- `sampling/createMessage` (the server asking our model to infer): never advertised in W3 (D15).
- Selection-to-context from a widget and the attributed image result strip (Ingestion): separate later pieces per the plan's first-slice note; not designed here.
- MCP tools in the Ask loop: they stay refused there (`MG/tool_loop.rs:4671-4686`); EP-4 works on the governed (Do) path only.
- MCP writes from an app (D13) and approvals for app-initiated calls: wait for resumable MCP approval (DEC-12 `MCP_DO_TOOLS=all` gate).
- Widgets on the Space room surface, in a person's own posts, in exports and in shared transcripts (D2).
- Long-lived or pooled MCP sessions (D5).

## 2. Verified current state

**The sandbox frame (reused, not replaced).**
- `FES/shared/lib/sandbox-html.ts:10-34`: `PREVIEW_CSP` (`default-src 'none'`, `connect-src 'none'`, `form-action 'none'`, `frame-src 'none'`, `img-src data: blob:`, `script-src 'unsafe-inline'`, `worker-src 'none'`) and `sandboxHtmlDocument(html)`, which always writes its own head first (34 lines, LF).
- `FES/features/chat/components/ChatArtifactPanel.tsx:548-574` `ArtifactHtmlPreview`: `srcdoc` iframe with `sandbox="allow-scripts"`, never `allow-same-origin`, `referrerpolicy="no-referrer"`, documented as a security requirement. `ChatAttachmentCanvas.tsx:176-180` uses the same pair.
- `FES/shared/artifacts/viewer-registry.ts:1-60`: trust class `untrusted_markup` has exactly one renderer, the opaque-origin `srcdoc` frame; the room surface needs a per-artifact click (`requiresActivation`). No bridge exists anywhere: a grep of `FES` for `postMessage` handlers on these frames finds none, and no frame listens to a `message` event.

**Where an answer's markdown is drawn.**
- `FES/features/chat/components/ChatMessages.tsx:1324` `ChatMarkdown`, `:1428-1443` `MarkdownCodeBlock`: a closed ```` ```chart ```` fence becomes `ChartFenceBlock`, a closed ```` ```mermaid ```` fence becomes `MermaidDiagram`, anything else a code block; a fence still streaming is always a code block. This is the seam for S1 (4,871 lines, LF).
- `ChatMarkdown` is also used by the Space room for model turns and for people's posts (`FES/features/spaces/components/sidepane/SpaceTurnList.tsx:455,477`; `SpaceRoomComposer.tsx:1202,1634`). A widget branch keyed only on the fence would therefore run script from a person's room post in every other member's browser. S1 must make the live widget opt-in per surface (D2).
- Follow-up chips already call `ctx.onSelectFollowUp(text)` (`ChatMessages.tsx:375-381`, `FollowUpChips` at `:2129-2140`); S3 reuses that handler for staged widget follow-ups.
- Safe links: `isSafeLinkHref` in `FES/features/chat/lib/safe-href.ts`, used by `chat-media-markdown.tsx:41,1249` (WS26-S3, done).
- FE switches are read as the literal `'true'` (`ChatMessages.tsx:234` `import.meta.env.VITE_CHAT_RENDER_LEGACY === 'true'`).

**Prompt guidance and client features.**
- `MG/format_guidance.rs:1-18,153-170`: per-turn notices (today the chart fence); `message_for_turn(user_text)` returns `None` on ordinary turns, so the prompt is unchanged; wired at `MG/sse.rs:3206-3209` (352 lines, CRLF).
- Client feature families: `FES/shared/api/chat-client.ts:1307-1314` `DEFAULT_FEATURES` (`usage`, `citations`, `reasoning`, `steps`, `artifacts`, `memory`), sent as `features` (`:1726`); the gateway tests membership (`MG/sse.rs:1693,3248,3264`).

**The MCP client.**
- `MG/mcp_jsonrpc.rs:17-18`: `MCP_PROTOCOL_VERSION = "2024-11-05"`; `:32-43` `initialize` sends `"capabilities": {}`. That revision has no elicitation; elicitation needs `2025-06-18` [verify the revision string against the MCP specification at slice start].
- `MG/mcp_jsonrpc.rs:155-165` `McpToolDef{name, description, input_schema_json, annotations}`; `:170` `TOOL_DIGEST_VERSION = 1`; `:345-362` `parse_one_tool` reads `name`, `description`, `inputSchema`, `annotations` and ignores `_meta`. There are 14 struct-literal sites of `McpToolDef {` in the services, so adding a field ripples (S8 avoids that, D11).
- `MG/mcp_http.rs:42-52`: session header `mcp-session-id`, fixed request ids 1/2/3 ("each session issues at most one of each call"); `:107-140` `connect` opens a session; `:142-183` `post` reads the **whole** response body through `bounded_mcp_response` (`MG/runtime_registries.rs:592-613`) before parsing. A server that sends `elicitation/create` inside the `tools/call` SSE stream and waits for the answer would therefore hang this read until the transport times out. `:207-220` `call_tool`.
- `MG/runtime_registries.rs:2822-2943` `handle_proxy_mcp_tool`: per-user ownership check, enabled flag, allowlist, HTTPS-only transport, WS05-S2 digest re-check, then a **fresh session per call** (`:2920`) and one `call_tool` (`:2930`). No `ui://`, `resourceUri`, `resources/read`, `elicitation/create` or `sampling/createMessage` anywhere in `MG` (grep), as the plan says.
- `MG/tool_loop.rs:4671-4686`: an `mcp__` name in the Ask loop is refused ("requires governed agentic execution and approval").
- `EC/mcp_gateway.rs:1-27`: execution-core reaches MCP only through the gateway's `ProxyMcpTool`; `:63-69` the channel has a 2 s connect and **8 s request timeout**. `EC/runtime_loop/mod.rs:2300-2330` `execute_mcp` refuses under ZDR and waived routes with `feature_not_supported("mcp_execution")` before any network, parses `mcp__<server>__<tool>`, and carries the signed class and pinned digest.
- `PROTO/gateway.proto:146` `rpc ProxyMcpTool`; `:1241-1264` `ProxyMcpToolRequest` fields 1-7 used (next free **8**), `ProxyMcpToolResponse` fields 1-3 used (next free **4**). Same on `main`, `space-w2`, `mp-w2w3`, `w23/integ` (checked with `git show`). `service ModelGateway` has 50 RPCs; adding one would force every implementation and test double to change, so S4 uses a new service in its own file (the `RunQuestionConsumeService` precedent, `PROTO/orchestration.proto` near `:2080`).

**The question primitive (WS14).**
- `PROTO/orchestration.proto:131-139` `CreateRunQuestion`, `ListRunQuestions`, `AnswerRunQuestion`; `:1963-1974` `QuestionItem{id, kind: single|multi|text, prompt, options, allow_other}` ("A secret or credential kind does not exist"); `:1990-2015` `RunQuestion` fields 1-14 (next free **15**); `:2020-2035` `CreateRunQuestionRequest` fields 1-7 (next free **8**). Same on the four other branches. File is LF.
- `MPC/user_question.rs:16-38`: `MAX_ITEMS = 4`, `MAX_OPTIONS = 6`, `MIN_OPTIONS = 2`, `MAX_TEXT = 2000`, `MAX_QUESTIONS_PER_RUN = 3`, `DELIVERY_NEXT_TURN`, `DELIVERY_RESUME`, `RESUME_TTL_SECS = 3600`.
- `SC/migrations/0054_run_questions.sql:21-55`: one row per question, `delivery IN ('next_turn','resume')`, `spec JSONB`, `answers JSONB`, erased with the run (`ON DELETE CASCADE`). No column says who authored the question: every question reads as the model's.
- `EC/runtime_loop/mod.rs:4053-4120` `execute_ask_user`: refused under ZDR; creates a `next_turn` question with idempotency `{run_id}:{step_id}`. `delivery = resume` is accepted by session-core (`SC/src/orchestration_grpc.rs:2544`, `ConsumeRunQuestion` at `:2648-2720`) but execution-core never sends it: queue row WS14-S5 is `partial`, remaining "the executor input pause and apply on resume ... then create_run_question accepts delivery resume". S7 depends on it.
- The card: `FES/shared/chat-nodes/QuestionCard.tsx:24-40` (answers are information, never approval; "asks for no credentials"). The `user_question` SSE event: `MG/sse_events.rs:374-379` (`question_id`, `delivery`, `expires_at`, `items`), emitted at `MG/sse.rs:13494-13509` from `crate::http_routes::run_question_value`.

**Single-step execution (for app-initiated calls).** `EC/grpc.rs:1419` `execute_step` authorises the delegated caller and the run owner (`authorize_run`), refuses reserved owner actions (`reject_direct_owner_action`, `:1286-1301`), requires an explicit `permission_mode` (`:1306-1313`) and refuses persistence-free callers (`enforce_persistence_free_execution(..., "tool_execution")`). The gateway already calls it for `code_interpreter` (`MG/tools.rs:291`). Whether its executor routes an `mcp__` name through `GovernedMcp` with the signed class is [verify at slice start] (S10).

**ZDR keys available.** `MP/api/contracts/zdr-refusals.json` already has `mcp_execution` ("MCP tool execution"), `tool_execution`, `client_tools`. No new key is needed.

**Ownership rows.** `MP/docs/capability-ownership-matrix.md:66-67`: per-tool class and MCP tool digests are capability-core's; model-gateway "digests and posts, never decides".

**Prerequisite status (queue `.plan-work/execution/queue.json`).**

| Prerequisite | Status | What EP-4 needs from it |
|---|---|---|
| EP-3-S1 per-thread drafts | done | S1 may start (plan: "S after EP-3 slice 1") |
| WS26-S3 safe links | done | S3 link opening |
| WS05-S4 MCP in Do runs | done (live proof pending) | S4, S8 |
| WS05-S2 digest pin | partial | S8/S10 read the same digest path; not blocking code |
| WS05-S6.code enforce | partial | S10 relies on the signed class being enforced; enable waits |
| WS14-S3 question store | done | S5 |
| WS14-S4 card and event | partial | S6 |
| WS14-S5 resume delivery | partial (exec half missing) | S7 cannot finish without it |
| EP-3-S2..S4 revision store | blocked-decision | not needed: widgets live in the answer text, not in the artifact store |

**Audit staleness.** (1) The plan's "first slice reuses the opaque-origin sandbox" holds; the sandbox has no bridge yet, so "bridge-less" is today's real state. (2) The plan does not mention that `ChatMarkdown` is shared with the room and with people's posts, which drives D2. (3) "Opens a fresh session per call" holds (`runtime_registries.rs:2920`); the plan does not mention that the transport reads whole bodies (`mcp_http.rs:142-183`), which makes streaming a prerequisite for elicitation. (4) The plan says elicitation reuses the WS14 pause; the `resume` half of that pause is not built in execution-core yet.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | How a widget reaches the transcript | (a) a dedicated ```` ```widget ```` fence (HTML or SVG body) in the answer; (b) an `html` artifact with an inline display hint; (c) every ```` ```html ```` fence | **(a).** Same pattern as the `chart` and `mermaid` fences, no new SSE event, no new store (the source is the answer text the thread already keeps). (c) would turn every HTML code sample into running script. (b) belongs to EP-3's artifact store, which is decision-blocked | S1 | Yes (default) |
| D2 | Where a widget runs | chat assistant answers only / also room / everywhere `ChatMarkdown` draws | **Chat assistant answers only.** A Solid context `InlineWidgetSurface` defaults to `inert`; only `AssistantMessage` on the chat page provides `live`. The room, people's posts, user messages, exports and shared views keep the code block. A live room widget would run one person's script in another person's browser | S1 | Yes |
| D3 | Follow-ups a widget asks for | (a) sent at once as a visible user turn, rate-limited; (b) staged as a chip the person clicks (same handler as follow-up chips); (c) none | **(b)** with rate limits: at most one staged chip per widget at a time, 5 per answer per minute, text <= 500 chars, shown as "Forslag fra widgeten: ..." The plan's "visible, rate-limited user turns" is met and the person confirms each one, so a widget can never loop the model. Ledger entry records the tightening | S3, S9 | Yes |
| D4 | Bridge contract | free-form / allowlisted envelope | Envelope `{v:1, type, id?, payload}`; host accepts only `event.source === frame.contentWindow` and `event.origin === 'null'`; types `ready`, `size` (height clamped 80-1200 px), `followup`, `open_link` (through `isSafeLinkHref` and the existing external-link confirmation); payload <= 8 KiB; unknown types dropped and counted; host to frame only theme tokens (S3) and, for MCP Apps, tool input/result (S9). No storage, no network, no host data | S3 | Yes |
| D5 | MCP session lifetime | per call (today) / per call extended while an elicitation waits / pooled per user | **Per call, extended only while an elicitation is pending**, up to `MCP_ELICITATION_HOLD_SECONDS` (default 600, clamped 60-1800, always below `RESUME_TTL_SECS` 3600). Pooled sessions would need per-user token custody and replica routing (WS04) for no W3 gain | S4 | Yes |
| D6 | Protocol revision and capabilities | always upgrade / upgrade only when elicitation is allowed | **Only when allowed.** With `ProxyMcpToolRequest.allow_elicitation` false (default, and always under ZDR) the client sends `2024-11-05` and `{}` exactly as today; with it true it sends the elicitation revision and `{"elicitation":{}}`. A server answering an older revision simply never elicits | S4 | Yes |
| D7 | What carries the pause | (a) a WS14 `resume` question, run paused, held call answered on resume; (b) a `next_turn` question and a retry; (c) always decline | **(a).** A retry would re-run the server's work and elicit again. (c) is what happens while the switch is off. Needs WS14-S5's executor pause | S7 | S4, S5, S6 yes |
| D8 | Which forms are accepted | anything / the MCP flat schema mapped onto `QuestionItem` | **Map, else decline.** `string` -> `text`; `string` with `enum` (2-6) -> `single`; `boolean` -> `single` (Ja/Nei); `number`/`integer` -> `text` validated as a number at answer time; `format` email/uri/date -> `text` validated; <= 4 properties (`MAX_ITEMS`). Anything else (nested, arrays, > 6 options) is declined to the server and the model reads "form declined: not supported". Property names or titles matching `password|passord|secret|token|api[_-]?key|pin|cvv|card|kort|personnummer|fødselsnummer` are declined (the spec forbids requesting sensitive data; the card has no secret kind) | S4 | Yes |
| D9 | Who may answer, which runs may elicit | owner / any Space member | **The run owner only** (the `run_questions` rule); elicitation is advertised only for depth-0, non-ZDR, non-waived, parkable runs that are not Space-scoped in W3 | S7 | Yes |
| D10 | How an MCP App's HTML reaches the browser | (a) the SPA asks the gateway by tool name; the gateway re-discovers the `ui://` uri and reads it with the viewer's authority; (b) execution-core ships the HTML on run events; (c) persist resources | **(a).** No proto change, no new run-event payload, nothing persisted; the gateway re-checks ownership, enabled flag and allowlist on every read, exactly like `handle_proxy_mcp_tool` | S8 | Yes |
| D11 | Is the `ui://` uri part of the tool digest | include now (changes `TOOL_DIGEST_VERSION`, re-approval of every MCP tool) / not in v1 | **Not in v1.** The app has no ambient authority (opaque origin, `connect-src 'none'`, calls go through S10's gate), so a changed uri changes only what is drawn. Revisit when WS05 bumps the digest version. Needs a security acknowledgement | S8 | Yes (security nod before S12 flip) |
| D12 | App CSP | server-declared domains / host CSP unchanged | **Unchanged** (`PREVIEW_CSP`). An app that declares network domains [verify the `_meta` key against the MCP Apps extension at slice start] shows "Denne appen trenger nettverkstilgang, som ikke er tilgjengelig her" and still renders | S8, S9 | Yes |
| D13 | App-initiated `tools/call` | none / read class only / any class with approval | **Read class only** (`none`/`read` in the registry's class for that tool), same server as the app, by the run owner, through `ExecuteStep` with the viewer's delegated bearer and `permission_mode = ask`, at most 10 calls per frame per minute. Anything else answers `app_call_requires_chat` and the app may stage a message instead. Matches `MCP_DO_TOOLS=read` (DEC-12) | S10 | S1-S9 yes |
| D14 | A dedicated user-content origin | build now / defer | **Defer** (DEC-40 recommendation; S11 ADR stub). Revisit with EP-12's share model | S11 | Yes |
| D15 | `sampling/createMessage` | support / never in W3 | **Never in W3**: not advertised; a server that sends it gets a JSON-RPC `-32601` | S4 | Yes |
| D16 | Replica routing of a held elicitation | sticky routing / fail closed | **Fail closed**: an answer that reaches a gateway replica without the held call reads `mcp_elicitation_expired`, and the model is told. Today's compose runs one gateway [verify live]; WS04 owns replica-safe MCP | S4, S7 | Yes |

Ledger entries this epic adds (newest first, `MP/docs/decisions/ledger.md`, CRLF): "Widgets are a fenced block in the answer, live only in chat answers" (D1, D2); "Widget and app follow-ups are staged, never sent by the widget" (D3, tightens the plan wording); "MCP sessions stay per call; an elicitation holds one call at most `MCP_ELICITATION_HOLD_SECONDS`" (D5, D6, D16); "Elicitation is a resume question with a server origin; sensitive fields are declined" (D7, D8, D9); "MCP App HTML is read on demand with the viewer's authority and never stored" (D10, D11, D12); "App-initiated calls are read class only in W3" (D13). Nothing in the ledger is reversed.

No plan `[RECONCILE]` is assigned to EP-4. The integration rounds name no EP-4 conflict; EP-10 explicitly leaves elicitation and widgets here.

## 4. Slices

Commands used below: Rust `cd "apps/Model Plane/rust" && cargo test -q -p <crate> --lib <filter>` (two cargo targets fail pre-existing on Windows, so `--lib`); `cargo fmt --all -- --check`; SSE taxonomy `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; ZDR contract `bash scripts/tests/zdr-contract-sync.test.sh`; session-core DB tests through the existing harness (memory: Docker recipe); proto `buf lint` / `buf breaking --against proto/baseline.binpb` in `MP/proto` (ratchet, pre-existing lint debt); FE from `FE`: `pnpm exec vitest run <files>`, `pnpm lint`, typecheck with `tsc -p tsconfig.app.json` diffed against HEAD's failing set; BFF `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`. Per the owner's 2026-11-03 order, the building agent writes code and tests and runs none of these; the guard agent compiles the batch.

### S1 - Bridge-less static widget [BUILD]
**Goal.** A closed ```` ```widget ```` fence in a chat assistant answer is drawn inline as a live, sandboxed widget with the host's theme, while every other surface keeps the code block.
**Files.**
- `FES/features/chat/lib/inline-widget.ts` - NEW, pure. `WIDGET_FENCE = 'widget'`, `MAX_WIDGET_CHARS = 100_000` (over it: code block plus a one-line note), `isWidgetFence(block)`, `widgetThemeCss(getVar)` that reads a fixed list of host tokens (background, surface, text, muted, accent, border, radius, font family, font size) through `getComputedStyle(document.documentElement)` and writes `:root{--vv-...}`; values are filtered to `[#a-zA-Z0-9(),.%\s-]` so a token cannot close the style element.
- `FES/shared/lib/sandbox-html.ts` - additive `sandboxWidgetDocument(html, themeCss)`: same `CSP_META` first, then `<style>` with the theme and a base reset, then the body. `sandboxHtmlDocument` stays byte-identical (its test pins it).
- `FES/features/chat/components/InlineWidget.tsx` - NEW. `InlineWidgetSurface` context (`'live' | 'inert'`, default `inert`) and `InlineWidget` (`<iframe sandbox="allow-scripts" srcdoc=... referrerpolicy="no-referrer" loading="lazy">`, fixed height 360 px, title from the first `<title>`/`aria-label` or "Interaktiv widget", toolbar "Vis kode" / "Åpne i panel" (opens the existing `ArtifactHtmlPreview` path with the same source, no new artifact)). Never `allow-same-origin`, `allow-forms`, `allow-popups` or `allow-top-navigation`; a test pins the attribute string.
- `FES/features/chat/components/inline-widget.css` - NEW, imported by `InlineWidget.tsx` (pattern of `SettledTurnFold.tsx:18`); tokens only, no raw colours.
- `FES/features/chat/components/ChatMessages.tsx` - `MarkdownCodeBlock` (`:1428-1443`) gains a third branch before the code block: closed fence, lang `widget`, `useContext(InlineWidgetSurface) === 'live'`, switch on. `AssistantMessage` (`:479`) wraps its answer in `<InlineWidgetSurface.Provider value="live">` only when the switch is on.
**Contract/proto/migration.** None. No SSE change.
**Flag.** `VITE_CHAT_INLINE_WIDGETS`, literal `'true'` only, default off. Off: the `widget` fence is an ordinary code block (byte-identical render). Rollback: switch off.
**ZDR.** No new store: the widget source is the answer text, persisted only where the answer already is (never on ZDR threads). No snapshot, no thumbnail, no storage inside the frame (opaque origin has none).
**Tests.** `inline-widget.test.ts` (`a_closed_widget_fence_is_a_widget`, `a_streaming_fence_is_not`, `html_and_svg_fences_are_not_widgets`, `over_cap_is_refused`, `theme_values_cannot_close_the_style_element`); `sandbox-html.test.ts` (`widget_document_puts_csp_before_theme`, `preview_document_is_unchanged`); `InlineWidget.test.tsx` (`sandbox_is_allow_scripts_only`, `no_same_origin_ever`, `show_code_toggles`, `title_falls_back`); `ChatMessages.inline-widget.test.tsx` (`assistant_answer_draws_the_widget`, `user_message_keeps_code`, `room_chat_markdown_keeps_code` (renders `ChatMarkdown` without the provider), `switch_off_keeps_code`). `pnpm exec vitest run src/features/chat/lib/inline-widget.test.ts src/shared/lib/sandbox-html.test.ts src/features/chat/components/InlineWidget.test.tsx src/features/chat/components/ChatMessages.inline-widget.test.tsx`.
**Live proof.** Second FE dev server, switch on: an answer with a hand-written widget fence (slider + SVG) renders at 1280 and 375 px in light and dark; DevTools shows `origin: null`, `document.cookie` throws, `fetch('/api/v1/me')` blocked by CSP; the same text posted in a Space room shows code [verify live].
**Effort.** M, 3-4 d. **Deps.** EP-3-S1 (done), WS26-S3 (done). **Risks.** `ChatMessages.tsx` is a hot file (round A/B: EP-14 S1-S3 restructure it; land after them or rebase); `srcdoc` iframes cost memory in long threads (lazy loading; S1 draws at most 8 live widgets per thread view, older ones show "Vis widget" until clicked).

### S2 - Widget notice and client feature [BUILD]
**Goal.** On a turn that asks for something interactive, a client that can draw widgets tells the gateway so, and the model is told the fence and its limits.
**Files.**
- `MG/format_guidance.rs` - `widget_notice(user_text) -> Option<&'static str>` (request verb within `MAX_WORDS_BETWEEN` of `kalkulator|calculator|interaktiv|interactive|widget|simulering|simulation|skyveknapp|slider`), and `message_for_turn_with(user_text, widgets_offered: bool)`; `message_for_turn` stays as today and calls it with `false`. The notice text: one fence, self-contained HTML/SVG/inline JS, no network, no external scripts or fonts, no forms, <= 100,000 chars, theme variables `--vv-*`, explain the widget in prose too.
- `MG/sse.rs` - the one call at `:3209` passes `widgets_offered = switch && features.contains("inline_widgets")`. Switch read once into a `OnceLock<bool>` in `format_guidance.rs` (`MODEL_GATEWAY_WIDGET_GUIDANCE`, `trim() == "true"`, the `attachment_policy.rs:180` idiom).
- `FES/shared/api/chat-client.ts` - add `'inline_widgets'` to the sent `features` only when `VITE_CHAT_INLINE_WIDGETS === 'true'` (no change to `DEFAULT_FEATURES` itself).
**Contract.** New feature family name `inline_widgets` (request-side only; no event). **Flag.** `MODEL_GATEWAY_WIDGET_GUIDANCE` (gateway, default off) and the S1 FE switch. Both must be on. Rollback: either off.
**ZDR.** Prompt text only. A ZDR turn may receive the notice (it adds no store); [verify at slice start whether `zdr_direct_stream` assembles `format_guidance` at all].
**Tests.** `format_guidance::tests` - `widget_notice_needs_a_request`, `widget_notice_off_without_the_feature`, `chart_notice_unchanged`, `message_for_turn_is_unchanged_when_widgets_not_offered`; `chat-client.test.ts` - `inline_widgets_feature_only_with_the_switch`. `cargo test -q -p model-gateway --lib format_guidance`, `pnpm exec vitest run src/shared/api/chat-client.test.ts`.
**Live proof.** "Lag en lånekalkulator med skyveknapp for rente" -> a widget fence; same prompt with the switch off -> no notice in the prompt (prompt inspector) [verify live].
**Effort.** S, 2 d. **Deps.** S1. **Risks.** `sse.rs` and `chat-client.ts` are hot; one line each.

### S3 - Narrow postMessage bridge [BUILD]
**Goal.** A widget can size itself, propose a follow-up the person may send, and ask to open a safe link; nothing else crosses the frame.
**Files.**
- `FES/features/chat/lib/widget-bridge.ts` - NEW, pure. Envelope parser (D4), per-frame token bucket (5 follow-ups per answer per minute, 1 staged at a time, 20 size messages per second), `BRIDGE_SHIM` (a small inline script injected by `sandboxWidgetDocument` when the bridge is on: `window.verevon = {resize(h), suggest(text), openLink(url)}`, posting to `parent` with target `'*'`, which is correct for an opaque origin; the host's check is on `event.source`).
- `FES/shared/lib/sandbox-html.ts` - `sandboxWidgetDocument(html, themeCss, {bridge})` adds the shim after the CSP and theme, before the body.
- `FES/features/chat/components/InlineWidget.tsx` - one `message` listener per frame (removed on cleanup), source and origin check, `size` sets the frame height, `followup` stages a chip under the widget that calls the existing `onSelectFollowUp` handler (`ChatMessages.tsx:375-381`; [verify at slice start that the chat page passes it into the message's render context]), `open_link` goes through `isSafeLinkHref` and the external-link confirmation the transcript uses.
- `FES/shared/actions/action-registry.ts` (+ test) - `chat.stage_widget_followup` (ownerPlane `frontend`, risk `low`), `chat.open_widget_link` (risk `low`), added before wiring (repo rule).
**Flag.** `VITE_CHAT_WIDGET_BRIDGE` (literal `'true'`, default off; needs S1's switch). Off: no shim injected, no listener (S1 behaviour).
**ZDR.** Nothing stored; a staged follow-up becomes a user message under the thread's own retention when the person sends it.
**Tests.** `widget-bridge.test.ts` (`foreign_source_is_ignored`, `non_null_origin_is_ignored`, `unknown_type_is_dropped`, `oversized_payload_is_dropped`, `followups_are_rate_limited`, `height_is_clamped`); `InlineWidget.bridge.test.tsx` (`followup_is_staged_not_sent`, `clicking_the_chip_calls_onSelectFollowUp_once`, `unsafe_link_is_refused`, `listener_removed_on_unmount`); action registry test count updated. Same vitest command pattern.
**Live proof.** A widget calling `verevon.suggest('Vis for 25 år')` 20 times in a loop shows one chip; clicking sends one visible user turn; `verevon.openLink('javascript:...')` is refused [verify live].
**Effort.** M, 3-4 d. **Deps.** S1, WS26-S3. **Risks.** a widget can still mislead by its own text (it is model output); the chip says it comes from the widget.

### S4 - MCP elicitation transport in the gateway [BUILD]
**Goal.** When a governed MCP call allows it, the gateway advertises elicitation, reads the call's response stream frame by frame, and on `elicitation/create` returns a typed pending form while it holds the call; a later answer completes the same call.
**Files.**
- `PROTO/mcp_elicitation.proto` - NEW. `McpElicitationPending{elicitation_id, server_id, server_label, message, requested_schema_json, items (repeated QuestionItem, imported from orchestration.proto), expires_at_unix}`; `service McpElicitationService { rpc AnswerMcpElicitation(AnswerMcpElicitationRequest) returns (AnswerMcpElicitationResponse); }`; `AnswerMcpElicitationRequest{elicitation_id, org_id, user_id, action ("accept"|"decline"|"cancel"), answers (repeated QuestionAnswerItem)}`; `AnswerMcpElicitationResponse{output_json, error_message, elicitation (McpElicitationPending, set when the server elicits again)}`. Every message and field commented. Own service so `ModelGateway` implementations and doubles do not change.
- `PROTO/gateway.proto` (CRLF) - import the new file; `ProxyMcpToolRequest`: `// EP-4 S4: the caller allows the server to ask the run's owner a question during this call. False keeps today's protocol revision and empty capabilities.` `bool allow_elicitation = 8;` `ProxyMcpToolResponse`: `// EP-4 S4: set when the server asked a question and the call is held; output_json and error_message are then empty.` `McpElicitationPending elicitation = 4;`
- `MPR/crates/mp-contracts/build.rs` - add `model_plane/v1/mcp_elicitation.proto` to the list.
- `MPC/mcp_elicitation.rs` - NEW (+ `pub mod` line in `MPC/lib.rs`). Pure mapping both ways (D8): `items_from_schema(schema) -> Result<Vec<user_question::Item>, Decline>`, `content_from_answers(schema, items, answers) -> Result<serde_json::Value, Problem>` (numbers parsed, booleans from Ja/Nei option ids, formats checked), `SENSITIVE_FIELD` matcher. Shared by the gateway and execution-core.
- `MG/mcp_jsonrpc.rs` (CRLF) - `ELICITATION_PROTOCOL_VERSION`; `build_initialize_request_with(id, elicitation: bool)` (the existing function delegates with `false`); `parse_server_request(value) -> Option<ServerRequest>` for `elicitation/create` and, to refuse it, `sampling/createMessage`; `build_elicitation_result(id, action, content)`; `build_method_not_found(id)`.
- `MG/mcp_http.rs` (CRLF) - `connect_with(url, token, elicitation)`; `call_tool_interactive(...) -> Interactive{Done(McpCallOutcome) | Elicit{request_id, params, stream}}` that reads SSE frames from the response with the same byte cap as `bounded_mcp_response` counted across frames; `respond(&self, payload)` posts the client's JSON-RPC response with the session header. `call_tool` is unchanged.
- `MG/mcp_elicitation.rs` - NEW (+ `pub mod` in `MG/lib.rs`). `ElicitationRegistry` (in-memory `DashMap`, key `el_<ulid>`, value: the held session and stream, org, user, run, server, deadline); cap 8 held calls per org and 64 per process; a reaper that cancels expired entries (JSON-RPC `cancel` result to the server, D5); `answer()` maps answers through `MPC/mcp_elicitation.rs`, posts the result, keeps reading the same stream, returns `Done` or a new `Elicit` (at most 3 elicitations per call, then cancel). Logs ids, counts and lengths only, never the message or answers.
- `MG/runtime_registries.rs` (CRLF) - `handle_proxy_mcp_tool` (`:2920-2943`): when `req.allow_elicitation && switch`, connect with elicitation and use `call_tool_interactive`; on `Elicit` map the schema (decline on `Err` and keep reading), register the held call and return `ProxyMcpToolResponse{elicitation}`; otherwise the existing path, unchanged.
- `MG/grpc.rs` (CRLF) - implement `McpElicitationService` on the gateway service struct and `add_service` it beside `ModelGatewayServer` (`:3037-3045`), with the same interceptor (service principal of execution-core only; the user id must equal the held call's user).
**Proto numbers.** `ProxyMcpToolRequest.allow_elicitation = 8`, `ProxyMcpToolResponse.elicitation = 4`; new file `mcp_elicitation.proto`. `buf breaking` must stay green (additive).
**Flag.** `MCP_ELICITATION_ENABLED` (gateway, literal `true`, default off); `MCP_ELICITATION_HOLD_SECONDS` (number, default 600, clamped 60-1800). Off: `allow_elicitation` is ignored, `initialize` is byte-identical, `call_tool` is used. Rollback: switch off.
**ZDR.** The gateway never decides retention: execution-core sets `allow_elicitation` only off ZDR and waived routes (S7), and `execute_mcp` already refuses those before any call (`EC/runtime_loop/mod.rs:2324`). Held calls are memory only. Golden: `e_zdr_run_mcp_elicitation_not_advertised` (asserts `initialize` capabilities `{}` when `allow_elicitation` is false).
**Tests.** `mp-contracts`: `mcp_elicitation::tests` (`string_enum_boolean_number_map_to_items`, `nested_or_array_schema_is_declined`, `more_than_four_fields_is_declined`, `sensitive_field_names_are_declined`, `number_answer_must_parse`, `boolean_answer_maps_from_option_id`). model-gateway: `mcp_jsonrpc` (`initialize_without_elicitation_is_unchanged`, `initialize_with_elicitation_advertises_it`, `server_request_is_parsed`, `sampling_is_refused_with_method_not_found`); `mcp_http` (`interactive_returns_done_for_a_plain_result`, `interactive_stops_at_an_elicitation_frame`, `byte_cap_spans_frames`); `mcp_elicitation` (`answer_posts_result_and_reads_on`, `expired_entry_is_cancelled`, `per_org_cap_refuses_a_ninth`, `wrong_user_cannot_answer`, `third_elicitation_cancels_the_call`); `runtime_registries` (`switch_off_ignores_allow_elicitation`). Commands: `cargo test -q -p mp-contracts --lib mcp_elicitation`, `cargo test -q -p model-gateway --lib mcp_`, `buf lint`, `buf breaking`.
**Live proof.** A fixture MCP server (Streamable HTTP, elicits `{project: enum[3], notify: boolean}` inside `tools/call`) on the proof stack; grpcurl `ProxyMcpTool{allow_elicitation:true}` returns `elicitation`; `AnswerMcpElicitation{accept}` returns the tool result; after 600 s without answer the server logs `cancel` [verify live].
**Effort.** L, 7-9 d. Reason: new proto file and service, a streaming SSE reader, a held-call registry with expiry, and the shared mapper; splitting further ships a reader nothing uses. **Deps.** WS05-S4 (done). **Risks.** the 8 s exec-core timeout (`EC/mcp_gateway.rs:69`) bounds the time to the first elicitation frame; long server work before eliciting fails as today. Held streams tie up connections (caps above).

### S5 - Question origin contract [BUILD]
**Goal.** A stored question says who asked it: the model (today) or a named MCP server; only execution-core can say the latter.
**Files.**
- `SC/migrations/0105_run_question_origin.sql` + `SC/migrations/revert/0105_run_question_origin.down.sql` - `ALTER TABLE run_questions ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'model' CHECK (origin IN ('model','mcp_elicitation')), ADD COLUMN IF NOT EXISTS origin_label TEXT NOT NULL DEFAULT '' CHECK (length(origin_label) <= 80), ADD CONSTRAINT run_questions_origin_label_chk CHECK ((origin = 'model') = (origin_label = ''))`; header comment in the 0054 style (why, erasure unchanged by `ON DELETE CASCADE`).
- `PROTO/orchestration.proto` (LF) - `CreateRunQuestionRequest`: `string origin = 8;` (`model` or empty = model; `mcp_elicitation`), `string origin_label = 9;` (the server's display name, <= 80); `RunQuestion`: `string origin = 15;`, `string origin_label = 16;`. Each commented.
- `MPC/user_question.rs` (LF) - `ORIGIN_MODEL`, `ORIGIN_MCP_ELICITATION`, `MAX_ORIGIN_LABEL = 80`, `validate_origin(origin, label)`.
- `SC/src/orchestration_store.rs` (CRLF) - `create_run_question` (`:2864`) writes both columns; `RUN_QUESTION_COLUMNS` and the row mapper read them.
- `SC/src/orchestration_grpc.rs` (CRLF) - `create_run_question` (`:3871`) validates origin; a non-model origin is accepted only from the execution-core service principal [verify at slice start how the handler distinguishes service callers; the delegated bearer is minted for the run owner]; the response and `list_run_questions` carry origin.
**Migration.** session-core **0105** (+ revert). **Flag.** None: no caller sends an origin until S7's switch is on; unset reads as `model`, and S6 omits the keys for `model`, so behaviour is byte-identical.
**ZDR.** No new store (two columns on an existing non-ZDR table). Erasure unchanged (cascade from `runs`).
**Tests.** session-core DB tests: `origin_defaults_to_model`, `mcp_origin_needs_a_label`, `model_origin_refuses_a_label`, `label_over_80_is_refused`, `user_principal_cannot_set_a_server_origin`, `list_returns_origin`; migration up/down re-run test in the existing pattern; `mp-contracts` `validate_origin` tests. `cargo test -q -p session-core --lib run_question`, `cargo test -q -p mp-contracts --lib user_question`.
**Live proof.** `psql` keys only: a model question row reads `model`; S7's proof row reads `mcp_elicitation` with the server label [verify live].
**Effort.** M, 3 d. **Deps.** WS14-S3 (done). **Risks.** `orchestration.proto` and `orchestration_grpc.rs` are shared with EP-1/EP-10 (round B C-B3/C-B4); fields here are on different messages.

### S6 - Origin on the event and the card [BUILD]
**Goal.** The person sees "Spørsmål fra <server>" and "Svaret sendes til <server>", never a server's question dressed as the assistant's.
**Files.**
- `MG/http_routes.rs` - `run_question_value` adds `origin` and `origin_label` only when origin is not `model`.
- `MG/sse_events.rs` (CRLF) - `ChatEvent::UserQuestion` gains `origin: Option<String>, origin_label: Option<String>`, serialised only when `Some` (`:892` writer); `MG/sse.rs:13501-13506` passes them from `fields`. Regenerate `MP/api/contracts/sse-event-payloads.json`, copy to the SPA, re-run `gen-chat-event-types.mjs --check`, update the pinned digest in `scripts/tests/zdr-contract-sync.test.sh` (the D2-956 recipe).
- `FES/shared/api/chat-client.ts` - `ChatUserQuestionEvent.origin?`, `originLabel?`; the reader accepts only `mcp_elicitation` with a non-empty label <= 80 chars.
- `FES/shared/chat-nodes/QuestionCard.tsx` (LF) - an origin line above the items and a notice under them when origin is `mcp_elicitation`; the existing no-credentials line stays.
**Flag.** None (renders only for a non-model origin, which needs S7's switch). **ZDR.** None.
**Tests.** `sse_events` (`user_question_without_origin_is_byte_identical`, `user_question_with_server_origin_carries_it`); `http_routes` (`run_question_value_omits_model_origin`); `chat-client.test.ts` (`origin_read_only_for_known_values`); `QuestionCard.test.tsx` (`server_origin_line_and_notice`, `model_question_unchanged`). SSE taxonomy and ZDR contract checks.
**Effort.** S-M, 2-3 d. **Deps.** S5, WS14-S4. **Risks.** SSE golden recipe touches contract files other slices also regenerate (rebase).

### S7 - Elicitation in the governed loop [BUILD]
**Goal.** In a Do run, a server's form becomes a resume question to the owner; the run pauses; the answer (or a skip) goes back to the same held call, and its result is the tool's result.
**Files.**
- `EC/mcp_gateway.rs` (CRLF) - `proxy`/`proxy_tool` set `allow_elicitation` from a `GovernedMcp` field; a client for `McpElicitationService` on the same channel; `answer_elicitation(...)`.
- `EC/runtime_loop/mod.rs` (CRLF) - `execute_mcp` (`:2315`): when the response carries `elicitation`, create a question (`delivery = resume`, `origin = mcp_elicitation`, `origin_label = server_label`, items from the pending form, `expires_in_seconds` = the hold left, idempotency `{run_id}:{step_id}:elicit:<n>`) and return the WS14-S5 input-pause outcome [verify at slice start the exact type WS14-S5 lands]; the tool result the model reads says "Venter på svar fra eieren på et skjema fra <server>".
- `EC/runtime_loop/resume.rs` (CRLF) - on resume, an answered elicitation question is consumed (`ConsumeRunQuestion`), mapped back (`MPC/mcp_elicitation.rs`) and sent with `AnswerMcpElicitation{accept}`; skipped -> `decline`; expired or cancelled -> `cancel`; the response becomes the step's tool result; `mcp_elicitation_expired` (gateway lost the held call, D16) becomes a tool error the model reads.
- `EC/runtime_loop/agent.rs` - only the admission check: `allow_elicitation` = switch && depth 0 && not ZDR && not waived && run parkable && no `space_id` (D9); counted against `MAX_QUESTIONS_PER_RUN`.
**Flag.** `MCP_DO_ELICITATION` (execution-core, literal `true`, default off). Needs the gateway's `MCP_ELICITATION_ENABLED` and `MCP_DO_TOOLS` != `off`. Off: `allow_elicitation` stays false (today's wire).
**ZDR.** Refused structurally: `execute_mcp` refuses ZDR/waived first; `allow_elicitation` is false there; `create_run_question` refuses persistence-free runs. Golden: `e_waived_run_mcp_elicitation_not_advertised`.
**Tests.** `mcp_gateway` (`allow_elicitation_false_when_switch_off`, `allow_elicitation_false_for_subagent_and_space_runs`); `runtime_loop` (`pending_form_creates_a_resume_question_with_server_origin`, `retry_returns_the_same_question`, `answer_is_mapped_and_sent_as_accept`, `skip_sends_decline`, `expiry_sends_cancel`, `lost_held_call_is_a_tool_error`, `question_cap_counts_elicitations`). `cargo test -q -p execution-core --lib elicitation`.
**Live proof.** Fixture server, Do prompt "registrer timene på prosjektet" -> card "Spørsmål fra Fixture"; answer -> same run continues, server logs one `accept` with the content; skip -> `decline`; a second run left unanswered -> `cancel` after the hold [verify live].
**Effort.** M-L, 5-7 d. **Deps.** S4, S5, WS14-S5 (exec half). **Risks.** `agent.rs` and `mod.rs` are hot (EP-1, EP-9 per round B); the WS14-S5 pause shape is not built yet.

### S8 - MCP Apps resource read [BUILD]
**Goal.** For an MCP tool whose definition names a `ui://` resource, the SPA can fetch that HTML through the gateway with the viewer's authority, never stored.
**Files.**
- `MG/mcp_apps.rs` - NEW (+ `pub mod` in `MG/lib.rs`). `ui_resource_for(tool_json) -> Option<String>` reads the tool's `_meta` [verify the exact key (`ui.resourceUri` or `ui/resourceUri`) against the MCP Apps extension at slice start; accept only `ui://`]; `read_app_resource(reg, ownership, org, user, server_id, tool, oauth_token)`: the same checks as `handle_proxy_mcp_tool` (`:2830-2878`: usable by this user, enabled, allowlisted, HTTPS), a fresh discovery that confirms the tool still declares that uri, `resources/read`, mime `text/html` (profile parameter allowed), size <= 512 KiB, UTF-8; an in-memory TTL cache (60 s) keyed by `(org, server, uri)` only; network-domain declarations reported, not honoured (D12).
- `MG/mcp_http.rs` - `list_tools_raw()` (the `tools/list` result entries as JSON, same paging) and `read_resource(uri)`; `McpToolDef` and the digest are untouched (D11).
- `MG/http_routes.rs` - `GET /v1/mcp-apps/resource?server_id=&tool=` (user bearer, org and user from claims), additive block at the end of the router; answers `{data:{uri, html, needs_network:boolean}}`, errors in the typed envelope (`not_found`, `too_large`, `unsupported_mime`); 404 while the switch is off. ZDR tokens refused with `feature_not_supported: mcp_execution`.
- `BFF/domains/mcp.rs` (CRLF) - pass-through `/api/v1/mcp-apps/resource` (pattern of `:56-66`), no extra headers.
**Flag.** `MCP_APPS_ENABLED` (gateway, literal `true`, default off). **ZDR.** Read-through only, memory cache 60 s, nothing written; refused on ZDR tokens. Golden: `e_zdr_token_mcp_app_resource_refused`.
**Tests.** `mcp_apps` (`only_ui_scheme_is_accepted`, `other_users_private_server_is_not_found`, `tool_not_in_allowlist_is_refused`, `uri_not_declared_by_the_tool_is_refused`, `oversized_resource_is_refused`, `non_html_is_refused`, `network_domains_are_reported_not_honoured`); `http_routes` (`route_404_when_switch_off`, `zdr_token_is_refused`); BFF route test (no forged scoping headers). `cargo test -q -p model-gateway --lib mcp_apps`, BFF cargo test.
**Live proof.** Fixture server with one app tool: curl through the BFF returns the HTML; an org-B user gets 404; a ZDR token gets the typed refusal [verify live].
**Effort.** M, 4-5 d. **Deps.** S4 (same transport file, land after), WS05-S4. **Risks.** the MCP Apps extension is new and its `_meta` keys may move (isolated in `mcp_apps.rs`).

### S9 - MCP App host in the frame [BUILD]
**Goal.** Under an MCP tool call that has an app, the person sees the app in the same sandbox, fed the call's input and result, able to stage messages.
**Files.**
- `FES/features/chat/lib/mcp-app-bridge.ts` - NEW, pure. JSON-RPC 2.0 over postMessage on top of `widget-bridge.ts`'s source/size/rate checks: host answers `ui/initialize` with host info and theme; sends `ui/notifications/tool-input` and `ui/notifications/tool-result` [verify the method names against the extension at slice start; keep them in one table]; handles `ui/size`, `ui/open-link` (safe link), `ui/message` (staged chip, D3); answers `tools/call` with JSON-RPC error `-32601`-style `app_call_requires_chat` until S10's switch is on; every other method `-32601`.
- `FES/features/chat/components/McpAppFrame.tsx` - NEW. Fetches the resource (`chat-client.ts` `getMcpAppResource(serverId, tool)`), labels the frame "App fra <server>", shows the network notice when `needs_network`, uses `sandboxWidgetDocument` with the bridge shim, never caches the HTML beyond the component.
- `FES/features/chat/components/ChatMessages.tsx` - under the answer, for each settled `ChatToolCall` named `mcp__<server>__<tool>` whose resource lookup succeeds, one `McpAppFrame` (at most 2 per answer; others "Vis app" on click); only on the chat surface (S1's context).
- `FES/shared/api/chat-client.ts` - `getMcpAppResource`.
- `FES/shared/actions/action-registry.ts` (+ test) - `chat.open_mcp_app` (risk `low`), `chat.stage_mcp_app_message` (risk `low`).
**Flag.** `VITE_MCP_APPS` (literal `'true'`, default off; needs S1 and S3 switches). **ZDR.** The resource call is refused for ZDR tokens; nothing is stored. Under a ZDR thread no frame is drawn.
**Tests.** `mcp-app-bridge.test.ts` (`initialize_handshake`, `tool_input_and_result_sent_once`, `tools_call_refused_while_s10_off`, `unknown_method_is_method_not_found`, `message_is_staged`); `McpAppFrame.test.tsx` (`label_names_the_server`, `network_notice_shown`, `fetch_failure_shows_the_tool_card_only`); `ChatMessages.mcp-app.test.tsx` (`non_mcp_tool_has_no_app`, `room_surface_has_no_app`, `third_app_is_click_to_load`).
**Live proof.** Fixture app renders the tool's result as a table at 1280/375 px; staging a message works; DevTools origin `null` [verify live].
**Effort.** M, 4-5 d. **Deps.** S3, S8. **Risks.** [verify at slice start] that Do-run MCP tool calls reach `ChatToolCall` with the namespaced name and output on reload (the `StepsPill` comment at `ChatMessages.tsx:2091-2096` says the cards exist).

### S10 - App-initiated tools/call [BUILD]
**Goal.** An app can call a read-class tool on its own server, as the person viewing it, through the same gate a Do run uses.
**Files.**
- `MG/mcp_app_calls.rs` - NEW (+ `pub mod`). Validates: switch, not ZDR, viewer is the run owner (the run id of the turn that drew the app), tool on the same `server_id` as the app, tool name allowlisted; asks capability-core for the tool's class through the existing read surface [verify at slice start which gateway client reads `capability_tools`] and refuses anything but `none`/`read` with `app_call_requires_chat` (D13); per-frame rate limit (10/min, key from a frame nonce the SPA mints); calls execution-core `ExecuteStep{tool_name: mcp__..., permission_mode: "ask", run_id, user_id}` with the viewer's delegated bearer, as `tools.rs:291` does for `code_interpreter`; returns the step output, scrubbed and capped at 64 KiB. An audit line (ids, tool, class, outcome; never arguments).
- `MG/http_routes.rs` - `POST /v1/mcp-apps/tool-call` additive block; body `{run_id, server_id, tool, arguments, frame_nonce}`; typed envelope errors (`app_call_requires_chat`, `rate_limited`, `not_found`).
- `BFF/domains/mcp.rs` - pass-through `/api/v1/mcp-apps/tool-call`.
- `FES/features/chat/lib/mcp-app-bridge.ts` - `tools/call` forwards to `chat-client.ts` `callMcpAppTool` when `VITE_MCP_APP_TOOL_CALLS === 'true'`; the refusal comes back as a JSON-RPC error the app can show.
- `FES/shared/api/chat-client.ts` - `callMcpAppTool`. `FES/shared/actions/action-registry.ts` - `chat.mcp_app_tool_call` (risk `medium`, ownerPlane `model`).
- `EC/grpc.rs` / `EC/runtime_loop/mod.rs` - only if [verify at slice start] `ExecuteStep` does not already dispatch `mcp__` through `execute_mcp` with the signed class: add that arm there, behind the same switch read in execution-core.
**Flag.** `MCP_APP_TOOL_CALLS` (gateway, and execution-core if the arm is added; literal `true`, default off) and `VITE_MCP_APP_TOOL_CALLS` (FE). **ZDR.** Refused on ZDR tokens before any call (`mcp_execution`); `ExecuteStep` refuses persistence-free callers anyway. Golden: `e_zdr_token_mcp_app_tool_call_refused`.
**Tests.** `mcp_app_calls` (`viewer_must_own_the_run`, `other_server_tool_is_refused`, `write_class_is_refused_with_requires_chat`, `rate_limit_per_frame`, `arguments_never_logged`, `zdr_token_refused`); FE bridge (`tools_call_forwards_when_on`, `refusal_maps_to_jsonrpc_error`); execution-core arm test if added.
**Live proof.** Fixture app's "Oppdater" button calls `echo_read` (read) -> works, SSE/run log shows the gate's v2 evidence; its "Opprett" button calls `create_item` (write) -> `app_call_requires_chat`; a Space member viewing the same thread cannot call [verify live].
**Effort.** L, 6-8 d. **Deps.** S8, S9, WS05-S6.code; enable only after WS05-S6 enforce is live. **Risks.** a second entry into tool execution beside the run loop; it must stay narrow (read class, same server, owner) or it becomes an approval bypass.

### S11 - User-content origin ADR stub [BUILD]
**Goal.** Record that no dedicated user-content origin is built in W3, why, and what would reopen it.
**Files.** `docs/architecture/adr-user-content-origin.md` - NEW. Status "Deferred". Context (opaque-origin `srcdoc` is the only untrusted-markup renderer, `viewer-registry.ts`), decision (no new origin, no app network, no app storage, no viewer-bound connectors), consequences (apps needing network do not work; S8's notice), reopen triggers (EP-12 share model, a customer app that needs network, a security review of a sandbox domain with its own cookie jar), owners (Frontend, Control for the domain and certificates, Application for any stored app state).
**Flag/contract.** None. **Effort.** S, 0.5 d. **Deps.** none.

### S12 - Flip, document, prove [FINISH]
**Files.** `MP/deploy/docker-compose.yml` (`MODEL_GATEWAY_WIDGET_GUIDANCE`, `MCP_ELICITATION_ENABLED`, `MCP_ELICITATION_HOLD_SECONDS`, `MCP_APPS_ENABLED`, `MCP_APP_TOOL_CALLS` on model-gateway; `MCP_DO_ELICITATION`, `MCP_APP_TOOL_CALLS` on execution-core; all `${...:-false}`); FE env documentation for the four `VITE_` switches (the owner edits `.env.example`); `MP/docs/decisions/ledger.md` (section 3 entries); `MP/docs/ZDR.md` rows; `MP/docs/capability-ownership-matrix.md` rows ("Inline widgets - Frontend, answer text only"; "MCP elicitation held calls - model-gateway memory; questions - session-core `run_questions`"; "MCP App resources - model-gateway read-through, never stored"); `MP/api/contracts/zdr-posture-matrix.json` cases (section 5); depth register `D3-nnn` rows for anything unproven.
**Order.** `VITE_CHAT_INLINE_WIDGETS` + `MODEL_GATEWAY_WIDGET_GUIDANCE` -> `VITE_CHAT_WIDGET_BRIDGE` -> `MCP_ELICITATION_ENABLED` + `MCP_DO_ELICITATION` (after WS14-S5 live) -> `MCP_APPS_ENABLED` + `VITE_MCP_APPS` (after the D11 security nod) -> `MCP_APP_TOOL_CALLS` (after WS05-S6 enforce live).
**Effort.** S, 2-3 d. **Deps.** all above.

## 5. ZDR clause

| Slice | New store or egress | ZDR turn/run | Waived (DEC-32) |
|---|---|---|---|
| S1, S3 | none (widget source is the answer text; frame has no storage) | drawn from the streamed answer only; nothing kept | as ZDR for anything new: nothing kept beyond today's thread |
| S2 | prompt notice only | allowed (no store) | allowed |
| S4 | held MCP call in gateway memory (<= hold window); user answer egresses to the MCP server | never advertised (`allow_elicitation` false; `execute_mcp` refuses first) | never advertised |
| S5, S6 | two columns on `run_questions` (existing non-ZDR store) | no question can be created (persistence-free runs refused) | refused |
| S7 | resume question row; answer sent to the server | refused (`mcp_execution`) | refused |
| S8 | 60 s memory cache of app HTML; read from the MCP server | route refused (`mcp_execution`) | refused |
| S9 | none | no frame | no frame |
| S10 | one `ExecuteStep` per call (egress to the MCP server) | route refused (`mcp_execution`); `ExecuteStep` refuses persistence-free callers | refused |

**Refusal behaviour.** No new feature key: every refusal reuses `mcp_execution` (`MP/api/contracts/zdr-refusals.json`), so the file and its `.sha256` do not change. Widgets need no refusal (they add no store); under ZDR there is no widget snapshot, no thumbnail and no app storage by construction.

**Golden matrix cases** for `MP/api/contracts/zdr-posture-matrix.json` (`e_` enforcement rows) and the inventory tests (`MG/../tests/zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs`):
- `e_zdr_run_mcp_elicitation_not_advertised` (S4, S7)
- `e_waived_run_mcp_elicitation_not_advertised` (S7)
- `e_zdr_token_mcp_app_resource_refused` (S8)
- `e_zdr_token_mcp_app_tool_call_refused` (S10)

`MP/docs/ZDR.md` gains rows for: elicitation answers (stored in `run_questions` on non-ZDR runs, sent to the named server, erased with the run), held calls (memory only), app resources (never stored).

## 6. Proof plan

1. Per slice: the unit tests named above; `cargo fmt --all -- --check`; SSE taxonomy and ZDR contract checks (S6); `buf lint`/`buf breaking` (S4, S5); session-core DB tests (S5); FE vitest, lint, typecheck diffed against HEAD's failing set; BFF tests (S8, S10).
2. Rebuilds for live proof: FE on a second dev server (S1-S3, S9, S10; the `:5173` container serves the main checkout); model-gateway (S2, S4, S6, S8, S10); execution-core (S7, S10); session-core (S5). After a worktree build use `.plan-work/execution/tools/recreate-with-live-env.sh` so the live env and NATS password are kept.
3. A fixture MCP server (Streamable HTTP, one read tool `echo_read`, one write tool `create_item`, one tool that elicits, one tool with a `ui://` app) registered on the proof org, allowlisted and approved through WS05's digest route.
4. Live flows on a non-ZDR proof org (owner's key from the env file outside the repos; stop at the usage line and summarise):
   - F1 widget (S1-S3): "lag en lånekalkulator" -> widget, slider works, follow-up staged and sent once, unsafe link refused; same text in a Space room is code.
   - F2 elicitation (S4-S7): Do run -> card with server origin -> answer -> run completes with the server's result; skip -> decline; timeout -> cancel.
   - F3 app (S8-S10): app renders the result; read call works through the gate; write call refused; org-B and Space viewer refused.
   - F4 ZDR org: no elicitation advertised (server log shows `capabilities: {}`), app routes refused with the typed refusal, zero new `run_questions` rows (count before/after).
5. Evidence: SSE transcripts (curl -N through the BFF), gateway and execution-core logs (ids and counts only), `psql` keys-only reads, fixture server logs, screenshots at 1280 and 375 px, appended to the depth register as `D3-nnn` rows.
6. Regression guards: `sandbox="allow-scripts"` without `allow-same-origin` pinned by tests in S1 and S9; `sandboxHtmlDocument` byte-identical; `initialize` byte-identical with the switch off; the Ask loop still refuses `mcp__` (`tool_loop.rs:4671-4686`); the two loops stay separate (S10 calls `ExecuteStep`, never `handle_proxy_mcp_tool` from a route); `cross_service_loop_contract.rs` untouched.

## 7. Risks and what the owner must decide

**Risks.**
1. Script in the transcript. Mitigated by the opaque origin, CSP, `inert` default on every surface but chat answers (D2), and staged follow-ups (D3). A widget can still show misleading text; it is labelled as a widget.
2. Elicitation as a phishing channel: a server asks for something it should not. Mitigated by the origin line (S6), the sensitive-name decline (D8), no secret kind, owner only (D9), and the answer-destination notice.
3. Authority creep through S10. Mitigated by read class only, same server, run owner only, the signed gate through `ExecuteStep`, rate limit; writes wait for resumable MCP approval.
4. Held connections and replicas (D5, D16): capped per org and per process, fail closed.
5. The MCP Apps extension and the elicitation revision are young; method names and `_meta` keys are marked `[verify at slice start]` and isolated in `mcp_apps.rs` / `mcp-app-bridge.ts`.
6. Hot files: `ChatMessages.tsx`, `chat-client.ts`, `action-registry.ts`, `http_routes.rs`, `sse.rs`, `sse_events.rs`, `runtime_loop/mod.rs`, `agent.rs`, `orchestration.proto`, `gateway.proto`, `runtime_registries.rs`. One slice at a time, additive blocks, CRLF kept where the file is CRLF (`gateway.proto`, `mcp_*.rs`, `runtime_registries.rs`, `http_routes.rs`, `sse_events.rs`, `orchestration_store.rs`, `orchestration_grpc.rs`, `BFF/domains/mcp.rs`), LF where LF (`orchestration.proto`, `user_question.rs`, `ask.rs`, all FE files named).
7. S7 waits on WS14-S5's executor pause; S4-S6 can land dark before it.

**Owner decisions (block only the slices named; defaults are built).**
- OD-1 (D3): widget and app follow-ups are staged chips the person sends, not auto-sent turns. Blocks the S12 flip of S3/S9 only.
- OD-2 (D7, D9): elicitation is a WS14 `resume` question, run owner only, no Space runs in W3. Blocks the S12 flip of S7.
- OD-3 (D8): the sensitive-field decline list and the four-field cap. Blocks the S12 flip of S7.
- OD-4 (D11): `ui://` uri outside the tool digest in v1 (security nod). Blocks the S12 flip of S8/S9.
- OD-5 (D13): app-initiated calls read class only in W3. Blocks the S12 flip of S10.
- OD-6 (D14, DEC-40): no dedicated user-content origin in W3. Recorded by S11.
- OD-7 (privacy): an elicitation answer leaves the platform to the named MCP server; acknowledge in the privacy notice before S7 is enabled for real orgs.

Not blocked by any decision: S1-S6, S8, S9, S11 (code, switches off). Suggested order: S1, S11, S2, S3, S4, S5, S6, S8, S9, then S7 (after WS14-S5) and S10 (after WS05-S6), then S12. S1-S3 (FE) run in parallel with S4/S5 (gateway, session-core).

**Reserved numbers.** session-core migration **0105** `0105_run_question_origin` (+ `revert/0105_run_question_origin.down.sql`; the tree's head is `0104_routing_policy_candidates`, and no branch among `main`, `space-w2`, `mp-w2w3`, `w23/integ`, `quarryv2-next`, `space` has a 01xx file). capability-core: none needed; **0036** is held only if S10 must seed a class row (0035 stays with EP-5 S6; the tree already has a `0034_browser_console_tool`, which round B's EP-11 `0034` reservation must note). Proto: `gateway.proto` `ProxyMcpToolRequest.allow_elicitation = 8`, `ProxyMcpToolResponse.elicitation = 4`; `orchestration.proto` `CreateRunQuestionRequest.origin = 8`, `origin_label = 9`, `RunQuestion.origin = 15`, `origin_label = 16`; NEW `mcp_elicitation.proto` (`McpElicitationService`, `McpElicitationPending`, `AnswerMcpElicitationRequest/Response`), added to the mp-contracts `build.rs` list. SSE: no new event name; `user_question` gains optional `origin`, `origin_label`. Request feature family: `inline_widgets`.
