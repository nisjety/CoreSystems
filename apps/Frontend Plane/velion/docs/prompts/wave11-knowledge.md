# Wave 11 — Knowledge base overhaul + AI-routed training pipeline

Self-contained implementation brief for unifying the `/knowledge` page with the Data Plane retrieval stack and the Ingestion Plane crawler, plus adding a per-agent fine-tune surface backed by an AI router that decides which slice of org knowledge goes to RAG vs GraphRAG vs LLM Wiki vs fine-tuning vs prompt-engineering.

> **Scope-setting**: This is a single, focused wave with one clear outcome — "/knowledge becomes the single source of truth for org data feeding any agent, with one-click ingestion paths and AI-routed training." Anything beyond that ships in Wave 12+.

---

## 1 · Requirements restatement

### 1.1 Functional
1. `/knowledge` must load and render without error (currently throws `X-Org-ID header required`).
2. Knowledge base must be **org-scoped** (not user-scoped) so every agent in the org sees the same corpus by default; per-agent overrides come later.
3. Five ingestion modes (parity with ElevenLabs + Chatbase + Lindy + Fin):
   - **Web crawl** (Quarry) — paste URL → discover → ingest pages.
   - **File upload** — PDF, DOCX, MD, TXT, CSV (existing documents-service).
   - **Integration sync** — Slack, Notion, Drive, Confluence (existing integration-core + Nango).
   - **Q&A pairs** — operator-authored question→answer entries (Chatbase-style "QA").
   - **Plain text / paste** — quick "remember this" snippet without a file.
4. Onboarding `website` step kicks Quarry against the org URL automatically; the resulting documents land in `/knowledge` with status `synced` so the user sees real org data on first login (Chatbase-style first-run).
5. Operator can **edit, re-process, deprecate, or pin** any document (Fin-style "review answers" surface).
6. Per-agent **knowledge binding**: each agent picks a subset of org documents (default = all). UI is a chip-picker on the agent workspace.
7. New **"Train" tab** on each agent with:
   - Fine-tune checkbox (master switch).
   - "Let AI choose what to fine-tune" toggle (recommended default ON).
   - Manual matrix view: per-document, dropdown of `{auto, rag, graphrag, wiki, finetune, prompt, skip}`.
   - Re-train button → kicks Wave-7 fine-tune flow on the auto-classified `finetune` subset.

### 1.2 Non-functional
- All ingest paths are async + status-polled; SSE for crawl, NATS-mirror-via-Convex for everything else (subscriber pattern from §16).
- The Knowledge page is reactive: when a Quarry job emits `mp.v1.crawl.page.indexed`, the doc list updates without a hard refresh.
- AI router is a small reasoning step (gateway `/v1/invoke` with a structured-output prompt), not a separate trained classifier — keeps the surface dependency-light.
- "Edit" on a document writes BACK to documents-service AND triggers re-embedding; we don't ship a UI that visibly lies.
- All routes degrade gracefully: if documents-service is down, Knowledge renders empty-state + warning chip (pattern landed in Wave 10 follow-up).
- Org-scoped: every server call must derive `orgId` from `resolveChatActor()` and pass it as `X-Org-ID` to downstream Data Plane services.

### 1.3 Hard non-goals (kill if scope-creeping)
- Multi-org knowledge sharing (federation).
- A custom embedding model — we keep using whatever embedding-worker is configured for.
- A drag-and-drop "knowledge graph editor". GraphRAG continues to auto-build from documents.
- Real-time collaborative editing of documents (Notion-style multi-cursor).

---

## 2 · Current-state inventory

### 2.1 What exists today

