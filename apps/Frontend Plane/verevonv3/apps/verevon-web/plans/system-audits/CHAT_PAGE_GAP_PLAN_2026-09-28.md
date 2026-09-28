# Chat page gap plan (2026-09-28)

**Owner direction, 2026-09-28:** the web app comes first, and right now that means making sure the **chat page** has everything it needs.
- **Close the gaps with:** Manus, Perplexity, Hermes Agent.
- **Surpass as a web app:** claude.ai, chatgpt.com, the DeepSeek harness, OpenCode, T3 Code.
- **From Microsoft Copilot:** connectors, Office documents, data protection, and the agentic stack, meaning Verevon controlling Chrome and working with Excel on the web.

Windows-shell features are parked (see `COPILOT_APP_PARITY_2026-09-28.md`).

**How this was built:** workflow `chat-page-gap-analysis`, with 13 sonnet agents:
- 5 read-only inventories:
  - the chat UI (111 items);
  - the agent backend (28);
  - the chat and harness docs (59);
  - the space and QM docs (42);
  - the Copilot priorities (15);
- 6 competitor researchers: Manus (38 features), Perplexity (35), Hermes Agent (38), claude.ai (32), ChatGPT (31), and DeepSeek + OpenCode + T3 Code (34);
- a synthesis agent;
- a critic who spot-checked 15+ claims in the repo.

The critic's corrections are applied below. The raw JSON is in the session scratchpad (`gap_result.json`).

## 1. Verdict

The chat page is already unusually strong where things are hardest to fake:
- durable-run transparency: plan, todo and lineage; SSE reconnect and replay; inline approvals; run receipts; a context-token inspector;
- a server-enforced privacy, ZDR and residency router;
- per-turn memory disclosure with correct and forget;
- the effectful-turn lock.

On governance legibility that puts Verevon ahead of Manus, Perplexity and Hermes. On agent-run transparency it is ahead of claude.ai and ChatGPT.

The gaps fall into three groups:
1. **Built but not wired to chat:** Outlook and Gmail read, cron scheduling, per-thread delete and rename. These are cheap to close.
2. **Real gaps against the systems to catch up with:**
   - Office export;
   - reading file contents from OneDrive and Drive;
   - calendar write, contacts, and Excel/Sheets workbooks;
   - Verevon controlling the user's own Chrome;
   - Wide Research scale;
   - a source-scope filter;
   - share links;
   - persona instructions.
3. **Table stakes against claude.ai, ChatGPT and T3:** highlighted, copyable code in the transcript, charts, and a shortcut sheet.

## 2. Already ahead (evidence)

- **Plan-approval rungs, inline tool approvals and run receipts:** `src/features/chat/components/ChatMessages.tsx:1791`, `:1531-1568`; `ChatPanels.tsx:1181-1320`.
- **Assembled-context inspector** (tokens against budget, per segment): `ChatPanels.tsx:535-630`. No researched competitor exposes this.
- **Verification notice** that separates "never checked", "checked and found nothing" and "confirmed against N sources": `ChatMessages.tsx:2096-2168`.
- **Memory recall disclosure** with inline correct and forget, and stated/inferred provenance: `ChatMessages.tsx:2179-2280`.
- **Privacy tiers and residency**, fail-closed:
  - `inference-core/src/provider/mod.rs:144,489-591`;
  - `fallback.rs:467-490`;
  - ZDR enforced per request in `grpc.rs:176-187`;
  - tri-state claims in `model-gateway/src/auth.rs:55-73`.
