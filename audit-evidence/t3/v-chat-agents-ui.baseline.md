# Verevon baseline: Chat page, Agent pages, gateway domains and Model Plane wiring

Audit 3 (Model Plane + Verevon chat and agent pages). Read-only. Auditor role: Verevon baseline auditor.

## 0. Scope, baseline and conventions

- **Baseline:** the working tree of `C:/dev/CoresSystem` as read on 2026-10-13. Git HEAD is `afb8392` (2026-10-10). The working tree has uncommitted changes on top of HEAD in exactly the audited area (`git status`: modified `ChatMessages.tsx`, `ChatPage.tsx`, `use-chat-controller.ts`, `chat-client.ts`, `chat-types.ts`, `chat-normalizers.ts`, `ScheduleDialog.tsx`, `sse.ts`, `http.ts` and others; untracked `ChatUtilityPage.tsx`, `ChatWorkPeriodPicker.tsx`). Findings describe the files as they are on disk, uncommitted changes included. The big commit `f5b8e1c7` (2026-10-08, "chat charts, dashboard home, insights and cost views") added the chart renderer, the run-notify control and the wide-research panel.
- **Ground truth:** code. Documents give intent. "Shipped" in this file means the code exists and is wired from the UI through the gateway to the owning Model Plane route. It does **not** mean live-proven. The repo's own parity audit (2026-10-01) separates `built_unit_tested` from `live_proven`; I mark that where it matters.
- **Provenance tags:** SRC = read directly in source; DOC = repository documentation; INF = my inference (basis stated). No REA was needed: everything audited is first-party source.
- **Path aliases used below:**
  - `FE` = `apps/Frontend Plane/verevonv3`
  - `CH` = `FE/src/features/chat`
  - `CMP` = `FE/src/features/dashboard/home/DashboardComposer.tsx` (the chat composer; 5,925 lines)
  - `API` = `FE/src/shared/api`
  - `GW` = `FE/apps/gateway/src/domains`
  - `MG` = `apps/Model Plane/rust/services/model-gateway/src`
  - `EC` = `apps/Model Plane/rust/services/execution-core/src`
  - `PL` = `FE/apps/verevon-web/plans`
- **Size of what was read:** `features/chat` 130 files / 42.5k lines; `features/agents` 38 files / 10.6k lines; `features/voice` 2.8k; `shared/api` 62 non-test clients; `shared/actions` 4.7k (168 action ids); gateway chat domain 4.3k lines plus `ag_ui`, `orchestration`, `agents_runs`, `mcp`, `memory`, `persona`, `ai`, `briefs`, `run_watchers`, `browser`, `agent_actions`, `studio`, `router_policy`, `cost`, `eval`, `finetune` and `search` (4.4k+ lines). Model Plane was read only where the chat wiring depends on it (`sse_events.rs`, `tool_loop.rs` builtins and gates, `sse.rs` instruction and agentic paths, `deep_research.rs`, `mcp_http.rs`, execution-core `capability_policy.rs` and `subagent/research_fan_out.rs`).

### Architecture in one picture (SRC)

```
SolidJS 2 RC SPA (/chat, /agents/*)            Rust Axum BFF gateway (same origin, /api/* only)        Model Plane
  CMP composer ──POST /api/v1/chat/stream──▶ GW/chat/streams.rs stream_chat ──mints 8-9 audience tokens──▶ MG /v1/invoke/stream
  use-chat-controller ◀── SSE (native events; AG-UI names also accepted) ◀── raw byte SSE proxy ◀─────────── sse.rs / sse_events.rs
  run panels ──GET /api/v1/runs/{id}/events(+replay)──▶ GW/chat/streams.rs ─────────────────────────────────▶ MG /v1/runs/{id}/events
  approvals/plans ──/api/v1/orchestration/*, /api/v1/agents/runs/*──▶ GW/orchestration.rs, agents_runs.rs ──▶ MG /v1/orchestration/*, /v1/runs
  threads ──/api/v1/chat/threads*──▶ GW/chat/history.rs (stores nothing) ──▶ MG /v1/threads* ──▶ session-core (durable owner)
  memory/skills/plugins/cron/mcp/watchers ──▶ GW memory.rs, agent_actions.rs, mcp.rs, run_watchers.rs ──▶ MG /v1/memories, /v1/skills, /v1/plugins, /v1/cron, /v1/mcp/servers, /v1/runs/{id}/watchers
  persona ──/api/v1/me/persona──▶ GW/persona.rs ──▶ Convex (Application Plane) users.personaInstructions
  hosted browser ──/api/v1/browser/*──▶ GW/browser.rs ──▶ Quarry-v2 /v1/agent/runs (executes); AI runs via MG /v1/browser/runs
```

Two execution paths exist behind one composer (SRC, `API/chat-client.ts:1034-1186`):
1. **Ask (direct inline tool loop)** in model-gateway. The `tools` feature is always requested (except support-derived threads), so MG merges its read-only builtins. Only side-effect-free tools pass `inline_tool_allowed` (`MG/tool_loop.rs:625-645`).
2. **Do (agentic run)** when `plan_mode` is set: the `agentic` feature routes to the orchestration-backed run in execution-core with durable approvals, plan proposals, subagents and pause/resume (`API/chat-client.ts:1085,1110-1121`; `MG/sse.rs:9527` `agentic_run_stream`).

---

## 1. Chat experience