| Layer | Component | Path | Notes |
|---|---|---|---|
| Frontend Plane | `/knowledge` page | `verevon/src/app/(dashboard)/knowledge/page.tsx` | SSR; calls `getKnowledgeIntegrations`, `getKnowledgeSources`, `getKnowledgeDocuments` |
| Frontend Plane | `KnowledgePageClient` | `verevon/src/app/(dashboard)/knowledge/KnowledgePageClient.tsx` | Tabs UI; currently throws on initial load |
| Frontend Plane | `knowledge-data.ts` | `verevon/src/app/api/knowledge/_lib/knowledge-data.ts` | Server-side fetchers; calls `documents-service` (port 8001) + `integration-engine-go-api` (port 3126) |
| Frontend Plane | Ingestion API | `verevon/src/app/api/ingestion/crawl/{route,[jobId]/status,[jobId]/stream}.ts` | Quarry trigger + SSE |
| Frontend Plane | Onboarding website step | `verevon/src/app/(onboarding)/onboarding/website/page.tsx` | Currently captures URL; doesn't trigger ingest |
| Frontend Plane | Agent finetune hook | `verevon/src/components/agents/hooks/useAgentFinetune.ts` | Wave 7 — supports JSONL upload + Azure fine-tune jobs |
| Data Plane | `documents` service | `Data Plane/services/documents` | gRPC + HTTP; stores raw docs + metadata |
| Data Plane | `knowledge-index` worker | `Data Plane/services/knowledge-index/worker` | Background re-indexing on doc changes |
| Data Plane | `retrieval` (RAG) | `Data Plane/services/retrieval` | gRPC; serves `/retrieval.Retrieve` to model-gateway |
| Data Plane | `embedding-worker` | `Data Plane/services/embedding-worker` | NATS-driven; turns doc text → vectors |
| Ingestion Plane | `Quarry` | `Ingestion Plane/Quarry` | Go + Temporal crawler (cmd/bin) |
| Ingestion Plane | `integration-core` | `Ingestion Plane/integration-core` | TS + Nango bridge; provider catalog + connections |
| Application Plane | Convex `agents` table | `convex-core/convex/agents.ts` | Already has `tools`, `model`, `systemPrompt`; no `knowledgeBindings` yet |
| Model Plane | `model-gateway` | `Model Plane/rust/services/model-gateway` | `/v1/invoke` + tool registry; has `kb_search`, `graph_search`, `wiki_lookup` |

### 2.2 What's broken today

| Symptom | Root cause | Fix in this wave |
|---|---|---|
| `/knowledge` 500s with "X-Org-ID header required" | `fetchDocumentsForActor` (knowledge-data.ts:304) calls documents-service but doesn't forward `X-Org-ID` | Phase 1 |
| Quarry crawl jobs orphan from documents-service | Crawl pages don't get persisted as `documents` rows; only available via the temporary crawl session | Phase 4 |
| Agent has no concept of "use this subset of org knowledge" | `agents` table has no `knowledgeBindings` field | Phase 6 |
| "Fine-tune" is a separate page that doesn't know what to feed itself | No data-routing classifier; user must hand-craft JSONL | Phase 7 |

---

## 3 · Competitor pattern distillation — Mobbin-verified

Pulled live from Mobbin MCP (24 screens, May 17, 2026). All patterns below are observed in production UIs, not paraphrased from memory.

### 3.1 IA shapes seen across the four products

| Product | Knowledge hub IA |
|---|---|
| **Chatbase** | Left sub-nav under "Data sources": Files / Text snippets / Website / Q&A / Notion / Tickets / Suggestions — each ingestion mode is a dedicated page (NOT a single dropdown). Right rail: total size · `Retrain agent` button · "Retraining required" warning. |
| **ElevenLabs** | Top-level "Knowledge Base" page + per-agent "Agent Knowledge Base" tab with `Configure RAG` and `Add document` buttons. The `Add document` modal uses a segmented control: **File / URL / Text** (3 modes, not 5). Per-doc drawer has Content / Agents tabs. |
| **Intercom Fin** | Left nav: Train (Content · Guidance · Tasks · Suggestions) → Test → Deploy. `Add content` is an expandable list of 5 entries: Public article · Create new article · Create snippet · Upload a document · Add a Custom Answer. Upload modal has a fixed format-spec sidecar (size · type · count limits). |
| **Lindy** | Single `Select knowledge base` modal with **8 source tiles in a grid**: Files · Text · Website · Google Drive · OneDrive · Dropbox · Notion · Freshdesk. Plus a separate "Add action" library that contains `Knowledge base` as an AI action inside agent workflows. |

