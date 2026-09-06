# Verevon Chat — UX Design Specification

**Status:** Proposed for approval  
**Date:** 2026-08-31  
**Scope:** Chat cold start, conversation flow, contextual workspace, composer, and responsive behaviour  
**Implementation:** Deliberately not started in this pass

## 1. Product thesis

Verevon is a calm AI chat that grows into a work environment only after the
conversation produces work worth inspecting.

> IDE-like capability, not IDE-like cold-start UI.

The conversation remains the command centre at every desktop state. Sources,
plans, files, browser sessions, generated pages, approvals, and receipts live in
one contextual workspace beside it. Opening that workspace must never remove the
composer or make the user leave the conversational flow.

## 2. Reference states

- [Cold-start concept](./verevon-chat-cold-start-concept.png)
- [Active-work concept](./verevon-chat-active-work-concept.png)

These concepts define hierarchy and interaction density. They are not permission
to copy typography, logos, or visual assets from another product.

## 3. Current UX audit

### Blocking

1. **The composer disappears when a workspace destination is selected.** In the
   live Tasks state the conversation remains visible, but the composer is
   `display: none`. The user cannot inspect work and immediately guide it.
2. **Workspace navigation is owned by the wrong region.** Chat, Actions, Output,
   Tasks, and More occupy the conversation header while the right canvas repeats
   the active destination as another heading. This creates two navigation models
   for one workspace.
3. **Tasks presents an event exhaust rather than a work summary.** Connection,
   model selection, search, tool payloads, retries, and user-relevant steps have
   almost equal visual weight. The user must interpret system plumbing to answer
   “What is happening?”

### Major

4. **Action hierarchy is duplicated.** Regenerate and New chat exist both inside
   Actions and as adjacent icon buttons. The header has several competing primary
   controls.
5. **The surface vocabulary mixes objects and actions.** Tasks is content inside
   Work; Actions is a menu; Output is a destination; More hides destinations.
   They should not all look like peer tabs.
6. **The empty state is generic.** A large question and pill prompts explain
   neither Verevon's governed knowledge scope nor the transition from Ask to Do.
7. **Provider and token metadata competes with the answer.** Raw model labels and
   per-turn tokens belong in details or Trace unless an administrator explicitly
   requires them in the reading flow.
8. **Errors are too close to raw runtime language.** Tool cut-offs and provider
   fallback messages need a user-facing outcome, cause, and next action; raw
   payload detail belongs behind disclosure.

## 4. Information architecture

### Global app chrome

Keep the existing narrow product rail and top organisation bar. Chat should not
add another permanent navigation sidebar. Thread history opens from the rail or
composer and becomes persistent only when the user chooses it.

### Conversation region

The conversation owns:

- thread title and quiet status;
- transcript and per-message actions;
- queued guidance and local errors;
- the persistent composer;
- New chat and a single overflow menu.

It does **not** own Work, Sources, Output, or Trace tabs.

### Contextual workspace

The right canvas owns one local tab strip:

| Norwegian | English | Purpose |
|---|---|---|
| Arbeid | Work | Plan, current phase, approvals, tools, and agent/browser activity |
| Kilder | Sources | Grounding scope, cited evidence, provenance, and conflicts |
| Resultat | Output | PDFs, files, tables, generated documents, and HTML/browser previews |
| Spor | Trace | Completed audit trail, receipts, retries, cancellations, and disclosure |

Chat is not a canvas tab; closing the canvas returns focus to the conversation.
Actions is not a canvas tab; actions stay near the object they affect or in one
overflow menu.

Only evidence-backed destinations appear. The panel never opens empty in the
ordinary Ask flow.

## 5. Screen states

### A. Calm cold start

- No right canvas.
- Verevon mark, concise welcome, and one line explaining governed knowledge.
- Three starter rows, not a card grid. Each row inserts a prompt; it does not
  submit automatically.
- One composer is the focal point.
- Ask is selected, Knowledge is visible, Standard effort is the default.
- Secondary features are available through progressive disclosure.

### B. Normal Ask conversation

- Header contains the thread title, status if relevant, New chat, and one menu.
- Transcript width remains comfortable (approximately 680–760 px).
- Composer stays docked and compact; it expands with attachments or multiline
  input.
- Citations may summon Sources on the first meaningful evidence event.

### C. Active work

- The right canvas is summoned by work and is resizable.
- Conversation and composer remain usable.
- Work opens for an effectful or multi-step run.
- Output opens for the first durable artifact.
- Sources opens for the first meaningful citation.
- Trace becomes available at completion but never steals focus.
- After the user selects a canvas tab, future events never replace that choice.

### D. Completed work

- Work collapses to an outcome summary: completed, needs attention, failed, or
  cancelled.