| Capability | What exists and how it works | Maturity | Evidence |
|---|---|---|---|
| Streaming | `streamChat` POSTs to `/api/v1/chat/stream`; the gateway mints model, data, ingestion, capability (best effort), inference, execution, cost and session bearers (503 if a required one fails), enriches the body and raw-proxies MG's SSE. Fetch-based SSE reader with `Last-Event-ID`. | shipped | SRC `API/chat-client.ts:1811-1836`; `GW/chat.rs:17`; `GW/chat/streams.rs:18-132`; `API/sse.ts:29` |
| Rich event taxonomy | Client handles `connected, chunk, done, stopped, error, artifact, attachment, reasoning_delta, tool_call, tool_result, citation(s)/search_results, source_scope_applied, grounding, step_update, awaiting_approval, usage, turn_timing, result_receipt, title, follow_ups, plan_proposed, verification, memory_recall, queued_input, retrieval_notice`. Producer names match MG's `ChatEvent` enum. Unknown events go to a UI-event hook rather than being dropped. | shipped | SRC `API/chat-client.ts:1488-1760`; `MG/sse_events.rs:144-149,462-484` |
| Resume after disconnect | Turn keeps the highest SSE frame id; resume GETs `/api/v1/chat/stream/resume/{request_id}` with `Last-Event-ID` (10-minute server buffer). Durable-outcome reconciliation with backoff when the stream cannot resume. Buffer is in memory, so a model-gateway restart loses an in-flight turn (DOC). | partial | SRC `CH/components/chat-types.ts:268-278`; `GW/chat/streams.rs:134-167`; `CH/components/use-chat-controller.ts:1208-1528`; DOC parity row "Resume ... across a gateway restart" (`PL/system-audits/chat-parity-audit-2026-10-01.json`) |
| Stop / cancel | Stop calls `/api/v1/chat/invocations/{id}/cancel` and, for runs, the durable run cancel; unconfirmed cancels are recorded per turn (`cancelFailures`) and reconciled. | shipped | SRC `API/chat-client.ts:1860`; `GW/chat/json_handlers.rs:153`; `CH/components/chat-types.ts:43-51,331-337`; `use-chat-controller.ts:1455-1528,2932` |
| Continue after stop ("Fortsett") | Resends the exchange with the partial text as displayed seed and a model-facing continuation instruction. No backend continue-from-partial field; deliberately not flagged as a regenerate. | shipped (client-spliced) | SRC `use-chat-controller.ts:3582-3640`; `ChatMessages.tsx:783` |
| Mid-run messages | Submit while streaming persists the message and queues it into the live run (`/invocations/{id}/queue`), shown as pending/delivered/refused. Registry is in-memory single-replica (DOC). | shipped | SRC `API/chat-client.ts:2000`; `GW/chat.rs:39-44`; `CH/components/chat-types.ts:475-493`; `ChatMessages.tsx:2105`; CMP prop `allowMidRunSubmit` (`ChatPage.tsx:249-253`) |
| Markdown rendering | Own parser (no markdown library): headings, ordered lists with start number, nested lists, task lists, aligned tables, nested quotes, raw `<details>` as collapsible, code fences with closed/unclosed state. | shipped | SRC `chat-types.ts:588-652`; `ChatMessages.tsx:1086-1600`; `chat-media-markdown.tsx` |
| Code blocks | Custom highlighter plus copy button with copied/failed states. | shipped | SRC `CH/components/chat-code-highlight.ts`; `ChatMessages.tsx:1218-1288` |
| Math | KaTeX for `$$` blocks, raw source shown until the fence closes. | shipped | SRC `ChatMessages.tsx:1428`; `chat-types.ts:645-649`; `FE/package.json:34` |
| Diagrams | Mermaid 12 with `securityLevel: 'strict'`, rendered only once the fence closes, cached. | shipped | SRC `ChatMessages.tsx:1360-1420`; `chat-types.ts:619-626` |
| Charts | ```chart fences and "view this table as a chart" (`ChatTableTools`) render one accessible SVG (role=img, data table, keyboard navigation, never colour alone); bad specs never throw; SVG export. | shipped | SRC `CH/components/ChatChart.tsx:1-15`; `CH/components/ChatTableTools.tsx`; `CH/lib/chat-chart/spec.ts:1-20`; `CH/lib/chat-chart/svg-export.ts` |
| Citations | Citation carries id, url, snippet, optional server `claimId`, `sourceGroupId/Label`, byte offsets and `origin` (web/knowledge). Inline markers are placed client-side by regex over `[n]`; server anchor pass exists. | shipped (chip binding not live-proven, DOC) | SRC `chat-types.ts:90-138`; `API/chat-client.ts:953-969`; `MG/citation_anchors.rs:1` |
| Sources panel | Separates "Read and used" from "Not read" leads; knowledge cards vs web cards; grounding overview (facts, sources, graph nodes, community summaries, trace id); low-confidence flag. | shipped | SRC `CH/components/ChatPanels.tsx:356-538`; `chat-types.ts:163-209` |
| Artifacts / canvas | `create_artifact` / `update_artifact` / `read_artifact` builtins; Output panel renders document (markdown), code, HTML (sandboxed iframe, `allow-scripts` without same-origin), images, PDFs and generated files. Version stepper (`v2/3`) and "show changes" diff against the previous revision. | shipped | SRC `MG/tool_loop.rs:4361-4383`; `CH/components/ChatArtifactPanel.tsx:88-600` (versions 311-323, diff 202-205/333-343, HTML 502-519); `chat-types.ts:54-79` |
| Editing artifacts by hand | No editor, selection-scoped edit, tone/length shortcuts or "restore this version". Revisions come only from the model. | absent | SRC: no textarea/contenteditable in `ChatArtifactPanel.tsx`, `ChatWorkspaceCanvas.tsx`, `ChatAttachmentCanvas.tsx` (grep). Planned as I17 (DOC `PL/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` Wave 5) |
| Regenerate with versions | Regenerate keeps the previous answer as a 1/N switchable version for the **final exchange only**; versions are not persisted and are lost on reload. Sends `regenerated: true` (feeds MG's dissatisfaction classifier). | partial | SRC `use-chat-controller.ts:3499-3531,3789-3794`; `CH/lib/chat-versions.ts:44-137`; `ChatMessages.tsx:734-751` |
| Edit and resubmit | Edit any user message; editing a non-final one asks for confirmation and discards later turns. Sends `edited_resubmit: true`. | shipped | SRC `use-chat-controller.ts:3686-3750`; `ChatMessages.tsx:886-954` |
| Branching | "Branch" copies turns up to a safe boundary into a new client-minted thread id and writes a local snapshot. The gateway's save route ignores `turns` (session-core is the only owner), so a branch exists only in the browser until it sends (INF from code). | partial | SRC `use-chat-controller.ts:190-210,3752-3786`; `GW/chat/history.rs:226-281` (comment 272-277) |
| Effectful-turn immutability | A turn whose run proof shows a real effect cannot be regenerated, edited or continued in place; "Run as a new turn" re-runs it. | shipped | SRC `use-chat-controller.ts:3502-3505,3642-3676`; `FE/src/shared/chat/effect-class.ts`; `ChatMessages.tsx:767` |
| Thread history | Durable threads from session-core through MG `/v1/threads?limit=80&origin=chat`; BFF caps the list at 80; rail shows run status chips. | shipped (80 cap) | SRC `GW/chat/history.rs:21,335-385`; `API/chat-client.ts:2189`; `FE/src/features/core/components/CoreSidebar.tsx:644,874` |
| Rename / pin / delete / clear | Presentation fields (title, preview, pinned, persona_enabled) PUT to MG `/v1/threads/{id}/presentation`; delete is a hard erase (archive routes exist in MG but are not exposed). | shipped | SRC `GW/chat/history.rs:226-320,415-511`; `CoreSidebar.tsx:473-480`; DOC parity row "Per-thread delete" |
| Projects / folders | Browser-only organizer: up to 100 named projects, thread-to-project assignment, group/sort/filter (all, pinned, active, attention). Never sent to a server, not cross-device. | partial | SRC `FE/src/shared/actions/chat-organization.ts:3-27`; `CH/lib/chat-organization.ts:5-69`; `FE/src/features/core/components/sidebar/ChatSidebarOrganizer.tsx:16,209` |
| Conversation search | Substring match over title, preview and project name of the loaded thread list. No full-text search over turns, no server search, no recall tool. | partial | SRC `CH/lib/chat-organization.ts:49-56`; grep for `recall_conversations\|search_threads\|SearchThreads` in Model Plane Rust: none |
| Deep links | `/chat?thread_id=`; the controller resolves the id through the owner-bound transcript route; foreign-origin threads open read-only. | shipped | SRC `CH/lib/chat-thread-deep-link.ts:6-31`; `use-chat-controller.ts:495-507` |
| Share link | None. | absent | SRC grep `share` in `features/chat` (non-test): only unrelated hits. Planned I19 (DOC gap plan Wave 6) |
| Import conversations | None (no ChatGPT/Claude export importer). | absent | SRC grep for conversation import in `features/chat`, `features/settings`, `shared/api`: no importer |
| Export | Word, Excel and PowerPoint built in the browser (pure OOXML builders, no network); PDF via a hidden print iframe; export button shown on answers of 600+ characters and in every message menu; artifacts export too. No images, charts or math in Office files. | shipped | SRC `CH/lib/chat-export/index.ts:1-60`; `print.ts`; `ChatMessages.tsx:621-624,708`; `ChatArtifactPanel.tsx:253-256`; grep `chart` in docx/xlsx/pptx builders: none |
| Follow-ups and titles | `follow_ups` chips (session-only) and server `title` events. | shipped | SRC `API/chat-client.ts:1659-1672`; `ChatMessages.tsx:1870` |
| Pin message into context | Up to 5 pinned message ids per thread (stored per browser) sent as `pinned_message_ids`; MG re-expresses them as protected system context. | shipped (per browser, not per user) | SRC `CH/lib/chat-pinned-messages.ts:33-122`; `API/chat-client.ts:95-106`; `ChatMessages.tsx:681-690` |
| Feedback | Thumbs plus reasons (Inaccurate, Not relevant, Incomplete, Harmful) to `/api/v1/chat/feedback` bound to the run. | shipped | SRC `ChatMessages.tsx:491-494,858`; `API/chat-client.ts:2724`; `GW/chat/json_handlers.rs:480-494` |
| Cold start | Greeting by time of day, suggestions from open inbox cases and alerts, starter rows, "pick up where you left off". | shipped | SRC `ChatMessages.tsx:3763-4035`; `CH/lib/prompt-suggestions.ts` |
| Findings / Reports / Routines pages | `/chat/findings` (notifications plus open inbox cases), `/chat/reports` (prose artifacts read from the local transcript cache), `/chat/routines` (scheduled runs). Untracked new file. | shipped (reports rely on local cache) | SRC `FE/src/app/App.tsx:178-180`; `CH/components/ChatUtilityPage.tsx:23-70` |
| Failure presentation | Typed failure actions (retry, reconnect ChatGPT, wait until a reset time, open integrations, none), "not saved" disclosure for text that streamed but was not persisted, truncation and stop-reason notices. | shipped | SRC `chat-types.ts:231-324`; `ChatMessages.tsx:3277-3419` |

## 2. Multimodal & creation

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Attachments (vision + documents) | Images go as base64 (vision). PDF/DOCX/XLSX/PPTX are text-extracted through `/api/v1/chat/documents/extract` (imports-core); txt/md/csv/json/html read locally. Limits: 1 MB per file, 60,000 characters per file, 120,000 per turn. Small document text is folded into the model-facing content. | shipped (vision `built_unit_tested`, DOC) | SRC `CH/lib/prepare-chat-attachments.ts:6-43`; `GW/chat/documents.rs:14-40`; `chat-types.ts:531-540` |
| Durable chat document upload | `uploadChatDocument` (text to Data Plane documents-api) exists but has **zero callers**. | stub | SRC `API/chat-client.ts:2081-2118`; grep: no callers; `GW/chat/documents.rs:43-109` |
| Add attachment to knowledge base | Explicit per-file action posts bytes to imports-core; blocked in temporary chat. | shipped | SRC `CH/components/ChatPage.tsx:187-205`; CMP `importFilesToKnowledge` (≈l.1764) |
| Connector attach picker | Attach items from OneDrive, Gmail, Outlook mail, Google Drive, Google Calendar, Outlook Calendar through integration-corev2 browse/content operations, with provenance on the attachment. Not live-proven on real accounts (DOC). | partial | SRC CMP `175-181,5019-5423`; `API/integrations-client.ts`; DOC parity rows "Attach from a connected source" |
| Screenshot attach | One-shot `getDisplayMedia` capture to a PNG attachment. No continuous screen share. | shipped (one-shot) | SRC CMP `1913-1945,3675` |
| "AI enhance" on attachments | Rewrites the draft into "Analyze the attached file(s) ..." locally. No model call. | shipped (cosmetic) | SRC CMP `1744-1762,4228-4230` |
| Image generation | Composer image mode / `/image` sets `generate_image`; MG streams an image-generation call and returns an image artifact; inline preview and download. Not live-proven (DOC). | shipped | SRC CMP `477-485`; `API/chat-client.ts:1107`; `MG/sse.rs:1427,5958,6200`; `ChatMessages.tsx:2687-2745` |
| Generated files | `code_interpreter` writes files that come back as download cards (PNG inline). | shipped | SRC `MG/tool_loop.rs:4356-4358`; `ChatMessages.tsx:2748-2790`; `chat-types.ts:140-155` |
| Office document generation | Via `code_interpreter` (python-docx, python-pptx, openpyxl/xlsxwriter, reportlab, matplotlib). No designed deck pipeline, no edit-in-place of an existing Office file. | partial | SRC `MG/tool_loop.rs:4357`; DOC parity rows "Slide-deck generation", "Edit an EXISTING Excel or PowerPoint" (not_built) |
| Data analysis | `code_interpreter` (pandas, numpy) in a networkless ~30 s sandbox; attachments are not staged as files into the sandbox (DOC H6 planned). | partial | SRC `MG/tool_loop.rs:4357`; DOC gap plan Wave 4 H6 |
| Dictation (STT) | Mic recording to `/api/v1/ai/dictate` (governed, MG → inference-core); blocked in temporary chat. | shipped | SRC `API/audio-client.ts:57-77`; `GW/ai.rs:46-78`; CMP `1887` |
| Voice mode + read aloud | Push-to-talk dialog, one dictate call per utterance, review, send through the normal composer; optional read-replies TTS via `/api/v1/ai/speech` (Azure Speech, default voice nb-NO-PernilleNeural); per-message "Read aloud". No browser-voice fallback; off in temporary chat. 10 dictation languages. | shipped | SRC `FE/src/features/voice/VoiceModeDialog.tsx:40-60`; `API/speech-client.ts:6-26,90`; CMP `3683`; `ChatMessages.tsx:816-818`; `ChatPage.tsx:212-229` |
| Realtime duplex voice / barge-in | None. Owner ruling recorded as "no duplex voice" (DOC). | absent | SRC grep `realtime\|WebRTC\|RTCPeer\|WebSocket` in `features/voice`: only CSS class names; DOC gap plan I22 |
| Video | No video generation or video understanding in chat. A Quarry video *search* route exists. | absent | SRC grep in `src` for video generation: none; `GW/search.rs:43,483` |
| Studio canvas | Social/campaign design canvas (blocks, undo/redo, desktop/mobile preview, export to social draft). The gateway store is in-memory and lost on restart. Not an AI generation surface. | partial | SRC `FE/src/features/studio/components/StudioPage.tsx`; `GW/studio.rs:1-35` |

## 3. Research & web

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Web search | "Nett" toggle / `/search` adds `web_search`; server gates web search behind the explicit toggle. Builtins also include `fetch_url`. | shipped | SRC `FE/src/shared/actions/agent-tools.ts:32-41,150`; `MG/tool_loop.rs:4341-4343`; `API/chat-client.ts:1050-1070` |
| Deep research | Its own field `deep_research`: plan up to 9 sub-queries, search concurrently, read pages concurrently, synthesise a cited report, emit citations and a document artifact. Only numbers pages that were actually read. In-process, not Temporal. Progress shows in Work. | shipped (`live_proven` per DOC) | SRC `MG/deep_research.rs:1-30`; `API/chat-client.ts:1120-1126` |
| Wide research fan-out | `subagent.research_fan_out` in Do runs (enabled by default, concurrency 4); chat renders per-item cards by `batchId`. No user-facing toggle; cards are not in the durable replay, so they start empty after reload. | partial | SRC `EC/subagent/research_fan_out.rs:1-40`; `apps/Model Plane/deploy/docker-compose.yml:1063-1074`; `CH/components/ChatWideResearchPanel.tsx:1-30`; grep "Bred forskning" in composer: none |
| Knowledge grounding | Builtins `knowledge_search`, `knowledge_graph_search`, `knowledge_wiki_search`, `knowledge_contradictions`, `knowledge_list_documents`, `result_query` over Data Plane v2; grounding event carries facts, sources and graph summary. | shipped | SRC `MG/tool_loop.rs` builtins (from 4323); `chat-types.ts:163-209` |
| Retrieval-scope filter | Per turn `auto / knowledge_only / web_only`, enforced server-side by pruning tool buckets and re-checking at dispatch; `source_scope_applied` receipt shown. | shipped (`built_unit_tested`, DOC) | SRC `MG/tool_loop.rs:4268-4300`; `API/chat-client.ts:1035-1047`; `ChatPanels.tsx:650-657` |
| Conversation-only scope | Thread-level `conversation` scope removes knowledge and web tools (artifact, word count, code interpreter, reattach only). Locked after the first turn. | shipped | SRC `MG/tool_loop.rs:4264-4266`; `ChatPage.tsx:233-236` |
| Inbox and org data tools | `inbox_search`, `inbox_get_conversation`, `insights_overview`, `social_list_*`, `shipping_get_quotes`, `get_weather` (Norwegian cities), `get_statistics` (SSB population). | shipped | SRC `MG/tool_loop.rs:4346-4373` and builtin list |
| Support context | Gateway enriches support-flavoured prompts with tickets and conversations; support-derived threads are read-only (no tools). | shipped | SRC `GW/chat/support.rs:23-69`; `API/chat-client.ts:1018-1031`; `GW/chat/history.rs:709-770` |
| Confidence and verification | Answer confidence scored (base 0.72 ungrounded); below 0.75 a caveat shows on the bubble; low-scoring answers get a verification pass whose verdict is shown. | shipped | SRC `chat-types.ts:211-229,339-345`; `ChatMessages.tsx:2801-2850`; `MG/confidence.rs:1`; `MG/verification.rs:1` |
| Retrieval notice | Typed "cannot answer without guessing" notice. | shipped | SRC `API/chat-client.ts:250,1740` |
| Perplexity-style answer search | On the dashboard (not `/chat`): streamed answer over Quarry search, plus image/video/similar search routes. | shipped (outside chat) | SRC `GW/search.rs:39-45,596`; `FE/src/features/dashboard/home/DashboardSearchPanel.tsx:339` |
| Connector reads by the agent | Outlook/Gmail/OneDrive/SharePoint reads for the agent are partial; writes are not built. | partial | DOC parity "harness" rows; gap plan Waves 3-5 |

## 4. Agent runtime

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Ask / Do switch | Segmented control "Spør / Utfør". Do sets `plan_mode` plus the `agentic` feature; disabled for conversation-only threads and for the ChatGPT subscription unless the catalogue advertises `agentic`. | shipped | SRC CMP `3030-3052,1145`; `API/chat-client.ts:1085,1110-1121,2617-2630` |
| Plan proposal and gate | `plan_proposed` event drives an editable plan card: edit, reorder, remove and add steps; choose an authority rung (`workspace_write` or `danger_full_access`); a justification of at least 12 characters; approve or reject via `/api/v1/agents/runs/{id}/plan-approval`. Last proposal wins. | shipped (`built_unit_tested`, DOC) | SRC `ChatMessages.tsx:2217-2542`; `API/chat-client.ts:1873-1975`; `GW/agents_runs.rs:43`; `chat-types.ts:393-420` |
| Tool approvals | `awaiting_approval` / `run_paused_for_approval` produce approval cards (Approve / Reject); decisions go through `/api/v1/orchestration/approvals/{id}/decide`; run resume/cancel routes. | shipped | SRC `ChatMessages.tsx:1889-1940`; `GW/orchestration.rs:64-112`; `API/chat-client.ts:1399-1450` |
| Durable runs and live events | Run id captured from `connected`; live run panel subscribes to `/api/v1/runs/{id}/events` (Last-Event-ID) and replays `/events/replay`. | shipped | SRC `GW/chat/streams.rs:169-273`; `CH/components/ChatLiveRunPanel.tsx:196`; `CH/lib/chat-run-watch.ts` |
| Plans, todos, proof bundle | Work tab plan view from orchestration plans/todos; Trace tab shows the run proof bundle (approvals and receipts). | shipped | SRC `ChatPanels.tsx:1008-1160,1271-1400`; `GW/orchestration.rs:64-80` |
| Subagents and lineage | `subagent.*` tools mapped to `cap.agent.spawn`; lineage fetched for the plan panel. | partial | SRC `EC/capability_policy.rs:741-744`; `ChatPanels.tsx:1041`; `API/orchestration-client.ts:315-324` |
| Background runs + notify | "Notify me when done" toggle in the waiting indicator and the live run header; durable per-user watcher through capability-core to notification-core. | shipped | SRC `CH/components/RunNotifyButton.tsx:1-20,128`; `ChatMessages.tsx:3685`; `ChatLiveRunPanel.tsx:535`; `GW/run_watchers.rs:1-54` |
| Scheduling | "Schedule this" message action and `/schedule` open a dialog with four cadence presets or a cron expression; creates `/api/v1/cron` schedules; Scheduled-runs panel; settings section. A scheduled run is a single no-tool call on Verevon Balance (DOC). | partial | SRC `CH/components/ScheduleDialog.tsx:56`; `API/schedule-from-chat.ts:21-65`; `GW/agent_actions.rs:51,511`; `ChatMessages.tsx:810`; DOC parity row "Schedule this" |
| Context management | MG compaction with a summary prefix and protected pins; `reattach_context` offered only after compaction; skills budget cap with truncation markers and `reattach_skill`. | shipped | SRC `MG/compaction.rs:1`; `MG/tool_loop.rs:4331-4338,4310-4320` |
| Checkpoint / rewind | None. | absent | SRC grep `rewind\|checkpoint` in `features/chat`: only run-detail test fields. Planned I11 |
| Error recovery | Reconcile stopped/unresumable turns against the saved answer; typed stream failures (subscription limits with reset time, reconnect). | shipped | SRC `use-chat-controller.ts:1241-1528`; `chat-types.ts:231-242` |
| Agent Run Console (`/agents/runs`) | Goal launcher (blueprint, Verevon mode, web toggle, presets) that runs `streamChat` with `planMode: true`; live timeline (step, tool, reasoning, plan, todo, browser, subagent, pause, resume, verification); approval deck; answer and trust panels; plan panel; proof bundle; run history (thread or system); browser observation screenshots; telemetry; watch toggle. | shipped | SRC `FE/src/features/agents/components/AgentRunConsole.tsx:97-330,784-840,1291,1925,2063,2222,2334,2478` |
| Preset agents | Four presets (draft reply, summarise inquiries, plan KB refresh, triage tickets), all with an empty action set "until owner adapters are live". | partial | SRC `FE/src/shared/actions/preset-agents.ts:11-85` |
| Agents overview | Blueprint roles explicitly labelled "reference operating model, not yet configured"; entries for Task Console, Installations and Chief/Core routing; Workflow builder and Chatbot studio are design previews. | stub (honestly labelled) | SRC `FE/src/features/agents/components/AgentsPage.tsx:47-206`; `DesignPreviewBadge.tsx`; DOC `FE/FRONTEND_PLANE_STATUS.md` "MIXED (honest)" |
| Agent installations / Chief-Core | Real reads of Space agent installations. | shipped (read-only) | SRC `AgentInstallationsPage.tsx:5`; `ChiefCoreRoutingPage.tsx:5` |
| Fleet run console | Fetches `/v1/fleets/{id}` and `/v1/fleets/{id}/budget` directly. No gateway route exists and nginx only proxies `/api/`, so the page cannot load data (INF). | stub | SRC `FE/src/features/agents/components/FleetRunConsole.tsx:1-30`; `FE/src/app/App.tsx:216`; `FE/nginx.conf:74-76`; grep `fleets` in FE: only those two files |
| Hooks | Declarative pre/post-tool hooks exist in the Model Plane but are not fed into agentic runs (DOC). | partial | DOC parity row "Declarative pre/post-tool hook" |
| Implicit learning signals | Regenerate and edit flags feed MG's implicit-dissatisfaction classifier against the skills that served the answer. | shipped | SRC `API/chat-client.ts:141-152,1160-1161`; `MG/dissatisfaction.rs:1` |
| Evals / quality | `/agents/quality` shows run success rate and drift from session-core history; honest empty state. | shipped (basic) | SRC `FE/src/features/quality/components/OpsQualityPage.tsx:1-40`; `GW/eval.rs:39` |

## 5. Tools, plugins & extensibility

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Slash commands | `/file`, `/image`, `/search`, `/research`, `/schedule` built in; plus backend skills, capabilities and connectors fetched from `/api/v1/skills`, `/api/v1/capabilities`, `/api/v1/integrations/providers` (each degrades to empty). | shipped | SRC CMP `259-271,465-532,540-595`; `API/chat-actions-client.ts:20-61` |
| User-authored command templates | None (no `$ARGUMENTS`). | absent | SRC grep `$ARGUMENTS` in `src`: none; planned I13 |
| Skills | CRUD with content, trigger keywords and file patterns, tool restrictions, org/user scope, sharing; up to 4 picked per turn sent as `skill_ids`; MG injects the body as system context; `reattach_skill`. Self-learning loop from completed runs not proven (DOC). | shipped | SRC `API/skills-client.ts:1-91`; `FE/src/features/settings/components/SkillsSection.tsx`; `GW/agent_actions.rs:41,175`; `API/chat-client.ts:980-1010` |
| Plugins | Registry with manifest JSON, risk level, enabled, pinned, rollout state. No marketplace or install flow; no runtime effect found from the UI side (INF). | partial | SRC `API/plugins-client.ts`; `PluginsSection.tsx`; `GW/agent_actions.rs:46,325` |
| MCP servers | Register (HTTP / Streamable HTTP, token never returned), user/org scope, share, OAuth start/callback/connect; MG implements the real JSON-RPC Streamable HTTP transport. **But** the inline chat loop denies `mcp__*`, `mcp_call`, `mcp_catalog`, and execution-core's `trusted_capability_id` returns no capability for `mcp__*`, so MCP tools fail closed in both loops. | partial (registration only; execution blocked) | SRC `API/mcp-client.ts:1-40`; `McpServersSection.tsx`; `GW/mcp.rs:46-54`; `MG/mcp_http.rs:1-30`; `MG/tool_loop.rs:625-645,3722-3733`; `EC/capability_policy.rs:741,965` |
| Capabilities catalogue | `/api/v1/capabilities` read for the `/` menu. | shipped | SRC `GW/agent_actions.rs:56` |
| Action registry | 168 typed actions (zod I/O, owner plane, risk, approval, reversibility) shared by UI and agents; only 8 are model-executable (`tickets.create`, `inbox.follow_conversation`, `inbox.set_csat_preference`, `chat.save_thread_snapshot`, `inbox.review_ai_action`, `org.mark_exported`, `org.acknowledge_deletion`, `chat.submit_feedback`), each with written evidence. | partial | SRC `FE/src/shared/actions/action-registry.ts`; `FE/src/shared/actions/model-eligibility.ts:1-139`; `agent-tools.ts:104-181` |
| Rules / instructions | Org instructions, Space authored instructions, @-mentioned Space agent persona and personal persona are layered into direct chat by the gateway before forwarding. | shipped (Ask only, see gaps) | SRC `GW/chat/streams.rs:55-96`; `MG/sse.rs:1855,6375-6440` |
| Provider actions | `list_provider_actions` / `execute_provider_action` exist as trusted execution-core tools (Do only). | partial | SRC `EC/capability_policy.rs:766-840` |
| AG-UI | Gateway `/api/v1/ag-ui/stream` (+resume) translates MG events to AG-UI; client adapter `streamAgentUi`/`streamAgentChat` has no non-test consumer. The native chat stream also accepts AG-UI event names inbound. | partial (unwired) | SRC `GW/ag_ui.rs:23-62`; `API/ag-ui-client.ts:668-753`; `API/chat-client.ts:1188-1460`; DOC parity row "AG-UI ... Reach it from the app or delete it" |
| A2A | None. | absent | DOC parity row "A2A agent-to-agent collaboration" not_built |

## 6. Computer & environment control

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Code interpreter sandbox | Python 3 or sh, no network, about 30 s, file outputs returned as artifacts. | shipped | SRC `MG/tool_loop.rs:4356-4358` |
| Shell / processes | `shell`, `process_start/read/stdin/list` exist as governed execution-core tools in Do runs; the UI shows them as generic tool cards (terminal intent), no terminal view. | partial | SRC `EC/capability_policy.rs:766-780`; `FE/src/shared/chat-nodes/tool-presentation.ts:38` |
| Durable workspace (Space) | A sandbox-manager bearer is minted only for Space-scoped turns so `code_interpreter` can lease a Space workspace. | partial | SRC `GW/chat/shared.rs:205-247` |
| Hosted browser (manual) | Quarry-v2 browser sessions: frames over HTTP, SSE and WebSocket, tabs, DevTools, timeline, human/agent control toggle, persistent profiles. Lives in Knowledge/Dashboard (`BrowserChrome`), reused compactly in the chat run panel. | shipped (`live_proven` in part, DOC) | SRC `GW/browser.rs:1-25` and its route table; `ChatLiveRunPanel.tsx:1-15` |
| Agent-driven browser | MG `/v1/browser/runs`; risky actions (login, checkout, form post, destructive, cross-domain, cookies) get durable approvals through the generic orchestration routes; events `browser_action_approval_required/decided`. Runs fail closed today on an unissued broker grant (DOC). | partial | SRC `GW/browser.rs:7-25,1349`; `GW/chat/streams.rs:186-191`; DOC parity rows "Agent-driven hosted browser run" |
| Browser from chat | Chat turns cannot yet open or drive a browser; the live panel can show browser evidence and screenshots of a watched run. | partial | SRC `ChatLiveRunPanel.tsx:1-15`; `GW/chat/json_handlers.rs:425-450`; DOC planned E9/E10/E12 |
| Desktop / OS computer use, user's own Chrome | None; parked or ruled out by the owner. | absent | DOC parity rows (parked_or_ruled_out) |
| Mobile app | None; responsive web with a full-screen sheet below 760 px. | absent | DOC `VEREVON_CHAT_UX_DESIGN_SPEC.md` section 10; SRC `CH/components/ChatWorkspaceCanvas.tsx:1-60` |

## 7. Coding capability

Verevon is not an IDE; only the harness pieces matter.

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Code artifacts and diffs | Code viewer; unified-diff rendering for tool patches and artifact revisions (2,000-line cap). | shipped | SRC `ChatArtifactPanel.tsx:474`; `ChatMessages.tsx:1942`; `FE/src/shared/chat-nodes/diff.ts:61-243` |
| LSP bridge | MG `lsp.rs` exists; off with no consumer (DOC). | partial | SRC `MG/lsp.rs:1`; DOC parity row "LSP-grounded code edits" |
| Editable / re-runnable code cell, code mode | None. | absent | DOC parity rows "Live and editable code REPL" (partial: read-only), "Code mode" (not_built); planned H7 |
| Codebase indexing, tab completion, PR bots | None in Verevon. | absent | SRC grep `pull request\|github` in `features/chat`, `features/agents`, `shared/api`: none |

## 8. Memory, learning & personalization

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Long-term memory | Memory injection is unconditional server-side; when it happens the client is told (`memory_recall`) which memories with provenance (stated / inferred / unrecorded), persisted on the turn; correct and forget in place; Settings list and delete. `save_memory` / `recall_memory` are agent tools in Do runs; denied in the inline loop. | shipped (`live_proven`, DOC) | SRC `API/memory-client.ts:16-175`; `GW/memory.rs:50-143`; `ChatMessages.tsx:2907-3243`; `FE/src/features/settings/components/MemorySection.tsx`; `MG/memory_prefetch.rs:1`; `MG/tool_loop.rs:641` |
| Persona / custom instructions | Personal text (max 1,500 chars) with a global on/off, stored in Convex; per-thread override on the session-core thread; shown as a segment in the context inspector. Applied in Ask turns only. | shipped (Ask) | SRC `API/persona-client.ts:9-40`; `GW/persona.rs:1-60`; `FE/src/features/settings/components/PersonaSection.tsx`; `ChatPanels.tsx:581-583`; `GW/chat/history.rs:51-55` |
| Instructions in Do runs | `agentic_run_stream` (`MG/sse.rs:9527-10128`) has no reference to authored instructions or persona, so persona, org and Space instructions do not reach Do runs. | absent (gap) | SRC `MG/sse.rs:1855` (direct path uses them), `9527-10128` (none); DOC parity row "Persona, org and Space instructions applied in agentic runs" not_built |
| Skill learning loop | Completed runs reviewed into skills is designed; never recorded on a real run (DOC). | partial | DOC parity rows "Skills registry with ... self-learning", "G7 learning loop" |
| Session search / recall tool | None. | absent | see section 1 search row; planned I12 |
| Knowledge base | Data Plane v2 knowledge is the grounding corpus; Knowledge page manages sources (outside this audit's depth). | shipped | SRC `FE/src/features/knowledge/components/KnowledgePage.tsx`; section 3 |

## 9. Models & routing

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Model picker | Pinned Verevon intent modes (Budget, Balance (default), Genius) resolved server-side by inference-core; catalogue from `/api/v1/models` grouped Subscription / Claude / OpenAI GPT / DeepSeek / Other (no Gemini group); expensive badge; privacy-tier badges; per-org remembered choice. | shipped | SRC `API/chat-client.ts:808-866,2496-2705`; CMP `1008-1095,2771-2842`; `GW/chat/json_handlers.rs:453-470` |
| BYO ChatGPT subscription | User-owned ChatGPT connection (integration-corev2) as a provider route; on that route tools, browsing, images, deep research, skills and attachments are stripped; Do only when the server advertises `agentic`. | shipped (`live_proven`, DOC) | SRC `API/chatgpt-subscription-client.ts`; `API/chat-client.ts:1050,1094-1172`; CMP `1008-1030` |
| Reasoning effort | Composer "Auto / Raskt svar / Dyp research" maps to `effort` quick/deep (budget chosen by the gateway); reasoning streams as `reasoning_delta` into a collapsible trace. Not visible on the subscription route (DOC). | shipped | SRC CMP `154-168,384-405,3552-3554`; `API/chat-client.ts:107-116`; `ChatMessages.tsx:1603` |
| Verbosity | Concise / balanced / detailed sent as `verbosity`. | shipped | SRC CMP `169-173`; `API/chat-client.ts:117-127,1136-1138` |
| Router policy | Admin 3x3 table (mode x complexity) with complexity weights, keywords, budget cap, constrained fraction and cheap fallback, PUT to inference-core. | shipped | SRC `API/router-policy-client.ts:10-109`; `FE/src/features/router-policy/components/RouterPolicyPage.tsx`; `GW/router_policy.rs:30-66` |
| Privacy-tier floor | Optional `min_privacy_tier` (global, eu_resident, zdr_contractual, sovereign); empty remainder fails closed. | shipped | SRC `API/chat-client.ts:153-162,1167-1169`; `GW/chat/shared.rs:35-60` |
| Served-by line | Provider, residency and tier stamped server-side per answer; nothing rendered when absent. | shipped | SRC `chat-types.ts:290-297`; `MG/sse_events.rs:20-80` |
| Model comparison | None. | absent | SRC grep `side-by-side\|compareModels\|arena` in `src`: no feature |
| Provider failover | Partial; none on the subscription route (DOC). | partial | DOC parity row "Provider failover" |
| Prompt cache metric | Cache read tokens shown in Work telemetry when present. | shipped | SRC `ChatPanels.tsx:926-940` |
| Fine-tuning | Jobs list, upload, create, deploy (Azure via MG). | shipped | SRC `FE/src/features/finetune/components/FinetuneJobsPage.tsx`; `GW/finetune.rs:31-37` |

## 10. Transparency, observability & control

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Per-answer details | Popover: model time, system time, confidence, cost or "included in the subscription"; metrics badge with tokens, latency, cost basis; turn timing by phase. | shipped | SRC `ChatMessages.tsx:1623-1835`; `API/chat-client.ts:333-553`; `chat-types.ts:279-303` |
| Tool-call inspection | Cards with intent icon, hit count (0 renders), truncated flag, args, output, error, patch as diff. | shipped | SRC `ChatMessages.tsx:2023-2100`; `FE/src/shared/chat-nodes/tool-presentation.ts:38-159` |
| Work panel | Answers what is happening, what is left, whether Verevon needs you, what happened underneath (telemetry: output tokens, tokens/s, prompt cache; lifecycle; raw events); live agent screen. | shipped | SRC `ChatPanels.tsx:722-1000` |
| Context inspector | "Assembled context": tokens vs assembly budget per segment (persona, policy, workspace, agent, user, thread, memory, sources, knowledge, graph). | shipped | SRC `ChatPanels.tsx:571-720`; `API/chat-client.ts:2058-2140`; `GW/chat/json_handlers.rs:325-340`; `ChatPage.tsx:302-327` |
| Trace | Durable event log for the run (owner-only), capped at 5,000 with a visible notice; run proof panel with receipts. | shipped | SRC `ChatPanels.tsx:1271-1466` |
| Receipts and effect class | `result_receipt` per checked answer; effect class from the durable proof bundle. | shipped | SRC `chat-types.ts:325-330,428-429` |
| Cost views | `/agents/cost` dashboard (summary, users, analytics, entries, Azure actual, pricing) from cost-core. | shipped | SRC `FE/src/features/cost/components/CostDashboardPage.tsx`; `GW/cost.rs:37-42` |
| Live spend meter | None while a run is in progress. | absent | DOC parity row "Live credit meter" not_built; planned I16 |
| Screen-reader narration | Live region announces lifecycle, heartbeat and source tally rather than tokens. | shipped | SRC `ChatPage.tsx:449-540` |

## 11. Safety, permissions & governance

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| BFF authority boundary | Org derived from the verified session; per-audience delegated bearers (model, inference, execution, cost, session, data plane, ingestion, capability, sandbox); required mint failure returns 503; forged headers stripped. | shipped | SRC `GW/chat/streams.rs:24-48`; `GW/chat/shared.rs:145-356`; DOC `FE/FRONTEND_PLANE_STATUS.md` "Confirmed solid" |
| ZDR propagation | Body flag OR `x-zdr` header, then the org's standing posture raised one way before the turn leaves; malformed values preserved so MG rejects them. | shipped | SRC `GW/chat/shared.rs:35-118`; `GW/chat/streams.rs:99-109` |
| Temporary chat | No history entry, no local transcript, no server snapshot, no title or follow-ups; blocks voice and knowledge import. The toggle is disabled when no catalogue model attests ZDR, and no provider is attested on this stack (DOC), so it is unavailable in practice. | partial | SRC CMP `3055-3078`; `API/chat-client.ts:2563-2615`; `use-chat-controller.ts:468-480,557-566`; `CH/lib/chat-retention.ts:6-20`; DOC `PL/system-audits/PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md` section 5 |
| Typed ZDR refusals | Catalogue of refusal codes with localized actions. | shipped | SRC `API/zdr-refusals.ts`; `API/contracts/zdr-refusals.json` |
| Autonomy rungs | `read_only`, `workspace_write`, `danger_full_access`; only the last two are grantable from the plan card, and a grant needs a written reason. | shipped | SRC `API/chat-client.ts:1873-1882`; `ChatMessages.tsx:2536-2542` |
| Tool gating | Inline loop admits only read-only tools; execution-core maps every tool to a server-owned capability id and fails closed on unknown names; only 8 registry actions are model-executable. | shipped | SRC `MG/tool_loop.rs:625-645`; `EC/capability_policy.rs:737-840`; `FE/src/shared/actions/model-eligibility.ts:125-139` |
| Support and foreign-origin threads | Support-derived threads strip tools and pins; threads from other surfaces open read-only. | shipped | SRC `GW/chat/history.rs:709-770`; `use-chat-controller.ts:495-507`; `ChatPage.tsx:1013` |
| HTML sandbox | Generated HTML runs in a `srcdoc` iframe without same-origin. | shipped | SRC `ChatArtifactPanel.tsx:500-519` |
| Rate limiting | Per-org/user token bucket at the gateway with 429 and Retry-After. | shipped | SRC `FE/apps/gateway/src/rate_limit.rs:1-20`; `FE/apps/gateway/src/main.rs:367` |
| Moderation / injection defense | MG moderation primitives and a semantic content-safety shadow pass; enforcement on input/output is incomplete (DOC). | partial | SRC `MG/moderation.rs:1`; `MG/semantic_screening.rs:1`; DOC parity row "Safety" |
| Enterprise admin | Settings sections for members, platform users, billing, SSO, org security, trust, quotas. | shipped (outside chat) | SRC `FE/src/features/settings/components/WorkspaceSettingsPage.tsx:448-497` |

## 12. UI/UX & product design

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Layout | Calm conversation first; contextual canvas with Work / Output / Sources / Trace that the work opens (Work > Output > Sources, once per thread; Trace never steals focus); one composer node moved between empty state and dock; resizable rail 340-880 px keeping 480 px for the conversation, width persisted. | shipped | SRC `ChatPage.tsx:231-283,410-447,635-672`; `CH/lib/chat-surfaces.ts:1-60`; `CH/components/ChatWorkspaceCanvas.tsx:1-60` |
| Keyboard | Ctrl/Cmd+Shift+O new chat, Ctrl/Cmd+I focus composer, Ctrl/Cmd+Shift+Up/Down switch thread, Esc closes the panel and returns focus; Ctrl/Cmd+K and "/" open global search; focus returns to the composer after an answer. | shipped | SRC `CH/lib/use-chat-shortcuts.ts:1-110`; `ChatPage.tsx:542-617` |
| Shortcut sheet / command palette | None. | absent | SRC grep `CommandPalette\|command-palette\|cmdk` in `src`: none; DOC planned I1 |
| i18n | Norwegian and English (`no`, `en`). Some chat strings are still Norwegian-only (plan rung labels, tool status words, approval buttons, trace copy). | partial | SRC `FE/src/shared/i18n/locales.ts:1`; `ChatMessages.tsx:1917-1930,2030,2538-2542`; `ChatPanels.tsx:1425-1440` |
| Rendering performance | No list virtualization; the whole message is reparsed on every streamed token (blocks remount). | partial | SRC grep `virtual` in `src`: none in chat; `ChatChart.tsx:15-20` (comment); DOC planned I5 |
| Composer extras | Date autocomplete, work-period picker (time-scoped requests), entity overlay, expand textarea, drag-and-drop and paste. | shipped | SRC CMP `441-447,3436,3874-3944`; `FE/src/shared/actions/chat-work-period.ts` |
| Dashboard handoff | The dashboard composer launches into chat with the draft and attachments carried over. | shipped | SRC `CH/lib/pending-chat-launch.ts`; CMP `95` |
| Theme | Theme saved via the navbar action. | shipped | SRC `FE/src/shared/actions/action-registry.ts:2266` |

## 13. Platform architecture

| Capability | What exists | Maturity | Evidence |
|---|---|---|---|
| Frontend stack | SolidJS 2.0 RC, @solidjs/router 2 next, Vite 8, zod 4, KaTeX, Mermaid 12; feature-sliced folders. `@tanstack/ai` and `@tanstack/ai-client` are declared but not imported anywhere (INF: dead dependencies). | shipped | SRC `FE/package.json:22-39`; grep `tanstack/ai` in `src`: none |
| Gateway | Rust Axum BFF, 55 domain files, 45 routers merged; same origin; nginx proxies only `/api/`. | shipped | SRC `FE/apps/gateway/src/main.rs`; `FE/nginx.conf:74-76` |
| Persistence | Session-core (through MG) is the only conversation owner; the BFF stores no transcript; the browser keeps a derived localStorage cache reconciled against the server and suppressed under ZDR. | shipped | SRC `GW/chat/history.rs:272-277`; `CH/lib/chat-retention.ts:6-20`; `CH/lib/chat-thread-history.ts:11-13` |
| Conversation-node model | Turns derive to data-only typed nodes rendered by a keyed registry (DeepSeek pattern). | shipped | SRC `FE/src/shared/chat-nodes/types.ts:1-30,91-207`; `derive.ts:81`; `registry.ts:41` |
| Protocols | Native SSE taxonomy (primary); AG-UI adapter (unwired); no ACP or A2A. | partial | see sections 1 and 5 |
| In-memory runtime registries | Stream buffer, cancel, queue and idempotency registries are single-replica in-memory (DOC). | partial | DOC parity rows "Resume ... across a gateway restart", "Idempotent regenerate" |
| Unused gateway route | `/api/v1/briefs` (daily brief from insight-core) has no SPA caller. | stub | SRC `GW/briefs.rs:39,94`; grep `briefs` in `FE/src`: none |
| Demo mode | `src/demo` scripted product-demo scenes with mocked routes (marketing recordings). | shipped (demo only) | SRC `FE/src/demo/product-demo/DemoApp.tsx:141-142` |

---

## Planned / in-flight per docs

From `PL/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (waves) and the 2026-10-01 parity audit, checked against code where possible:

- **Wave 3:** B7 continue a plan after an approval pause; C6 a "Bred forskning" control in Do mode (not in the composer today, grep); C9 credential lifetime for long wide research; D10/D11 unattended identity and guards for scheduled runs; E9 "Open in browser" from a message and `/browse`; E10 live view and takeover of the agent's browser from the chat panel; F7 paged file text for the agent; F11b ownership of synced mail.
- **Wave 4:** H3 direct PDF without the print dialog; H4 charts inside Office exports (not built, grep); H6 stage attachments into the sandbox; H9 local OCR; H10 xlsx/pptx in knowledge imports; G4/G5 calendar and workbook reads; I1 shortcut sheet; I2 idempotency key per send (not built, grep); I5 block-level streaming render; I6 citation chips bound by number; I7 skills on the subscription route; I8 server-owned branch; I9 supersede primitive for regenerate/edit; I12 full-text chat search plus `recall_conversations`; I13 user slash templates; I15 visible reasoning on the subscription route; I16 live usage meter.
- **Wave 5:** G6 edit existing Office files in place; G7-G15 provider writes with always-ask and save to OneDrive/Drive; H8 designed deck path; I4 fast orphan settling; I10 server-persisted versions; I11 checkpoint and rewind; I17 canvas highlight-to-edit and restore.
- **Wave 6 (do not start):** E12 model-initiated `browser_agent`; H7 editable code cell; I19 share link; I20 copy chat into a Space; I21 server-synced project folders; I23 prove one learned skill.
- **ZDR program** (`PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md`, `BUILD_PASS_*_2026-10-1x.md`): typed refusal catalogue, per-connection ZDR waiver (designed, partly built, Z-15 held "do not ship"), Data Plane rerank. None of this is deployed beyond the Control Plane wave; no provider is attested ZDR-capable.
- **Frontend roadmap** (`FE/FRONTEND_PLANE_ROADMAP.md`): security IDOR fixes, de-fake residual settings rows, MCP discovery and health feedback, delete obsolete docs.