### 3.2 Concrete UX patterns to lift (with screen IDs)

| Pattern | Source | Screen ID | Verevon adoption |
|---|---|---|---|
| **Left sub-nav of ingestion modes** (not a modal) | Chatbase | `3109ea55…1ddb` | Adopt — each `Add data` mode becomes its own `/knowledge/<type>` page; faster nav + deep-link friendly |
| **8-tile source picker modal** for first-time add | Lindy | `04b7331b…2a981` | Adopt for the entry-point "Add knowledge" button; tiles fall through to the sub-nav pages |
| **Segmented File / URL / Text modal** | ElevenLabs | `af414e04…5910` | Adopt for in-context "Add document" inside an agent's Knowledge tab |
| **Format-spec sidecar in upload modal** ("pdf/docx ≤ 100 MB, up to 10 files, no images") | Intercom Fin | `cc67dd1f…dc7c` | Adopt — copy this exact pattern in our UploadFilesModal |
| **Sticky right rail with totals + Retrain CTA** | Chatbase | `3109ea55…1ddb` | Adopt for `/knowledge` and per-agent Knowledge tab |
| **`Retraining is required for changes to apply`** chip | Chatbase | `3109ea55…1ddb` | Convex `org.knowledgeDirty` flag; chip clears on retrain completion |
| **`Your content is currently being ingested. You will be notified once…`** top banner | Intercom Fin | `ab3fc815…dced` | Wire to crawl + embedding progress |
| **Per-document drawer with Content / Agents tabs** | ElevenLabs | `c107464e…8017` | Adopt — Agents tab lists every agent bound to this doc with quick-unbind |
| **`Extracted content` text preview in drawer** | ElevenLabs | `c107464e…8017` | Adopt — shows the chunked text the LLM actually receives |
| **Per-question evaluation: Unrated / Good / Acceptable / Poor** with keyboard shortcuts (G/A/P) | Intercom Fin | `a4875514…94ca` | Adopt for the Q&A pane + Test surface (replaces "trust slider") |
| **"This answer uses: Content (N) · Guidance (M)"** source attribution panel | Intercom Fin | `ab3fc815…dced` | Adopt — surface on playground replies, links back to the cited docs |
| **`Configure RAG` button** per agent | ElevenLabs | `94abd75b…b2e4` | Adopt — opens chunking / k / re-ranker settings; defaults are sane |
| **Bulk-select with floating action bar** ("2 selected · Delete · Restore") | Chatbase | `591d4424…b936` | Adopt for document list (multi-deprecate, multi-pin) |
| **`New` pill badge on freshly-ingested rows** | Chatbase | `3109ea55…1ddb` | Adopt — Convex `freshUntil: now + 7d` |
| **Connect-accounts onboarding card with right-pane illustration** | Lindy | `a136897f…a04a` | Adopt for the Phase-4 onboarding auto-ingest screen |
| **Text-paste modal with name + content + items list with sync status** | Lindy | `dc4b9aad…de69` | Adopt for PasteTextModal — already in scope |

### 3.3 Patterns from the original plan that need correction

| Original plan said | Mobbin evidence | Corrected plan |
|---|---|---|
| Single "Add data" dropdown with 5 chips | Chatbase splits modes into sub-nav pages; Lindy uses an 8-tile grid; ElevenLabs uses a 3-tab segmented modal | **Hybrid**: top-level `Add knowledge` button → Lindy-style 8-tile picker → routes to per-mode page (Chatbase pattern). In-agent quick-add uses ElevenLabs 3-tab modal. |
| Per-document **trust slider** (0–1) | Not observed in any of the four — they all use per-answer rating chips (G/A/P) instead | Drop the trust slider. Use rating signals on answers to feedback-loop the re-ranker (Phase 5). |
| Q&A as `documents.type='qa'` row | Chatbase, Lindy, and ElevenLabs all separate Q&A into its own ingestion mode; Fin treats it as `Custom Answers` (separate entity with versioning) | Q&A is a **first-class entity**, not a doc subtype. New table `knowledgeQnA` with `question`, `answer`, `status`, `rating`, `lastEvaluatedAt`. |
| "Outcome chip" per doc | Fin shows "This answer uses: Content(2)" on the answer side, not per-doc | Reverse direction: on each agent answer in playground/analytics, show citation pills that link back to docs. On the doc drawer, show "used by N answers · M agents · last cited 3h ago." |

