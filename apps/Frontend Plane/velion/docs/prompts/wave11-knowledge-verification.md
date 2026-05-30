# Wave 11 — UX-stack verification against ownership matrix + data-plane spec

Verification pass before updating `wave11-knowledge.md`. Cross-checks the proposed Chatbase → Fin → Lindy → ElevenLabs UX stack (plus Logseq-style GraphRAG viewer) against:

- `apps/master-ownership-matrix.md` — the constitutional rules
- `apps/Data Plane/docs/gap-data.md` — Data Plane target architecture
- `apps/Ingestion Plane/Quarry-v2/docs/LLM-Wiki.md` — LLM Wiki spec

Plus 22 Mobbin screens pulled live (ElevenLabs RAG + MCP + Tools, Reflect graph viewer as Logseq-style reference).

---

## 1 · Verdict

**Proposed stack is sound. Adopt as the layering rule, with one correction and three additions.**

| Layer | Source | Role | Verdict |
|---|---|---|---|
| 1. Page chrome + IA shell | **Chatbase** | Clean baseline (left sub-nav, sticky right rail, Retrain CTA) | ✅ Keep |
| 2. Content + answer-curation surface | **Intercom Fin** | Per-question evaluation (G/A/P), source attribution, polished modals | ✅ Keep |
| 3. Source picker + connector modals | **Lindy** | Easy 8-tile picker, account-connect cards, sync status | ✅ Keep |
| 4. Per-agent RAG config + MCP/Tools | **ElevenLabs** | "Configure RAG" + `Tools | MCP` segmented control + integration tool modal | ✅ Keep |
| 5. **GraphRAG visualizer** | **Logseq / Reflect "Map"** | Force-directed graph view of org's GraphRAG corpus with hover-card metadata + filters | ✅ Add (NEW — not in original plan) |
| 6. **LLM Wiki page editor** | **Logseq outliner** | Block-outline editor for durable wiki pages with backlinks panel | ✅ Add (NEW) |

---

## 2 · Ownership-matrix cross-check

The matrix is unambiguous (`apps/master-ownership-matrix.md` §0 Decision Rules):

> Quarry captures evidence. **Data Plane knows.** Model Plane reasons. **App Shell presents.**

Every UX element we lift must touch ONLY the App Shell layer. Backing implementation crosses planes — the UI never does plane-mixing itself.

| UX element we adopt | Backing plane(s) | OK? |
|---|---|---|
| Chatbase sub-nav + Retrain CTA | App Shell → Data Plane API | ✅ |
| Fin G/A/P answer ratings + citation pills | App Shell → Model Plane (gateway returns `tool_trace` already, Wave 9) | ✅ |
| Lindy 8-tile source picker | App Shell → Quarry v2 (crawl) + Data Plane (documents-api-go) + integration-core | ✅ |
| ElevenLabs `Configure RAG` per agent (chunk size, top-k, re-ranker threshold) | App Shell → Data Plane retrieval-engine-rs params | ✅ — gap-data.md §6.3 explicitly contracts these knobs |
| ElevenLabs Tools/MCP segmented control + per-tool config | App Shell → model-gateway tool registry + capability-core | ✅ — already in place from Wave 5/7 |
| **GraphRAG viewer (Logseq-style)** | App Shell → Data Plane `graph-index-rs` (gap-data.md §5.2) → returns nodes+edges JSON | ✅ — gap-data.md §6.1 lists "graph viewer" explicitly as App Shell surface |
| **LLM Wiki page editor (Logseq outliner)** | App Shell → Data Plane wiki service (gap-data.md §5.3, "LLM Wiki durable pages/versioning/source log" — Data Plane owner) | ✅ — gap-data.md §5.3 + LLM-Wiki.md confirm Data Plane owns persistence, App Shell owns the page UX |

**No plane-mixing required.** Everything is presentation; backing data flows through existing canonical contracts.

---

## 3 · Data Plane parity check (gap-data.md confirmations)

The current /knowledge wave needs the following Data Plane endpoints. Per gap-data.md:

| Need | Endpoint | gap-data.md ref | Status |
|---|---|---|---|
| List/CRUD documents | `documents-api-go` REST | §3 + §4 + Phase D3 | ✅ Exists (just missing X-Org-ID header — Phase 1 fix) |
| Configure RAG per agent | `retrieval-engine-rs` params on `/v1/retrieve` | §6.3 Agentic RAG contract | ⚠️ Endpoint exists; needs `agentId` scope + persisted-config wrapper |
| GraphRAG query | `graph-index-rs` / retrieval combined call | §5.2 + Phase D5 | ⚠️ Planned not built — Wave 11 deferred to read-only mock until D5 lands |
| LLM Wiki list/get/edit | wiki service (Data Plane) | §5.3 + LLM-Wiki.md | ⚠️ Planned not built — gate behind a feature flag |
| Per-doc rating feedback to re-ranker | extension to retrieval-engine-rs | §6.3 | New work for Phase 5 |
| AI routing classifier (rag/graphrag/wiki/finetune/prompt/skip) | model-gateway `/v1/invoke` w/ structured output | Model Plane (matrix §0) | ✅ Already available |

**Hard truth:** GraphRAG viewer and LLM Wiki editor depend on Data Plane services that aren't shipped yet. We CAN ship the App Shell UX against typed mocks now, with a feature flag, and switch to real data when Data Plane D4/D5 ship. Or we ship Wave 11.A (everything except graph + wiki) first.

---

## 4 · ElevenLabs verified specifics (from screens this turn)