## Known gaps (code-verified unless marked)

1. **MCP tools cannot run from chat.** Registration and OAuth exist, but the inline loop denies MCP names and execution-core has no trusted capability binding for `mcp__*` (`MG/tool_loop.rs:625-645`; `EC/capability_policy.rs:741,965`). This is the biggest extensibility gap against Claude, Codex and Cursor.
2. **Persona, org and Space instructions do not reach Do runs** (`MG/sse.rs:9527-10128`).
3. **No share link, no import, no checkpoint/rewind, no server-side conversation search, no model comparison** (greps in sections 1, 4, 9).
4. **Versions and branches are client-side.** Regenerate versions die on reload; branch transcripts are browser-local; the BFF drops `turns` on save.
5. **Projects are a browser-only organizer**, not Claude/ChatGPT-style projects with shared files, instructions and memory.
6. **Artifacts are read-only** to the user: no canvas editing, no selection edit, no restore.
7. **Voice is push-to-talk only**: no duplex, no barge-in, no screen-share stream.
8. **Computer use from chat is not wired**: a chat turn cannot drive a browser; agent browser runs fail closed without a broker grant (DOC); no desktop control (parked).
9. **Wide research has no user entry point** and its cards are not in the durable replay.
10. **Scheduled runs are weak**: a single no-tool call on Verevon Balance (DOC).
11. **Temporary chat is unavailable in practice** because no provider is attested for ZDR (DOC + SRC gate).
12. **Plugins have no install, marketplace or runtime surface**; preset agents have empty action sets; only 8 of 168 actions are model-executable.
13. **Agent pages are mostly blueprints**: role workspaces, Workflow builder and Chatbot studio are design previews; the Fleet console calls `/v1/fleets` with no route (stub).
14. **Streaming performance**: no virtualization, full reparse per token.
15. **Unwired code**: AG-UI client, `uploadChatDocument`, `/api/v1/briefs`, `@tanstack/ai` deps.
16. **Live spend meter, shortcut sheet, user command templates** absent.
17. **i18n leaks**: several Norwegian-only strings in the chat chrome.
18. **Thread list capped at 80** in the BFF with no paging (`GW/chat/history.rs:21`).
19. **Single-replica in-memory registries** for resume buffer, cancel, queue (DOC).