---

## 4 · Implementation phases

### Phase 1 — P0 unblock: fix `/knowledge` load (HALF DAY)

**Goal**: Get the page rendering with real org data, no errors, no fixtures.

- [ ] `knowledge-data.ts::fetchDocumentsForActor`: forward `X-Org-ID: ${actor.orgId}` AND `X-Internal-Api-Key: ${INTERNAL_API_KEY}` headers. Audit ALL `fetchJson` callers and centralize header injection.
- [ ] Confirm documents-service is actually running locally (currently `:8001` is down per Wave-10 diagnosis). If down, the route must degrade gracefully to empty `documents: []` with a warning header (mirror pattern from `/api/knowledge/integrations/route.ts`).
- [ ] Same hardening on `getKnowledgeSources` and `getKnowledgeIntegrations`.
- [ ] Smoke: load `/knowledge` against a stopped documents-service → empty state + dev-warning toast. Against a running service → real documents.

**Files**
- `verevon/src/app/api/knowledge/_lib/knowledge-data.ts` — header forwarding + ECONNREFUSED catch
- `verevon/src/app/(dashboard)/knowledge/page.tsx` — wrap each `Promise.all` member with `.catch(() => fallback)` so one service outage doesn't 500 the page
- `verevon/src/app/(dashboard)/knowledge/KnowledgePageClient.tsx` — surface per-section warnings

**Exit criteria**: `/knowledge` renders for a user whose org has zero documents AND for a user whose org has 100+ documents, with documents-service running or stopped.

---

### Phase 2 — Knowledge page IA + Lindy/Chatbase hybrid picker (1.5 DAYS)

**Goal**: Rebuild `/knowledge` as a 3-pane hub (Sources · Documents · Q&A) with the unified "Add data" dropdown.

#### 2.1 Information architecture (Mobbin-corrected)

```
/knowledge                       ← top-level page; left sub-nav (Chatbase pattern)
├─ /knowledge/files              ← uploads — drag-drop + format spec sidecar (Fin)
├─ /knowledge/text               ← pasted text snippets (Lindy text modal)
├─ /knowledge/website            ← URL crawls (Quarry-backed, discover→commit preview)
├─ /knowledge/qa                 ← operator-curated Q&A pairs (Fin Custom Answers)
├─ /knowledge/integrations       ← Slack/Notion/Drive/Confluence (existing connectors)
└─ /knowledge/suggestions        ← AI-proposed gaps in coverage (post-Phase-7)

PRIMARY ENTRY: [+ Add knowledge] button → Lindy 8-tile picker modal:
  Files · Text · Website · Q&A · Google Drive · OneDrive · Notion · Slack
  (Tiles deep-link to the sub-nav pages above; integrations open OAuth.)

PER-PAGE CHROME:
  Left:   the sub-nav (selected item highlighted)
  Center: list view (search · sort · bulk-select · floating action bar)
  Right:  sticky rail (total size · doc count · `Retrain agents` CTA · dirty chip)

PER-DOCUMENT DRAWER (ElevenLabs):
  Tabs: Content / Agents
  Content: extracted text preview, edit, deprecate, ingestion status
  Agents : list of agents bound to this doc, with unbind action
```

#### 2.2 Source picker — Lindy 8-tile grid (verified screen `04b7331b…2a981`)

| Tile | Routes to | Backing |
|---|---|---|
| 🗎 Files | `/knowledge/files` | documents-service upload |
| ✍️ Text | `/knowledge/text` | documents-service `type=text` |
| 🌐 Website | `/knowledge/website` | Quarry crawl (discover→commit) |
| 💬 Q&A | `/knowledge/qa` | new `knowledgeQnA` table |
| ☁️ Google Drive | OAuth → `/knowledge/integrations` | integration-core (Nango) |
| ☁️ OneDrive | OAuth → `/knowledge/integrations` | integration-core |
| 📓 Notion | OAuth → `/knowledge/integrations` | integration-core |
| 💼 Slack | OAuth → `/knowledge/integrations` | integration-core |