- Output remains the primary place to review created material.
- Trace holds receipts and implementation detail.
- The composer asks for the next instruction and remains the fastest route to a
  revision.

## 6. Composer model

The composer has three layers, revealed only when needed:

1. **Input:** message, attachments, queued guidance.
2. **Primary controls:** Ask/Do, grounding scope, effort, attach, voice, send/stop.
3. **Secondary controls:** temporary chat, image generation, research options,
   history, settings, and advanced model transparency.

Rules:

- Ask/Do is a real two-option segmented control, not an icon plus changing label.
- Use “Spør / Gjør” in Norwegian and “Ask / Do” in English.
- Grounding is always visible: “Kunnskap” or “Kunnskap + nett”.
- Show Quick / Standard / Deep. Do not show raw provider model IDs in the default
  composer.
- One send action. During a run it becomes Stop; queued guidance remains possible.
- Temporary/ZDR state is visible when active but does not compete with the
  primary intent controls.

## 7. Work surface hierarchy

The Work surface answers four questions in order:

1. **What is Verevon doing now?** Current phase and plain-language status.
2. **What is left?** Compact plan with completed/current/upcoming groups.
3. **Does Verevon need me?** Typed approval, blocked, or ambiguous callout.
4. **What happened underneath?** Collapsed activity disclosure.

Do not place raw connection events, model selection, tool JSON, and content
steps in one flat timeline. Runtime events remain available under activity
details or Trace.

## 8. Output surface hierarchy

- A single horizontal artifact/file selector at narrow canvas widths.
- A compact local toolbar for preview, code, copy, download, revision, and open.
- The selected viewer uses the remaining width and height.
- Files of different types share the canvas shell but keep native renderers.
- Generated HTML remains sandboxed and visually labelled as generated content.

## 9. Visual direction

- Warm neutral base, graphite text, earth/rust Verevon accent, restrained teal
  for verified system state.
- Open layouts and hairline separators; avoid nested cards around every region.
- 8 px spacing rhythm with 6–10 px control radii and 12–14 px content frames.
- Transcript typography is calm and editorial; utility labels are compact.
- Motion is functional: 160–220 ms canvas reveal, tab change, and composer
  expansion; honour reduced motion.
- No gradients, glassmorphism, decorative telemetry, raw terminal styling, or
  permanent developer chrome.

## 10. Responsive model

- **≥ 1180 px:** split conversation and resizable canvas; preserve at least
  480 px for the conversation.
- **760–1179 px:** the conversation keeps at least 480 px and the canvas takes
  the rest, up to 58% of the workspace; the composer remains mounted.
  *Amended 2026-09-04 (audit item 24).* This band originally read "canvas may
  cover 48–58%", which cannot hold together with a readable conversation at the
  low end: 48% of 760 px leaves 395 px of transcript, and the same 480 px floor
  this document mandates above 1180 px is the better rule. `ChatWorkspaceCanvas`
  already enforces it through `MIN_CONVERSATION_WIDTH`, and the canvas width is
  a persisted user resize clamped against it — so the measured 31–40% share was
  that floor working, not a defect. The share ceiling stays as guidance for wide
  viewports, where both rules fit.
- **< 760 px:** canvas becomes a full-screen sheet with a clear back-to-chat
  action. Never squeeze chat and preview side by side.
- At 200% zoom the same mobile-sheet rule applies based on available container
  width, not device identity.

## 11. Interaction acceptance criteria

1. A first-time user can send an Ask without seeing Work/Output/Trace chrome.
2. Opening any workspace destination never hides or resets the composer on
   desktop.
3. The conversation header contains no workspace tab strip.
4. Work/Sources/Output/Trace appear in exactly one tab strip inside the canvas.
5. The user can inspect a result and request a revision without closing it.
6. First-summon precedence is deterministic: Work → Output → Sources → Trace,
   while a manual tab choice is sticky.
7. Work presents user outcomes first and runtime detail second.
8. Canvas resize, close, focus transfer, keyboard tabs, reduced motion, and
   mobile sheet behaviour are complete.
9. Norwegian and English labels switch together; mixed-language chrome is not
   accepted.
10. No empty canvas is shown for an ordinary Ask turn.

## 12. Recommended implementation order after approval

1. Keep the composer mounted while a canvas is open.
2. Move the evidence-backed surface tab strip into the canvas header.
3. Reduce the conversation header to title, status, New chat, and one menu.
4. Recompose the chat variant of the composer around Ask/Do, grounding, and
   effort; preserve existing backend payloads.
5. Replace the flat Tasks event exhaust with Work summary + disclosed activity.
6. Restyle the empty state and transcript rhythm.
7. Finish responsive sheet and keyboard/focus behaviour.