## Architecture constraints that limit what Verevon can adopt

- **Plane ownership.** Model Plane owns reasoning, runs, sessions, sandboxes, browser grants and cost; the Frontend gateway is a thin normalizing proxy that may not store transcripts (`GW/chat/history.rs:272-277`). Any feature that needs durable conversation state (server branches, persisted versions, checkpoints, share snapshots, project memory, full-text search) must be built in session-core / model-gateway, not in the BFF or the SPA. Durable knowledge must live in Data Plane v2 (documents, retrieval); collaborative state in the Application Plane (Convex).
- **No cross-plane DB access.** The BFF mints per-audience delegated tokens for every hop; new features add audiences and Auth Core principals rather than shortcuts (`GW/chat/shared.rs:145-356`).
- **Model proposes, Quarry executes** for every browser action. Browser control from chat must go through Quarry-v2's agent lane and the broker grant; a client-side Chrome extension or CDP bridge contradicts the rule and is ruled out (DOC).
- **Effect authority.** Inline (Ask) tools must be read-only; any write goes through execution-core's capability policy, durable approvals and, for registry actions, the model-eligibility evidence bar. This is why MCP is blocked: MCP side effects are unverifiable until capability-core provides a durable binding (`MG/tool_loop.rs:625-640`).
- **Zero Data Retention must propagate.** Any new feature that persists content (memory, share links, snapshots, generated files, voice) must refuse or degrade under ZDR; Temporary chat already blocks voice, knowledge import and local caches. Today no provider is ZDR-attested, so ZDR features cannot be proven live.
- **Space access never widens an owner-plane ACL.** Space-scoped chat needs Control-signed decisions (`inject_personal_thread_context`, sandbox token only for Space turns); "move chat into a Space" needs Control authority and audience handling (DOC).
- **Effectful turns are immutable.** Regenerate, edit and continue are refused on turns with documented effects; any rewind design must mark provider writes as non-rewindable.
- **Subscription route limits.** On the user-owned ChatGPT route the body is stripped of tools, browsing, images, deep research, skills and attachments; the only live-testable route is this one (DOC), which caps what can be proven.
- **Channel Plane is docs-only**: no public widget or visitor-chat runtime to build against.