In-agent quick-add (agent Knowledge tab) uses the smaller ElevenLabs 3-tab segmented modal (File / URL / Text) — keeps the agent context focused.

#### 2.3 Sticky right rail (Chatbase verified `3109ea55…1ddb`)

```
┌─────────────────────────────┐
│ Data sources                │
│ ┌──────────────┬──────────┐ │
│ │ 47 Sources   │  12.4 MB │ │
│ └──────────────┴──────────┘ │
│                             │
│ Total size       12.4 / 50 MB │
│ [████░░░░░░░░░]              │
│                             │
│ ┌─────────────────────────┐ │
│ │  Retrain agents (3)     │ │
│ └─────────────────────────┘ │
│                             │
│ ⚠ Retraining required        │
│   for changes to apply       │
└─────────────────────────────┘
```

#### 2.4 Files to add/change

```
verevon/src/app/(dashboard)/knowledge/layout.tsx                            (new — left sub-nav shell + right rail)
verevon/src/app/(dashboard)/knowledge/page.tsx                              (rewrite — index/overview)
verevon/src/app/(dashboard)/knowledge/files/page.tsx                        (new)
verevon/src/app/(dashboard)/knowledge/text/page.tsx                         (new)
verevon/src/app/(dashboard)/knowledge/website/page.tsx                      (new)
verevon/src/app/(dashboard)/knowledge/qa/page.tsx                           (new)
verevon/src/app/(dashboard)/knowledge/integrations/page.tsx                 (rewrite from existing — use new shell)
verevon/src/components/knowledge/AddKnowledgeModal.tsx                      (new — Lindy 8-tile grid)
verevon/src/components/knowledge/KnowledgeSidebar.tsx                       (new — left sub-nav, Chatbase)
verevon/src/components/knowledge/KnowledgeStatusRail.tsx                    (new — sticky right rail w/ Retrain CTA)
verevon/src/components/knowledge/DocumentList.tsx                           (new — bulk-select + floating action bar)
verevon/src/components/knowledge/DocumentDrawer.tsx                         (new — Content/Agents tabs, edit, deprecate, extracted-content preview)
verevon/src/components/knowledge/QnAEvaluator.tsx                           (new — Fin G/A/P rating chips w/ keyboard shortcuts)
verevon/src/components/knowledge/modals/CrawlSourceModal.tsx                (new — discover→preview→commit)
verevon/src/components/knowledge/modals/UploadFilesModal.tsx                (new — Fin spec sidecar)
verevon/src/components/knowledge/modals/PasteTextModal.tsx                  (new — Lindy text pattern)
verevon/src/components/knowledge/modals/QnAModal.tsx                        (new — operator-authored pair)
verevon/src/components/knowledge/modals/AddDocumentSegmented.tsx            (new — ElevenLabs 3-tab File/URL/Text for in-agent context)
verevon/src/components/knowledge/hooks/useKnowledgeStats.ts                 (new — totals + dirty flag for right rail)
verevon/src/components/knowledge/hooks/useKnowledgeDocuments.ts             (new)
verevon/src/components/knowledge/hooks/useKnowledgeQnA.ts                   (new)
```

**Exit criteria**: All five "Add data" modals work end-to-end against running services; the page reactively reflects status changes; documents-service-down still renders the page.

---

### Phase 3 — Crawl preview UX + Quarry → documents-service pipeline (1 DAY)

**Goal**: Make Chatbase-style discovery work. User pastes URL → Quarry discovers → checklist of found pages → user confirms → those pages persist as `documents` rows.

- [ ] Quarry's `discover` (no-fetch) mode already returns the page list; wire `/api/ingestion/crawl?phase=discover` to return it.
- [ ] New `/api/ingestion/crawl/[jobId]/commit` POST that takes the URL checklist subset and triggers actual fetching + indexing.
- [ ] On `mp.v1.crawl.page.indexed` NATS event → mirror to Convex `knowledgeDocuments` table → page list updates reactively.
- [ ] Add `sourceId` and `crawlJobId` columns on Convex `knowledgeDocuments` so the Sources pane can group by origin.

