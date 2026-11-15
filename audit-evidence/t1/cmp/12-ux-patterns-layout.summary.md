# T1 / 12 UX patterns & layout: cross-source comparison

Inputs: 68 reference records (buzz 12, grokbot 20, qm 13, slack 11, teams 12), 11 Verevon baseline records. Output: 32 clusters; 67 records assigned, 1 excluded (grokbot-agent-product-158, Popover-API overlay root, an implementation detail). Detail: `12-ux-patterns-layout.clusters.json`.

## Shared baseline (table stakes)

- **Primary navigation IA** (C-001, 4/5): a rail or app bar plus a sidebar with first-class destinations (Home/Inbox, Activity, Agents, Files, Later). Verevon is **partial**: it has one sidebar, but no cross-room destinations.
- **Personal sidebar organisation** (C-002, 4/5): stars, custom sections, sorting per group, drag-and-drop and batch actions. Verevon is **partial**.
- **Contextual right pane** (C-005, 3/5): one shell for threads, agent sessions and details. Verevon is **partial**: its pulse rail is status-only.
- **Thread side panel** (C-006, 3/5, Buzz counted through buzz-product-172). Verevon is **absent**.
- **Typed transcript rows** for mixed human and agent rooms, with grouping and collapse (C-012, 3/5). Verevon is **partial**.
- **Empty and zero states** (C-015, 3/5). Verevon has **shipped** these and is ahead on rendering "empty" separately from "unavailable".

## Best of breed

| Capability | Best source | Why |
|---|---|---|
| Navigation IA | Buzz (+ Teams app bar) | Agents and Workflows are first-class destinations. One right panel serves threads, canvas, members and agent sessions. Teams adds keyboard drag-and-drop with spoken narration. |
| Sidebar organisation | Buzz | Sort per group, synced sections and stars, and a More-unread pill. Grok Bot adds hide and batch actions. |
| Right pane | Grok Bot | Close linger, inert-while-closing, focus hand-off and a labelled separator for resizing. |
| Thread panel | Slack (+ Teams inline reply) | Side panel, pop-out window and a Threads view. Teams lets you reply in place from an inbox card. |
| Typed rows | Grok Bot / Buzz / QM | Grok Bot defines the row-kind contract and collapsible machine groups. Buzz labels agent owners. QM collapses completed work. |
| Ambient agent activity | Buzz + Grok Bot | Headline strip near the composer with click-through. Grok Bot's latch and threshold rules stop flicker. |
| Room home / header | QM (+ Slack tabs) | Scope page and a top bar showing tool counts. Slack lets users add their own tabs and bookmarks. |
| Accessibility | Grok Bot (+ Teams narration) | Inert outgoing pane, focus to the heading, an announcer keyed by tick, and row narration. |
| Timeline virtualization | Buzz | Tested anchoring policy on a patched virtua. Grok Bot adds FLIP for content that shifts. |
| Honest states | Buzz (DOC) / Verevon (SRC) | "Unknown is not zero" and "facts, not timers". Verevon enforces this in code. |
| Shell failure UX | Teams + Slack | Named start-up milestones, and a cannot-connect page with a log download. |
| Composer | QM (+ Slack) | Model presets, paste chips and slash commands. Slack adds formatting, clips and send-later. |

## Differentiators worth carrying

- Ambient agent-activity strip with hysteresis (C-010).
- One event log seen through several lenses (C-013; Verevon already has this).
- Selection-to-context hand-off, which passes references rather than content (C-028).
- Unified "To:" picker that finds or creates agents, people and groups (C-029).
- Agent computer preview that expands from a thumbnail to a modal (C-021).
- Multiview tiling (C-007, QM only).
- Plain-language activity copy (C-011).
- Honest-state UX (C-017; Verevon shares this).

## Verevon's biggest gaps (priority)

1. **No thread side panel** (C-006, sig 5). Activity emits `?thread_id=` deep links, but nothing reads them.
2. **No cross-room navigation destinations** (C-001, sig 5): no Home/Inbox, Agents or Activity outside the room. Depends on Application Plane aggregates.
3. **No contextual pane shell** (C-005, sig 4). It is the enabler for threads, live runs and members, and the Inbox resize logic can be reused.
4. **Agent work is opaque in the room** (C-010 and C-012, sig 4). Status lines exist, but there are no step headlines, no click-through, no typed tool or step rows and no grouping.

Recommended next, in rough order:
- Composer attachments and slash commands (C-009). These wait on the human-only message path from category 01.
- Agent avatar colour (C-020). The colour is collected but never displayed.
- Command palette (C-008). Cmd/Ctrl+K currently opens web search.
- Localise the activity grammar (C-011). It is hard-coded Norwegian.
- Virtualization and scroll anchoring (C-014). Nothing exists in `src/features/spaces`.
- Root error boundary and a cannot-connect page (C-018). The code says there is no root ErrorBoundary.
- Shared announcer (C-023).
- Selection-to-context (C-028).
- Unified picker (C-029).
- Agent computer preview (C-021), once a per-agent computer exists.

Skip for now:
- Org colour themes.
- Mini-mode: Teams ships it disabled.
- External-surface continuity cues: Channel Plane is docs-only.

## Verevon strengths (keep)

- The six-tab cockpit (Chat, Work, Knowledge, Activity, Agent, Members) shows more of an agent's working context than Slack or Teams.
- The Activity grammar (verb, object, outcome) is ordered by consequence and has lenses. It is equal to or ahead of Buzz and QM on "one log, many lenses".
- "Never go dark" is enforced in code: the `unavailable[]` contract, and every tab tells empty, could-not-load and not-published apart. One exception to fix: the silent 403 shared-read refusal at `SpaceRoomTimeline.tsx:201`.
- Destructive actions arm in place before they run, and personal-Space deletion returns a receipt from each plane.

## Caveats

- Prevalence counts come only from the records. Missing records undercount some prevalence: for example, Teams and Buzz almost certainly have quick switchers (INF), but only Slack and Grok Bot records cover them.
- Slack's product-surface records are mostly DOC (help text). Buzz's honest-state and lens rules are DOC. Grok Bot records are tagged SRC but come from minified bundles, and some carry medium confidence.
- I checked these Verevon statuses not covered by baseline records with targeted greps only, on 2026-10-13: virtualization, scroll anchoring, hover cards, error boundary, reduced motion and density. C-026 (What's New and feedback) and C-030 (link interstitial) stay **unknown**.
- The baseline notes a product-stance conflict. Verevon's docs reject "chat parity" features, while the audit goal is a hybrid of Teams, Slack and Grok Bot. The tiers here follow the audit goal.
- All write paths still need Control decisions for each append, plane ownership and ZDR propagation. Several gaps depend on push realtime and cross-room aggregates (categories 05 and 09).