## Doc-vs-code discrepancies

1. `VEREVON_CHAT_UX_DESIGN_SPEC.md:3-6` says "Proposed for approval" and "Implementation deliberately not started"; the workspace canvas, Ask/Do control and cold start are implemented (`ChatPage.tsx`, CMP).
2. UX spec section 6 asks for "Spør / Gjør" and "Quick / Standard / Deep" with no raw model ids; code shows "Spør / Utfør" (CMP `3037-3050`), "Auto / Raskt svar / Dyp research" (CMP `384-405`) and a full model picker (sanctioned later in `VEREVON_CHAT_DESIGN.md` 3.5).
3. Parity audit 2026-10-01 says "no 'notify me when done' control in /chat"; `RunNotifyButton` now sits in the waiting indicator and the live run header (`ChatMessages.tsx:3685`, `ChatLiveRunPanel.tsx:535`, commit `f5b8e1c7` 2026-10-08).
4. Parity audit 2026-10-01 says charts have "no native chart renderer, no table-to-chart control"; `ChatChart.tsx` and `ChatTableTools.tsx` now provide both.
5. `VEREVON_CHAT_DESIGN.md` 2026-09-02 status item 7 says text attachments persist to Data Plane v2 through `/api/v1/chat/documents`; `uploadChatDocument` now has zero callers and small documents are inlined into the model content instead (`API/chat-client.ts:2081-2118`, `chat-types.ts:531-540`).
6. `VEREVON_CHAT_DESIGN.md` 2026-09-02 item 6 says `?space_ref=` was "verified live"; the controller has since removed the URL entry point (`use-chat-controller.ts:2274-2283`).
7. `CLAUDE.md` says API clients live under `src/shared/api`, `src/shared/rpc` and `src/shared/graphrest`; `shared/rpc` and `shared/graphrest` do not exist (the audit prompt's `shared/rpc` path also does not exist).
8. `FRONTEND_PLANE_STATUS.md` says `shared/api` has 56 client files and "~50-domain" gateway; today there are 62 non-test clients and 55 domain files. The 2026-07-13 note that the live model-gateway/inference listeners are absent predates the 2026-10-01/02 deploys and Terra proofs (`PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md`).
9. `FRONTEND_PLANE_STATUS.md` lists "agents" as MIXED with a real run console; the Fleet console route (`/agents/fleets/:fleetId`) calls a non-existent `/v1/fleets` path.
10. `FRONTEND_PLANE_ROADMAP.md` Phase D treats MCP as "form ready once backend OAuth exists"; MG now has the Streamable HTTP client and OAuth routes, but execution is still blocked by capability policy, which the roadmap does not mention.
11. The client comment in `buildChatWireBody` lists the MG builtins as "fetch_url, knowledge_search, get_weather, code_interpreter" (`API/chat-client.ts:1053-1060`); the real builtin set is about 24 tools including artifacts, inbox, statistics, shipping and social reads (`MG/tool_loop.rs:4323+`).
12. `VEREVON_CHAT_DESIGN.md` 9.x item 4 says "no localStorage pin store exists"; message pins are stored in localStorage (`CH/lib/chat-pinned-messages.ts:33`). The doc meant thread pins, which are server-owned; the wording is ambiguous.