**Exit criteria**: From `/knowledge` → Add source → Crawl website → enter URL → see "found 42 pages" preview → uncheck unwanted → commit → docs appear in Documents pane within 30s.

---

### Phase 4 — Onboarding auto-ingest (HALF DAY)

**Goal**: Chatbase parity for first-run experience. When user finishes onboarding `website` step, fire Quarry against the URL in the background; results land in /knowledge automatically.

- [ ] On the onboarding "website" form submit, POST to `/api/ingestion/crawl?phase=discover&autoCommit=true&maxPages=50`.
- [ ] Surface a sticky banner on first /knowledge load: "We've started indexing your site — 42 pages found. Come back in a minute or two."
- [ ] If the org already has documents (user revisiting onboarding), skip the auto-ingest to avoid duplicates.

**Exit criteria**: New user finishes onboarding → 60s later /knowledge shows real org content from their website.

---

### Phase 5 — Document editing + answer rating feedback loop (1 DAY)

**Goal**: Fin-style "review and curate" surface — operators rate answers (G/A/P) and the per-answer ratings feed back into the re-ranker. No trust slider (Mobbin showed no competitor uses one).

- [ ] `DocumentDrawer` with **Content / Agents** tabs (ElevenLabs). Content: extracted-text preview, inline edit (debounced auto-save), deprecate toggle, ingestion status. Agents: agents bound to this doc + quick-unbind.
- [ ] On edit → `PATCH /api/knowledge/documents/[id]` → documents-service updates row + emits `doc.changed` → embedding-worker re-embeds.
- [ ] **Per-answer rating UI** (Intercom Fin): in playground + Test surface, every assistant reply gets `Good (G) / Acceptable (A) / Poor (P)` chips with keyboard shortcuts. Ratings persist on `agentRuns.rating`.
- [ ] **Citation pills under each reply**: `This answer uses: Content (2) · Q&A (1)` linking back to the source rows in /knowledge.
- [ ] Aggregate per-doc stats in the drawer: "Used by N answers · M agents · last cited 3h ago · 87% Good" — sourced from `agentRuns.citations` + `agentRuns.rating`.
- [ ] Re-ranker boost in `retrieval/app`: docs with `> 0.8` good-rating ratio get a small positive boost; `< 0.3` get a negative boost. Auto-tuned, no UI surface required.

**Exit criteria**: Rate 5 answers Poor → next retrieval visibly down-weights the cited docs; edit a doc → next answer cites the new content; drawer shows correct usage + rating stats.

---

### Phase 6 — Per-agent knowledge bindings (1 DAY)

**Goal**: Each agent can scope which slice of org knowledge it can retrieve from.

- [ ] Convex: add `knowledgeBindings: v.optional(v.object({ scope: v.union(v.literal('all'), v.literal('selected')), documentIds: v.optional(v.array(v.string())), sourceIds: v.optional(v.array(v.string())) }))` to `agents` table.
- [ ] Agent workspace: new "Knowledge" tab with chip-picker (existing knowledge sources + documents). Default chip is "All org knowledge".
- [ ] When agent's `kb_search` tool fires, gateway scopes the retrieval call by the bound document/source ids.
- [ ] Migration: existing agents get `scope: 'all'`.

**Files**:
- `convex-core/convex/agents.ts` (+`updateKnowledgeBindings`)
- `verevon/src/components/agents/AgentWorkspaceView.tsx` (new tab)
- `verevon/src/components/agents/hooks/useAgentKnowledge.ts` (already exists; extend)
- `model-gateway/src/tool_registry.rs::kb_search` (forward `bound_document_ids` to retrieval)

**Exit criteria**: An agent bound to a 3-doc subset answers ONLY from those 3 docs; analytics confirms zero citations from out-of-scope documents.

---

### Phase 7 — AI training-data router (2 DAYS)

**Goal**: The headline feature. Given an org's knowledge corpus, an AI step decides per-document which retrieval modality it belongs in: `rag`, `graphrag`, `wiki`, `finetune`, `prompt`, or `skip`.

