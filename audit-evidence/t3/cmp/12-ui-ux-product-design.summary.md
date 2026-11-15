# T3 / 12: UI/UX and product design: cross-source comparison

176 reference records from 12 sources were grouped into 38 clusters. 174 records are assigned to a cluster and 2 are excluded (a TUI transport architecture, and a model-compute item). By provenance, the members are 122 SRC, 51 DOC and 1 REA. The Verevon mapping uses 22 baseline records plus light read-only greps of `apps/Frontend Plane/verevonv3/src`. Full detail is in `12-ui-ux-product-design.clusters.json`.

## Shared baseline (table-stakes)
- **Agent-first shell (C-001).** Recents, Projects, Agents and Settings live in one shell, and every object has a deep link. Verevon has this only in part: chat, agent runs and Spaces are separate lists.
- **Attention model (C-006).** Notifications are typed by event (needs approval, needs input, done), with a level per type, sounds, badges and push. Verevon has this in part: it has a per-run "notify me when done" option, but no Web Push and no per-type levels.
- **First-run onboarding (C-010), 10 of 12 sources.** Verevon has an organisation wizard (crawl, connect, paywall). It is not verified whether that wizard seeds the personal persona or role-based starter prompts.
- **Theming (C-013).** Verevon is at basic parity.
- **i18n (C-019).** Verevon is Norwegian-first, but Norwegian-only strings remain.
- **Responsive and mobile (C-018).** Verevon has a responsive web layout. It has no PWA manifest and no app.
- **Common to most sources:** a command palette (C-004), keyboard-first use (C-005), settings IA (C-012), a contextual side pane (C-002), the home composer (C-009), performance (C-017), accessibility (C-016) and feedback surfaces (C-015).

## Best-of-breed
| Capability | Best source | Why |
|---|---|---|
| Notifications by event type (C-006) | Claude + Antigravity | Notifications are split into permission, idle and question, each set to off, badge or banner. "Action required" and "done" have distinct sounds. Web push reaches remote sessions. |
| Inbox-style work list (C-007) | T3 Code | Lists pinned, active, snoozed (wakes with a notice) and a collapsed "Settled (N)" group. Rows show status pills and elapsed time. Claude supplies the closed state vocabulary. |
| Command palette (C-004) | T3 Code | One reducer covers commands, threads (with content snippets), files and content search. The modes never stack. |
| Keybindings (C-005) | T3 Code + DeepSeek | Context-aware ("when") clauses with a key recorder and conflict flags. DeepSeek adds a double-Escape stop that is checked against the live turn. |
| Side workspace pane (C-002) | Claude + T3 Code | Each thread has its own tabs (diff, file, terminal, browser, PR, plan, artifact), and the agent can call a show_pane tool to open one. |
| Process-visibility levels (C-023) | DeepSeek | Four levels from Compact to Verbose, applied at turn, group and row. Activity titles come from a fixed vocabulary. |
| Agent-asks trays (C-025) | Cursor + DeepSeek | Trays for plan, questions, queue and goal dock above the prompt. Question cards can be timed and still accept late replies. The queue can be edited and promoted to steering. |
| Performance (C-017) | Claude (REA) + T3 Code | 257 lazy modules in three tiers, with reload when a chunk fails to load. The timeline is virtualised and hidden mounts are capped. |
| Theming (C-013) | T3 Code | About 56 colour roles, and an OKLCH generator that picks contrast-safe colours. |
| i18n gate (C-019) | DeepSeek + Hermes | A gate rejects hard-coded copy, a parity test keeps dictionaries aligned, and language packs are layered. |
| Design contract (C-014) | Hermes + DeepSeek | A written state checklist (empty, loading, reconnecting, degraded, exhausted), plus checks that enforce semantic tokens. |
| Long-session navigation (C-024) | DeepSeek | The server provides a turn outline, so the turn rail works before history has loaded. |
| Computer takeover (C-031) | Manus (DOC) | Choose, view, take over, pause and hand back control. A login done during takeover can carry over to similar tasks. |

