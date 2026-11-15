# T3 category 3: Research & web (comparison summary)

71 reference records from 11 sources, grouped into 27 clusters. All 71 are assigned and none are excluded. 34 Verevon baseline records were used, plus spot checks in code on 2026-10-13. Cluster detail: `03-research-web.clusters.json`. Provenance tags: SRC = read in source, DOC = documentation, INF = inference.

## Shared baseline (table-stakes and common)

| Capability | Sources | Verevon |
|---|---|---|
| Web search with grounded, cited answers (C-001) | 11/11 (T3 Code only renders what its agents do) | **shipped**, and ahead of the references |
| Connected apps in chat, with read/write and multi-account (C-016) | 6/11 | partial: Do mode only, mail returns headers or ids, no writes |
| Deep research run with a cited report (C-006) | 5/11 | partial: the core is shipped, but it makes one pass over the web only |
| In-context connector suggestions and a directory (C-017) | 4/11 | absent |

## Best of breed

| Capability | Best source | Why |
|---|---|---|
| Web search UX | ChatGPT (DOC) | Query rewriting, several searches per turn, hover previews, a Sources list, and a published statement of what goes to search partners |
| Batched web tool | Codex (SRC); DeepSeek for the minimal version | Typed operations (open, click, find by reference id; PDF; finance, weather). DeepSeek runs up to 4 queries at once, merges them round-robin, removes duplicates, caps at 8 and adds an instruction to cite |
| Safe fetch | DeepSeek (SRC) | Resolves, validates and pins the address (NAT64-aware), refuses redirects to another origin, labels the body as untrusted. Antigravity adds an outline plus chunked reading of long pages |
| Web governance | Claude (SRC) | Org switch, consent per domain, admin choice of search provider, egress allow-lists. Codex adds disabled, cached or live search modes |
| Deep research method | Perplexity (DOC scale); Codex skill (DOC method) | Two passes, a ledger linking each claim to its source, a test that stops when new sources add little, and QA on the rendered report |
| Research steering | ChatGPT (DOC); Perplexity (DOC) | Clarifying questions, a plan you can edit, interrupt and refocus, a list of allowed sites, key findings while the run is going |
| Wide research | Manus (SRC + DOC) | Cost estimate and start card, a grid of subtasks, replay and retry per subtask, a spending check partway through, sub-agents kept apart |
| Agent browser | Manus (DOC); Claude for the consent model (SRC) | Live view and human take-over; separate "always allow" for reads and for actions on each host |
| Connector suggestions | Claude (SRC) | Opt-in suggestion cards, a connect step that resumes after login, one directory for connectors, skills and plugins |
| Connector permissions | Claude (SRC); Manus for the cards (SRC) | Four policy levels checked against MCP read/write annotations; editable cards before writing to mail or calendar |

## Differentiators worth noting
Wide research (Manus) and plan steering (ChatGPT, Perplexity) are a real edge. So are batched web tools (Codex, DeepSeek), SSRF-safe fetch with chunked reading (DeepSeek, Antigravity) and governance of web access (Claude, Cursor, Codex). Features found in one source only: Model Council (Perplexity, DOC), search as code inside the sandbox (Perplexity, DOC, the vendor's own numbers), app event subscriptions (ChatGPT MCP Events, DOC), an open search-provider layer (DeepSeek, SRC) and knowledge items (Antigravity, SRC).

## Verevon's biggest gaps (priority tier)
1. **Research plan steering (C-007, significance 4).** The plan is shown as a checklist before any search runs. But there are no clarifying questions, the user cannot edit or approve the plan, and cannot add questions, limit sites or see key findings while it runs. Fix in the Model Plane: split deep research into a plan phase and a run phase, and reuse the existing `plan_proposed` and `queued_input` events.
2. **Wide research (C-008, significance 4).** The engine exists (`execution-core/subagent/research_fan_out.rs`). There is no way for the user to start it, no cost estimate or spending check, no retry per item, and it cannot be replayed after a reload (cards come back empty). It runs in Do mode only and at small scale.
3. **Deep research quality (C-006, significance 4).** `deep_research.rs` makes one pass over the web only: no second, targeted round and no stop test. INF: no knowledge or connector sources (grep). It lives inside the request, so it ends when the stream ends. Adding Data Plane knowledge next to the web in one cited report is where Verevon's moat would show.
4. **Connected apps in chat (C-016, significance 4).** Provider reads work in Do mode only. Gmail returns ids only, mail listings are headers only, writes are not built, and there is no @-mention of a connector.

Recommended next: chunked reading of long pages (chat `fetch_url` cuts pages at 4,000 characters), an org domain policy and cached mode for web access, a batched multi-query `web_search`, a working agent browser for chat (grant minting fails closed today), connector suggestions, write-confirmation cards, and research deliverables (a document pane that opens at the cited passage, plus provenance). Skip: Health/Finance spaces, shopping checkout, Learn mode.

## Verevon strengths (keep)
- **Grounding honesty is ahead of every reference.** Answers come from pages that were actually read. A relevance gate drops off-topic hits before they can be cited. Citations are tied to claim segments by byte offset and stored. Deep research numbers only the pages it read, lists unread sources as not citable, and always ends with a "Kunne ikke verifiseres" (could not be verified) section. A retrieval notice says when an answer cannot be grounded.
- **Source scope is enforced on the server** (Auto, knowledge only, web only, or this conversation only), and the client gets a receipt each time.
- **The Sources panel separates sources that were read from those that were not**, and shows a grounding overview.
- **SSRF protection sits in Quarry-v2** (`dns_guard.rs`, which resolves the address, vets it and pins the connection), consistent with the rule that the Model plane proposes and Quarry executes. Fetched pages are fully screened for prompt injection before they are cut down.
- **Paid search providers are gated by tier and retention**, so outbound traffic can be audited.
- **Data Plane v2 knowledge tools** (graph, wiki, contradictions).
- **Norwegian public-data tools** (Brreg, SSB, Yr, shipping).

## Caveats
- About half the reference records are DOC only: these are public help pages and vendor claims, for example Perplexity's search-as-code reliability numbers. Confidence was lowered in clusters that rest only on DOC.
- Hermes has no records in this category. Prevalence is counted over the 11 sources present. In C-001 and C-006, sources with records in sibling clusters are counted as well; each cluster's description says which.
- Some Verevon statuses rest on grep (absence of a connector-suggestion tool, inline web images, Model Council, knowledge in deep research) and are INF. C-013 (inline images) is `unknown` and needs a check of the chat renderer.
- The working tree has uncommitted Model Plane changes, and several baseline items are DOC-proven only (chip binding, wide research at scale, connector reads). Code is ground truth and the docs may be stale.
- Some records in this category are really about memory or computer use (knowledge items, agent browser). They were kept, with lower significance where another category owns the topic.