#### 7.1 Routing rubric (the system prompt the classifier runs on each doc)

| Modality | When chosen | Example doc shape |
|---|---|---|
| `rag` (default) | Mostly unique factual content, retrieval-by-meaning works | Help articles, policy docs, product specs |
| `graphrag` | Entity-rich, relationships matter (org charts, product hierarchies) | "Who reports to whom", "which feature is in which plan" |
| `wiki` | Stable, frequently-referenced summaries; benefits from structured lookup | Glossary, acronym dictionary, named-entity definitions |
| `finetune` | Tone/voice exemplars; format examples; recurrent task patterns | Past chat transcripts, brand-voice samples, response templates |
| `prompt` | A short rule the agent must always follow | "Never reveal pricing", "Always greet by first name" |
| `skip` | Boilerplate, low signal, duplicated, deprecated | Cookie banners, legal headers, footer text |

#### 7.2 Architecture

```
                    /knowledge
                        │
                        ▼
         POST /api/knowledge/route-classify
                        │
                        ▼
  ┌────────────────────────────────────────┐
  │  classify-data-router (server action)  │
  │  · loads org documents (paginated)     │
  │  · for each: model-gateway /v1/invoke  │
  │    with structured-output schema       │
  │  · writes back routing column          │
  └────────────────────────────────────────┘
                        │
                        ▼
       documents-service: update row
       routing: 'rag'|'graphrag'|'wiki'|'finetune'|'prompt'|'skip'
                        │
                        ▼
    embedding-worker re-routes downstream:
      rag       → vector index (current)
      graphrag  → graph builder (existing service)
      wiki      → llm-wiki summarizer (existing tool)
      finetune  → finetune-corpus bucket (Wave 7 pickup)
      prompt    → appended to agent.systemPrompt
      skip      → archived, not indexed
```

#### 7.3 Files
```
verevon/src/app/api/knowledge/route-classify/route.ts                  (new)
verevon/src/lib/knowledge/router-prompt.ts                             (new — the rubric prompt)
verevon/src/lib/knowledge/router-schema.ts                             (new — Zod schema for the structured output)
Data Plane/services/documents/app/routing.py                          (new — column + ingest hooks)
verevon/src/components/knowledge/RoutingMatrix.tsx                     (new — per-doc dropdown override UI)
```

#### 7.4 Cost/safety
- Throttle classification to N=10/sec (chunked).
- Skip docs already classified within last 30 days unless `force=true`.
- All classifier outputs go through Zod parse; non-conforming → default to `rag`.
- Operator can manually override any row from the RoutingMatrix UI; manual overrides persist a `routedBy: 'manual'` flag so re-classification doesn't clobber them.

**Exit criteria**: Click "Auto-route" on a 50-doc org → completes in <2 min → routing column populated → fine-tune bucket has the right shape of data → RAG search excludes `skip` rows → wiki tool finds glossary content.

---

### Phase 8 — Per-agent fine-tune checkbox + retrain (1 DAY)

**Goal**: Bring Wave 7 fine-tune flow under the agent workspace UI; tie it to Phase 7 routing.

- [ ] New agent-workspace section "Train" with:
  - **Fine-tune this agent** (master checkbox).
  - **Let AI choose what to fine-tune** (recommended, ON by default).
  - When OFF: matrix view of every routed-`finetune` doc with override dropdown.
  - **Start training run** button → POSTs to existing Wave 7 fine-tune endpoint with the curated JSONL built from the matrix selection.
- [ ] Training run progress surfaced via existing `useAgentFinetune` hook.
- [ ] Banner: "Knowledge changed — last trained on data from 12d ago. Retrain?"

**Exit criteria**: Operator can flip the master switch, accept the AI recommendation, click train, and see a real Azure fine-tune job spin up against the auto-classified corpus.

---

### Phase 9 — Docs + closure (HALF DAY)

- [ ] §21 in `docs/ui-ux-verevon-gap.md` mirroring §19's shape.
- [ ] Update `docs/CODEMAPS/knowledge.md` (regen via /update-codemaps).
- [ ] Rename `docs/prompts/wave11-knowledge.md` → `wave11-knowledge.closed.md`.

