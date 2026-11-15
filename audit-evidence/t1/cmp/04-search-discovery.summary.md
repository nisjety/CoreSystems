# T1 / Category 4: Search & discovery (cross-source comparison)

35 reference records from 5 sources (buzz, grokbot, qm, slack, teams) fall into 16 capability clusters, with 0 excluded. 12 Verevon baseline records are mapped. Full detail is in `04-search-discovery.clusters.json`.

## Shared baseline (table-stakes)
- **Permission-aware full-text message search** (C-001; buzz, qm, slack, teams). The index only proposes candidates. The owner of access re-checks each hit, and each hit opens the message in its context.
- **Operator grammar** (C-002; buzz, slack, teams). Supports from:, in:, with:, is:, has:, before:/after:.
- **Scoped search with Ctrl+F** (C-003; buzz, grokbot, slack, teams). Search covers this room, this thread or one person.
- **People directory and search** (C-011; all 5 sources).
- **Room directory with browse and join** (C-012; buzz, slack, teams. The member record is DOC only).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Message search engine (C-001) | buzz (SRC) | Postgres generated tsvector and GIN index, with no separate search service. Sensitive kinds get a NULL tsvector, so they never match. Each candidate is re-authorized on the server, so search is never the access boundary. Teams contributes the API shape: suggestions, history, feedback events and query alteration. |
| Operators (C-002) | slack (DOC; q5) | Has the broadest grammar. Buzz's `parseSearchOperators` (SRC) is the parser to port: operators only at token start, and malformed values stay in the free text. |
| Scoped search (C-003) | teams (REA; q5) | Covers channel, chat, thread, post and person-centred scopes. Slack routes the native Find command to in-app search. |
| Result domains and history (C-004) | teams (REA) | Files by type, meetings, people, channels and settings, with filters and counts. |
| Cmd+K palette (C-005) | qm (SRC) | Searches conversation entries and resources in parallel, with a 150 ms debounce. GrokBot contributes the command registry with hotkeys. |
| Federated search (C-006) | qm (SRC) | Pluggable `SearchBackend` with `failed[]`/`limited[]`, so a gap is reported instead of an empty result. |
| AI answers with citations (C-007) | slack (DOC) | Answers come only from content the asker can access. The index covers files, canvases, clip transcripts and connected documents. Teams adds an "Ask Copilot" action on selected text. |
| Agent history search (C-008) | qm (SRC; q4) | `sessions.search` returns references (session id and entry seq) and never widens access. The `history` tool reopens one exact entry. GrokBot adds scopes and a prompt rule to trust the newest result. |
| Search as the asking human (C-009) | slack (DOC; q5) | The Real-time Search API takes an action token minted from the triggering event, has per-content scopes, and forbids storing results. |
| People directory (C-011) | teams (REA) | Covers the org, contacts and external users found by email only. qm contributes resolver errors an agent can read (ok, ambiguous, none). |

## Differentiators
- C-006 federated search with partial-failure reporting (qm, slack).
- C-007 AI search answers (slack, teams). Buzz explicitly has none.
- C-009 delegated-authority agent search (slack, qm).
- C-016 OS search-index hooks (slack, teams). Both are gated or disabled on Windows.
- Unique to one source: C-010 memory notebook (qm), C-014 marketplace (grokbot), C-015 discover feed (teams).

## Verevon's biggest gaps
1. **No message, thread or room search at all** (C-001, absent; priority, significance 5). Gateway `search.rs` covers web, image and video search only. Recommendation: build a tsvector index in Session Core (Model Plane), which owns room posts. Exclude ZDR turns at index time. Reuse the "room transcript read as room" decision and its audience-revision predicate for per-hit re-authorization. Thread deep links (`?thread_id=`) are generated but never read, and the thread list has no paging. Fix both so search hits open in context.
2. **In-room scoped search and Ctrl+F** (C-003, partial; priority). Today there is only a client-side filter over titles and previews of the selected room.
3. **Agents cannot search room history** (C-008, partial; priority). `knowledge_search` (cap.retrieval.query) covers Data Plane documents only. Add `search_space_history` and `open_space_entry` tools, gated by a capability and run with the turn's Control decision. This status comes from a grep in this step; no baseline record covers it.
4. **Cmd+K palette** (C-005, absent; recommended). Cmd+K opens web search today. Build the palette from the existing typed action registry and the org agent registry.
5. **Room browse and join** (C-012, absent) and a **searchable org directory** (C-011, partial). Both are recommended and belong in the Control Plane. Self-join must never widen the ACL on bound documents.
6. **AI answers over room messages with citations** (C-007, partial; recommended). Grounded turns exist but cover Space documents and wiki only. Space knowledge is also empty in practice, because no import writes `space_ref`.

Skipped: C-015 discover feed (low value, ZDR-sensitive signals) and C-016 OS indexing (Verevon v3 is web-only and has no desktop shell; ZDR conflict).

## Verevon strengths (keep)
- **Retrieval quality.** The Data Plane retrieval engine combines dense, keyword, graph and rerank search with RRF fusion (`search/fusion.rs`, k=60). That is ahead of buzz (FTS only) and qm (all-words filter).
- **Policy-gated grounding.** A separate `retrieval_read_entitled` bit is checked, and a 403 degrades the turn to ungrounded instead of failing it.
- **Signed, effect-bound Control decisions.** They are already a stronger "search as the asker" primitive than Slack's action token. They should be the only path for agent search.
- **Per-section gap reporting in the Knowledge tab.** This is the same contract as qm's `failed[]`/`limited[]`.
- **Agent memory tools** (`save_memory`/`recall_memory`, C-010). Verevon is equal or ahead here. Borrow qm's recall deltas and per-scope audience labels.
- **Room-scoped @ candidates** from the Control roster (C-013). The core scoping is already correct.

## Caveats
- **Weak provenance in some clusters.** Slack product records are DOC only (help centre and API docs). Teams records are REA string and locale evidence, so they show the feature exists but not its server behaviour. The C-012 member record is DOC only, and teams-shell-and-native-091 is INF with low confidence.
- **Status from a quick grep, not a baseline record.** The Verevon status for C-008 and C-010 comes from a grep in this step: execution-core `capability_policy.rs:795`, `runtime_loop/mod.rs:71,77`. Whether Space room turns offer these tools needs a code check. C-015 is "absent" without a code check.
- **Prevalence counts.** A source counts as having a capability when any of its records evidences it, even when that record sits in another cluster (for example, buzz's channel directory inside buzz-product-051). GrokBot and qm are agent harnesses, so table-stakes for human-chat features was judged mainly on buzz, Slack and Teams.
- **Unverified dependency.** ZDR in Session Core is decoded but not acted on yet (baseline). Any index must not ship before that is enforced.