- **Effectful-turn lock** (an effectful turn can't be silently edited or regenerated): `src/shared/chat/effect-class.ts:15`.
- **Governed browser takeover:** `src/features/dashboard/home/BrowserChrome.tsx:497,624,720`.
- **Declarative pre- and post-tool hooks:** `execution-core/src/hook/mod.rs`.
- **Norwegian-first:** Brønnøysund lookup from message text, `src/features/chat/lib/brreg-action.ts`.
- **Mid-run input and stream resume:** `use-chat-controller.ts:2096`, `:1450-1669`.

## 3. Gap matrix (corrected)

Tiers:
- **T0:** closes a gap with Manus, Perplexity or Hermes, or is an owner Copilot priority.
- **T1:** needed to surpass claude.ai or ChatGPT.
- **T2:** surpasses the DeepSeek harness, OpenCode or T3 Code on speed and craft.

| Capability | Best example | Verevon today (evidence) | Gap | Tier |
|---|---|---|---|---|
| Outlook mail read and search from chat | Copilot; Manus Mail | Executor has `mail.messages` (`integration-corev2/internal/actions/service.go:104`), but it is missing from the agent catalog (`execution-core/src/integration_tools.rs:106-112`) | S | T0, **Wave 1** |
| Gmail read and search from chat | Copilot; Perplexity | `gmail.messages` exists (`service.go:172`), not in the catalog | S | T0, **Wave 1** |
| OneDrive file listing from chat | Copilot | Listing exists (`service.go:110`), not in the catalog (Google `drive.files` is) | S | T0, **Wave 1** |
| OneDrive, SharePoint and Drive file **content** | Copilot; Perplexity | Metadata listing only | M–L | T0 |
| Outlook and Google calendar **write** | Copilot; Perplexity | No write op despite the scopes. Business tenants need admin consent (Copilot report §2) | L | T0 |
| Contacts (Outlook People, Google People) | Copilot | Absent | L | T0 |
| Excel workbook and Google Sheets read/write | Owner ask ("Excel through the web") | Absent. Code interpreter can only create *new* xlsx | L | T0 |
| Attach from a connected source in the composer | Perplexity; ChatGPT @apps | Composer shows a read-only status list (`DashboardComposer.tsx:4394-4449`) | M | T0 |
| **Export an answer to docx, xlsx, pptx and PDF** | Copilot (button on answers ≥600 chars) | **No backend or UI** (critic: `artifacts.rs` only emits files the model chooses to generate) | M | T0, **Wave 1**, client-side |
| Upload .xlsx and .pptx | Copilot | Accept list excludes them (`DashboardComposer.tsx:2838`) | S | T0, **Wave 1** |
| Slide-deck generation (research, design, images) | Manus; Perplexity Labs | Absent | L | T0 |
| Source-scope filter (org / web / both) | Perplexity Focus | A different axis exists (workspace vs conversation) | S | T0 |
| Inline citation chips at claim boundaries | Perplexity; Claude | Client is ready (`chat-normalizers.ts:1295-1303`); nothing on the SSE side fills it | M | T1 |
| **Wide Research scale** | Manus (100+ parallel agents) | **Already parallel** (`runtime_loop/agent.rs:1402`, `join_all`), but capped: `MAX_TOTAL_CHILDREN=8`, `MAX_DEPTH=1` (`subagent/mod.rs:27,55`) | M | T0 |
| Editable plan with a confirm gate | Manus Plan Mode | Rung approval exists (`ChatMessages.tsx:1791`), but the plan isn't editable | S | T0 |
| Background task with completion notice | Claude Cowork; ChatGPT; Perplexity | Durable RunAgent exists; no notification binding | M | T1 |
| **Verevon controls the user's own Chrome** | Manus Browser Operator; Claude in Chrome; Comet; Atlas | Only a Verevon-hosted Chromium (`tool_bridge/mod.rs:82-113`) | L | T0 |
| Live window share | Copilot Vision | One-shot `getDisplayMedia` screenshot (`DashboardComposer.tsx:1691-1724`) | M | T0 |
| OS-level computer use | Copilot Actions | Absent. Outside the web app, parked with Windows | L | parked |
| "Schedule this" from chat | Manus; Perplexity; ChatGPT; Claude | Cron exists (`CronSchedulesSection.tsx`, Space Work tab) but can't be reached from `/chat` | M | T0 |
| Shareable read-only link | claude.ai; ChatGPT; Manus; Perplexity Pages | Absent | M | T1 |
| Presence in a plain chat | Manus Collab | Built for Space rooms only (`SpaceRoomComposer.tsx`) | M | T0 |
| Persona / custom instructions | Claude Styles; ChatGPT; Perplexity | Absent | M | T0 |
| Highlighted, copyable code in the transcript | claude.ai; ChatGPT; T3 | Plain `<pre><code>` (`ChatMessages.tsx:960-976`) | S | T1, **Wave 1** |
| Per-thread rename and delete | Every competitor | **Endpoints exist** (`apps/gateway/src/domains/chat.rs:19-26`; `history.rs:70,259,436`); sidebar has only pin and bulk clear (`CoreSidebar.tsx:479-484`) | S | T1, **Wave 1** |
| Charts from data | ChatGPT; Manus | No chart renderer | M | T1 |
| Live and editable code REPL | ChatGPT; Claude analysis | Replay only | M | T1 |
| Code mode: tools callable from the sandbox | DeepSeek harness; Claude Code | Absent (backlog #11) | L | T2 |
| LSP-grounded code edits | OpenCode | Absent | M | T2 |
| Keyboard-shortcut sheet | claude.ai; T3 | Absent (the bindings exist in `use-chat-shortcuts.ts`) | S | T2 |
| Block-level streaming re-render | T3 Chat | Unverified | M | T2 |
| Full-duplex voice with barge-in | ChatGPT; Claude; Hermes | Voice dialog exists; duplex unverified | M | T1 (verify first) |
| Live credit meter while running | Perplexity; Manus | Space hard-stop only | L | T1 |
| Create or move a chat into a Space from `/chat` | Claude, Manus and Perplexity Projects | `spaceRef` is forwarded for already-scoped threads (`chat-client.ts:962`); no control for new ones | M | T1 |
| "Forward an email to create a task" | Manus Mail | Absent. Cheaper than full mail parity | S–M | T0 |
| Third-party audit of the data-protection design | Microsoft EDP (audited) | Code is real and tested; no external audit | owner | T0 |

## 4. Waves

**Wave 1: done and live on freeze 5 (2026-09-28T02:01Z).** It was built by workflow `chat-wave1-implement` (5 implementers, 5 adversarial reviewers, 1 final verify).

The reviewers fixed seven real defects before verification:
- a markdown `javascript:`/`data:` link could become a live hyperlink in Word;
- PowerPoint could produce an empty slide before an overflowing paragraph;
- heading markdown leaked into Excel sheet names;
- an xlsx or pptx with a package-absolute relationship target silently lost a sheet or slide;
- a failed delete of the open conversation stranded the user on a blank chat and lost its cached transcript;
- the row-menu trigger was invisible to keyboard focus;
- the copy state was lost on re-render.

Live verification found four more, all fixed:
- **The document is the artifact.** Verevon replies are short notes, and the draft lives in the Resultat panel. Export was therefore added to the panel ("Eksporter": Word, Excel, PowerPoint, PDF). A short reply that produced a document now exports that document under its own title.
- **Duplicate title in Word.** A leading heading now becomes the document title and file name. The old 60-character preview title had also carried raw `##`/`**` into file names.
- **Literal `**` in PowerPoint table cells.** Cells are now stripped of inline markdown, and a wholly bold totals cell stays bold.
- **The panel menu was clipped.** It now opens downward, aligns itself inside the panel, and scrolls into view.

Evidence:
- **Tests:** 1158 of 1159 frontend tests pass. The failure is a stale `CoreShell.test.tsx` test from before the 2026-09-24 privacy-tier work, flagged as a separate task. execution-core passes 589 of 589.
- **Checks:** `tsc -b` clean; eslint 0 errors.
- **Real Office:** sample and live exports opened in real Word, Excel and PowerPoint, invisibly and read-only, and were saved as PDF and inspected:
  - Word: headings, bold, links, a right-aligned numeric table with a bold totals row, nested and numbered lists, monospace code, sources and footer;
  - Excel: Norwegian `1 234,50` stored as a number, percentages kept as text;
  - PowerPoint: 8 slides from a real sales-report draft, with a real table.

Items:
1. **Export an answer to Word, Excel, PowerPoint and PDF.**
   - Built in the browser with no new dependencies (`src/features/chat/lib/chat-export/`). Nothing leaves the browser.
   - A visible button on answers of 600+ characters; the same options in the "…" menu on every answer.
   - PDF goes through the browser's print-to-PDF, from the rendered answer.
2. **Highlighted, copyable code blocks** in the transcript, reusing `chat-code-highlight.ts`.
3. **Per-conversation rename and delete** in the sidebar, on the existing endpoints.
4. **.xlsx and .pptx uploads**, on top of what the imports extractor supports.
5. **Outlook mail, Gmail and OneDrive read tools** in the agent catalog (execution-core). Reads don't trigger write approvals.

All five are live: the launcher `--from model` rebuild ran at 2026-09-28T01:28–01:41Z, and the frontend container was restarted after it. Note that the frontend container runs Vite dev over a bind mount with watching disabled, so a source edit reaches the app only when the container restarts.

Known limits:
- Italic text containing inline code keeps its backticks, in the export and in the chat renderer alike. `parseInline` doesn't nest.
- Mail search is read-only listing. The executor ignores query parameters.
- "Import to knowledge" still rejects .xlsx and .pptx (imports-core `ALLOWED_FILE_TYPES`).

**Wave 2 progress (2026-09-28).** Specs are in `chat-wave2-specs-2026-09-28/`. Every item had an implementer and an adversarial reviewer, then a whole-batch verify.

- **Batch 1, built:**
  - **Inline citation chips.** Chips are bound to claims by a post-stream anchor pass, and sources are grouped by domain or provider in the chip and the Kilder tab. This also fixed a knowledge-base citation off-by-gap bug.
  - **Composer picker.** Attach OneDrive-personal or Google My Drive files, Outlook or Gmail messages, and calendar events. Content reads are size-capped and go through the imports extractor, provenance lands in the receipt, and the integration egress client's redirect bug is fixed with a host allow-list.
  - **Mail-to-task plumbing.** Postmark receiver, self-forward verification, alias service, the `mail_task` origin (migration 0041), and a settings card. It is off until the owner supplies the Postmark account and domain.
- **Batch 2, built:**
  - **Source-scope filter** ("Kunnskap og nett", "Bare kunnskap", "Bare nett"). It is enforced server-side at tool dispatch, shown as a receipt badge and a context-inspector line, and carried in citation `origin`. The reviewer fixed a high-severity bypass through Plan mode.
  - **"Planlegg dette".** Available from the reply menu and `/schedule`. It has four Norwegian cadence presets, Europe/Oslo, and is draft-only. Your own schedules can be listed, paused and deleted from chat.
    - The creator can schedule in their own personal Space.
    - Phase B lets the owner read their own scheduled-run threads (user-core).
    - Settings no longer offers the six workflow types that could never fire.
  - **Custom instructions.** Settings has a section, and the context inspector shows them as their own segment. They can be switched off per thread (migration 0042, ZDR-guarded). An `@`-mentioned agent takes precedence.
  - **Personal connections are owner-only**, on every route including sync jobs. The reviewer found and closed the sync-job hole.
  - **Latency levers**, all on by default with env switches:
    - `tiny_candidate_skips_propose_edits`;
    - `targeted_source_repair`;
    - `list_item_segments`: bullet lists are reviewed item by item, like table rows.
- **Caught in verification:** the new source-scope code panicked on every default-scope turn (an eager `then_some` over an `unreachable!()` arm), failing 71 of 86 e2e tests. Six agents had blamed it on others; the verifier fixed it.
- **Test status:** model-gateway 1542 lib and 86/86 e2e, session-core 381, gateway BFF 583, and every Go package pass, with typecheck clean.
  - The one frontend failure is `CoreShell.test.tsx`. It comes from another session's "Søk" to "Nett" relabel in `DashboardHomeChrome.tsx`, not from this work.

**Open after Wave 2:**
1. **Delegated identity for services acting for a user.** It unblocks two things:
   - the chat agent's use of *personal* connections, which execution-core now correctly refuses because it conveys no end user;
   - the mail-task thread creation, since `PrepareUserOwnedThread` exists but model-gateway has no service credential.
   
   This is the next item.
2. The "Se resultater" link in the schedules panel needs a `cron_fires.thread_id` link and a runs endpoint.
3. Wave 2 batch 3: the editable plan gate and wide research.

**Wave 2 (close Copilot, Manus and Perplexity gaps; moderate):**
- "Schedule this" from chat, reusing the cron API;
- the source-scope filter;
- the attach-from-connector picker;
- persona instructions;
- the editable plan gate;
- raising the subagent ceiling (8 → configurable, with a cost guard);
- "forward an email to create a task";
- inline citation chips (SSE producer).

**Wave 3 (real backend work, owner decisions first):**
- calendar write, contacts, and Excel/Sheets workbook read/write, with approval design and the Entra admin-consent path;
- reading file contents from OneDrive and Drive;
- the user's-own-Chrome bridge (extension);
- live window share;
- background-task notifications;
- share links.

**Wave 4 (surpass the dev harnesses):**
- block-level streaming re-render;
- a chart renderer;
- the shortcut sheet;
- an editable code REPL;
- code mode;
- LSP diagnostics;
- a voice barge-in check.

## 5. Owner decisions

### Decided by the owner (2026-09-28)

| Question | Ruling |
|---|---|
| Who may create a schedule from chat | **The creator, in their own personal Space.** Admins keep org-wide control. The existing Control check verifies personal-Space ownership. |
| Unattended (scheduled) runs and effects | **Draft-only.** A scheduled run produces drafts and stops before any effect; approval happens in the thread afterwards. Pause and resume for unattended approvals stays Wave 3. |
| Inbound mail for "forward an email to create a task" | **Postmark inbound, self-forward only** (only mail from the user's own verified address). The owner sets up the Postmark account. |
| Chrome control | **Hosted, governed browser first.** Improve the live view, takeover and approvals. The user's own Chrome and its logins are not driven. |
| Who may use a personal Microsoft or Google connection | **Only the person who connected it**, including through the agent acting for them. Org-wide connectors stay org-shared. This fixes the pre-existing org-only check in integration-corev2 `AssertOrgAccess`. |

**Engineering defaults adopted** (from the Wave-2 critic, `chat-wave2-specs-2026-09-28/00-critique-and-sequence.md` §4):
- Source scope and persona are stored per turn, as metadata; the thread-immutability code is untouched.
- No new tool becomes agent-autonomous through the new filters.
- The connector picker covers OneDrive-personal and Google My Drive; SharePoint sites come later.
- Schedules default to Europe/Oslo with four cadence presets.
- Mail-to-task gets its own `mail_task` thread origin.
- Wide research divides its budget evenly, with concurrency starting at 4 on the subscription route.
- Plans: the last one proposed wins; there is no revising after a reject.
- Citation claims bind on the first occurrence only; there is no uncited-claim UI yet; the cross-producer counter fix is deferred.
- Persona: an explicitly summoned agent wins, and persona text follows the org-instruction ZDR precedent.

### Still open

1. **Write approvals** for calendar, Excel and Sheets: always ask, or trusted after N clean runs? (Wave 3.)
2. **Contacts scope:** a new personal-data scope; needs compliance sign-off.
3. **Share-link model:** an unguessable public snapshot, or named recipients only?
4. **Collaboration on `/chat`:** add presence here, or keep multi-user work in Spaces?
5. **External audit** of the data-protection design, to claim parity with Microsoft EDP.

Chrome control and the scheduled-task posture were decided on 2026-09-28 (above).