---

## 5 · Dependencies (services that MUST be reachable)

| Service | Port | Used in phase | Fallback if down |
|---|---|---|---|
| documents-service | 8001 | All | Empty state + warning |
| integration-engine-go-api | 3126 | 2, 6 | "Integrations unavailable" chip |
| Quarry crawler | 3601 (Temporal) | 3, 4 | "Crawler offline" toast on Add-source |
| retrieval (gRPC) | 50053 | 5, 6 | RAG calls fail; degraded chat |
| embedding-worker | NATS-driven | 5, 7 | Indexing lag, not a UI block |
| model-gateway | 18080 | 7, 8 | Classifier + fine-tune fail; surface error |

The harden-down work from Wave 10 covers most failure modes — Phase 1 finishes that audit for `/knowledge`.

---

## 6 · Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Classifier hallucinates routing (e.g. routes everything to `finetune`) | HIGH | Zod-parse outputs; manual override; default-to-`rag` on malformed; cap fine-tune corpus at N docs |
| Quarry auto-ingest on onboarding hits sites that aggressively rate-limit | MEDIUM | `maxPages=50` cap; respect robots.txt; user can cancel from the banner |
| Re-embedding cost spikes on bulk edit | MEDIUM | Debounce 5s; batch via NATS; show "indexing" status chip |
| Existing `useAgentFinetune` hook assumes user-supplied JSONL; Phase 8 changes the producer side | MEDIUM | Adapter layer: keep manual upload working; AI-routed JSONL is a parallel input path |
| Convex `knowledgeDocuments` table doesn't exist yet | LOW | Schema migration in Phase 3 |
| Documents-service `X-Org-ID` is a hard-fail today, suggesting other Data Plane services have the same require | LOW | Phase 1 audit covers all `fetchJson` callers |
| Mobbin MCP unavailable so screen specifics are paraphrased | LOW | Implementation follows the four product references' documented patterns; Mobbin verification can happen post-Phase-2 |

---

## 7 · Complexity estimate

| Phase | Days |
|---|---|
| 1 — `/knowledge` unblock | 0.5 |
| 2 — IA + Add-data dropdown | 1.5 |
| 3 — Crawl preview + Quarry→documents | 1.0 |
| 4 — Onboarding auto-ingest | 0.5 |
| 5 — Edit + trust + outcomes | 1.0 |
| 6 — Per-agent bindings | 1.0 |
| 7 — AI router | 2.0 |
| 8 — Fine-tune integration | 1.0 |
| 9 — Docs + closure | 0.5 |
| **Total** | **9 focused days** |

Splittable: Phases 1-4 ship as 11.A (basic knowledge base + auto-ingest), 5-9 ship as 11.B (curation + AI routing + training).

---

## 8 · Open questions for confirmation

1. **Scope cut**: ship as 11.A + 11.B (4d + 5d) or one wave (9d)?
2. **AI router default**: should "AI decides routing" be checked-on by default, or ask-on-first-run?
3. **Onboarding auto-ingest**: implicit (just crawl) or explicit ("we'll index your site — proceed?")?
4. **Trust slider granularity**: 0-1 continuous or 3-step (low/medium/high)?
5. **Fine-tune budget guard**: hard cap (e.g. $20/training run) before kicking Azure? If yes, where does the limit live — env var or per-org setting?
6. ~~Mobbin MCP wait~~ — DONE (May 17, 2026). All §3 patterns are Mobbin-verified with screen IDs.
7. **Configure RAG button** (ElevenLabs `94abd75b…b2e4`): adopt now (Phase 6 sub-task) or punt to a later wave? Surface lets operators tune chunk size, top-k, and re-ranker threshold per agent — small UI, real value.

---

## 9 · Confirmation required

This plan is **non-executing**. No code is touched until you reply with one of:

- `proceed` → Phase 1 first, sequentially.
- `proceed 11.A` → Phases 1-4 only, defer 5-9.
- `proceed parallel` → Phases 1-3 in parallel sub-agents, rest sequential.
- `modify: <changes>` → revise the plan.
- `skip phase N` → drop a phase.