`Tools | MCP` segmented control (screen `971cc033…56e5`):
- `Tools` tab: list of attached tools + `Add tool` button → modal with **3 categories**: webhook tool · client tool · integration tool
- `MCP` tab: external MCP server connections (separate from in-tree tools)
- Right rail: **System tools** with toggle switches (End conversation · Detect language · Skip turn · Transfer to agent · Transfer to number · Play keypad touch tone · Voicemail detection)

`Select Integration Tool` modal (screen `507df2c4…e85d`):
- Picks an **Integration Connection** (e.g. "Zendesk – API Token") from dropdown
- Then a checklist of available tool actions (Create Ticket, List Tickets, Show Ticket…)
- `Select All / Deselect All` shortcut
- CTA shows count: `Create 3 Tools`

Per-tool config drawer (screen `7ad55a24…58ef`):
- Left rail: Sharing · Tool ID · Integration Connection · Stats (calls, avg latency) · Dependent agents
- Main body: Conditions (prompt) · Response timeout · Disable interruptions · Pre-tool speech · Response filtering · Parameters · Dynamic Variable Assignments · Response Mocks · Edit as JSON

**Take all of this verbatim** for our Tools surface (Wave 11 phase 6.5, new sub-task).

---

## 5 · Logseq-style GraphRAG viewer (verified via Reflect `0e1be504…155e5`)

Reflect's "Map" view is the cleanest Logseq-style we found:
- Left sidebar: Daily notes / All notes / **Map** (the graph) + pinned notes list
- Force-directed graph: colored nodes (purple=note, blue=date, red=person, green=concept), thin edges
- **Hover/click** → floating metadata card with `Title · Company · Type · Email · Phone · Location` (clickable cross-links)
- **Filters** popover: Access (Published/Private), Type (Daily/Regular), Content (Show unlinked/Show blank)
- Both connected clusters AND orphan nodes visible — important for finding RAG dead-ends

**Adopt this exact pattern for our GraphRAG viewer** under `/knowledge/graph`:
- Sidebar shows entity-type filters (Person · Org · Product · Concept · Document) + selected-doc filter
- Graph shows entities + edges from `graph-index-rs`
- Click an entity → drawer with: backlinks, source docs, "agents that have queried this entity", "regenerate node summary" button (Model Plane GraphRAG query)
- Orphan-node panel surfaces poorly-connected nodes (signal for ingest gaps)

---

## 6 · LLM Wiki editor (Logseq outliner, per LLM-Wiki.md)

LLM-Wiki.md confirms: **Logseq format, not Obsidian.** Pages stored as block-outline markdown with frontmatter + source-log + version trail.

UX adoption for `/knowledge/wiki/[pageId]`:
- **Left**: tree of wiki pages (collapsible folders)
- **Center**: block-outline editor (bullet hierarchy, drag-to-reorder, indent/outdent shortcuts) — same UX as Logseq's main pane
- **Right**: 3-tab side panel
  - `Backlinks` — every page/doc that references this one
  - `Source log` — the Quarry artifacts that produced each block
  - `History` — version timeline with diff view
- **Top bar**: `[[link]]` autocomplete · `#tag` autocomplete · `Regenerate from sources` button (Model Plane refresh)

No npm-Logseq dep needed; we write a thin block-outline component on top of an existing rich-text base. The persistence shape comes from the Data Plane wiki service (already specced).

---

## 7 · Recommended layering rule (final)

When designing any new screen in /knowledge or /agents, ask in order:

1. **What IA shell?** → Chatbase (left sub-nav + sticky right rail).
2. **What content/curation chrome?** → Intercom Fin (banners, G/A/P chips, citation pills, eval drawer).
3. **What "add data" flow?** → Lindy (tile picker + connector cards).
4. **What per-agent config?** → ElevenLabs (collapsible-sections page · Configure RAG · Tools | MCP segmented control).
5. **What graph/wiki surface?** → Logseq via Reflect (Map view + outliner).

If a pattern can't be found in this stack, default to **Chatbase** for IA and **Fin** for content interaction (lowest-friction defaults).

---

## 8 · Scope recommendation

Given Data Plane services D4 (graph) and D5 (hybrid + GraphRAG) are not built:

| Wave | Contents | Days |
|---|---|---|
| **11.A — Ship-ready** | Phase 1-4 (`/knowledge` unblock + Lindy/Chatbase IA + crawl preview + onboarding auto-ingest) + Phase 6.5 (ElevenLabs Tools/MCP segmented surface) | **~4d** |
| **11.B — Curation + AI routing** | Phase 5-8 (doc editing + Fin ratings + bindings + AI router + fine-tune) | **~5d** |
| **11.C — Graph + Wiki (gated)** | Logseq-style GraphRAG viewer + LLM Wiki outliner against typed mocks; flips to real data when Data Plane D4/D5 ship | **~3d** |

Total: **~12d** (was 9d before adding the graph + wiki surfaces).

The graph + wiki UX is real value and the matrix supports it cleanly. The honest constraint is that the data behind them lands in a later Data Plane wave; we can ship the UX against mocks and flip the switch later without UI rework.

---

## 9 · Open question (decision needed before plan update)

Pick the layering of 11.C:

- **A. Ship 11.C alongside 11.B against typed Data Plane mocks** — UX visible immediately, real data in Q3.
- **B. Defer 11.C until Data Plane D4/D5 lands** — no mocks, but no graph/wiki UX in the first cut.
- **C. Build 11.C now but feature-flag OFF in production** — full UX exists, demo-ready, hidden from operators until backend is real.

Recommended: **C**. Keeps momentum, no UX rework cost, no operator-facing risk.

Awaiting your reply (`A` / `B` / `C` plus any modifications) before I revise `wave11-knowledge.md`.