## Differentiators worth noting
These are the run-state inbox (C-007), the prompt-anchored trays (C-025), the density levels (C-023), the turn rail (C-024), point-and-comment on previews (C-030, Cursor and Manus), takeover (C-031) and the agent and automation hubs (C-033, Perplexity, Manus and Cursor team agents).

## Biggest Verevon gaps (priority tier)
1. **C-033: Agent and automation hubs.** The Run Console is real. The Workflow builder and Chatbot studio are design previews, the Fleet console is a stub, and preset agents have no actions. This blocks the vision for the agent pages. **Model Plane, L.**
2. **C-006: Attention model.** Paused approvals stay silent unless the user opted in for that run. Fix: notification-core sends typed attention kinds, Web Push and a tab badge. **Application Plane, M.** ZDR-safe payloads are required.
3. **C-007: Run-state vocabulary and inbox.** One state enum in mp-contracts, shared by the sidebar and notifications. **Model Plane, S.**
4. **C-025: Agent-asks trays.** Plan and approval cards exist but sit inside the transcript, and there is no structured clarify question. **Frontend Plane + execution-core, M.**
5. **C-017: Render performance.** There is no virtualisation, and the whole message is reparsed on every token. **Frontend Plane, M.**

Recommended (selected):
- Command palette over the 168-action registry (C-004)
- Shortcut sheet and double-Escape stop (C-005)
- i18n lint gate, fixing the Norwegian-only approval buttons (C-019)
- Density setting (C-023)
- Toast, banner and connection pill (C-015)
- PWA with Web Push for approving from a phone (C-018)
- Inline limit notices (C-028)
- Takeover UX, where Quarry executes and the Model proposes (C-031, blocked on broker grants)
- A personal onboarding step that writes the persona (C-010)

Skipped:
- Desktop-only features (split windows, tray, quick entry)
- Hardware devices
- Plugin UI slots: they conflict with the authority model
- Slack-resident agents: Channel Plane is future
- IDE and PR surfaces: Verevon is not an IDE
- Community gamification: it goes against the works-council decision

## Verevon strengths (keep)
- **C-002, contextual canvas.** The canvas is opened by the work in a fixed order. Trace never steals focus, and the conversation keeps 480 px. This calm discipline beats IDE-style tab sprawl for a product that is not an IDE.
- **C-027, composer.** Ask/Do, Auto/Quick/Deep effort, server-resolved intent modes, and privacy-tier badges in the model picker.
- **C-016, accessibility.** The live region narrates lifecycle rather than tokens, charts are accessible SVG, and the focus ring has 4.5:1 contrast.
- **C-009, dashboard composer.** It has a work-period picker and hands draft and attachments over to chat.
- **C-019, language.** The product is Norwegian-first, and refusal codes are typed and localisable.
- **C-023, transparency.** The Work panel and the context inspector already exceed most references.
- **Honesty badges.** Unbacked surfaces carry honesty badges instead of fake features.

## Caveats
- **Unknowns and light checks.** C-011, C-015 and C-036 are "unknown". C-010, C-012, C-014 and C-018 rest on light greps, not on baseline records. A later step should verify them in code (paths are in each cluster).
- **DOC-only clusters.** C-029 (privacy center, ChatGPT only) and much of C-031 (Manus takeover strings) rest on DOC records. Their confidence is not upgraded.
- **Web benchmarks.** The web-only benchmarks (ChatGPT, Claude.ai, Manus web, Perplexity) are DOC and cannot show desktop-shell features. Prevalence for C-001, C-006 and C-018 was raised to table-stakes for that reason.
- **Overlap with chat-01.** Several clusters overlap with chat-01 (C-005, C-013, C-015, C-018, C-022, C-027). Tiers were kept consistent with that comparison.
- **Working tree.** The Verevon working tree had uncommitted changes in the chat area at baseline time.
