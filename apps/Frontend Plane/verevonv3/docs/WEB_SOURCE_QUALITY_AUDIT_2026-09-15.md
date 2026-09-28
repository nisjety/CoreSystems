# Web-search source quality audit — Verevon chat

**Date:** 2026-09-15 · **Scope:** web search only (the `web_search` tool, forced web
grounding, deep research, verification escalation). Knowledge-base / Data Plane
retrieval is explicitly out of scope. **Type:** research & audit — no code changed.

Method: end-to-end code trace of model-gateway → quarry-edge → quarry-runtime →
SearXNG/Brave (two independent tracing passes, five load-bearing claims re-read by
hand), live probes of the running SearXNG and quarry-edge containers, and a review of
how Perplexity / OpenAI-style deep research / Tavily / Exa / Brave approach the same
problem. Findings marked **[verified]** were read in the code or observed live; the
rest come from the traces and are cited by file:line so they can be checked.

---

## 0.0 Remediation status — updated 2026-09-16

> The audit below records what was **measured on 2026-09-15** and is left
> intact. This section records the remediation. Everything marked ✅ was proved
> by a build plus a live run against the stack — the quarry crates compile only
> in Docker on this host, so "compiles" means a `Dockerfile.edge --target build`
> pass with the crate confirmed *not* served from cache.

### Measured before → after, same queries

| | Before | After |
|---|---|---|
| Live general engines contributing | **1** (`google cse`) | **8** |
| Results per query | 20 | **70–93** |
| Results corroborated by >1 engine | 0 | **13–14** |
| Page text used for grounding | none | **3 pages read, inside a 6 s budget** |
| Structured facts per page | 0 | **64 typed figures** with units and periods |
| The Oslo-population question | 60 % confidence, no number | **86 %, 729 437, SSB tabell 01222, Q2 2026** |

### The 13 findings

| # | Status | Note |
|---|---|---|
| W-01 snippet-only grounding | ✅ | `grounding.rs`: fetch top hits, select passages, tier-aware wall-clock budget, labelled snippet fallback |
| W-02 effectively one engine | ✅ | 12 engines declared, 8 contributing; each in its own suspension bucket |
| W-03 unofficial Google scrape as sole source | ✅ mitigated | No longer sole; `braveapi` (official) available, env-gated and inert without a key |
| W-04 no locale reaches search | ✅ | `language`/`country`/`topic`/`time_range` derived per turn and sent; adapter bug (country in the language slot) fixed |
| W-05 ranking = engine order | ✅ | Cross-engine RRF fusion using SearXNG's own `positions[]`; semantic rerank + autoprompt enabled in the deployed config |
| W-06 no web citation floor | ✅ | Thin sources reach context with a warning, withheld from citations |
| W-07 exact-URL dedup only | ✅ | Canonical dedup key (scheme, `www.`, trailing slash, tracking params); duplicates now *merge* into agreement evidence |
| W-08 raw conversational query | ◐ | Autoprompt rewriting enabled server-side; no model-written query on the forced path yet |
| W-09 signal discarded (`highlights`, `rank`, `intent`) | ✅ | `engines[]` forwarded as a capped agreement signal; intent hint now actually honoured by the router |
| W-10 deep research reads 3 of 24 | ✅ | Budget by *successful* reads (16, ceiling 24), typed unread statuses, read/unread split in the UI |
| W-11 thin fetcher | ✅ | Charset decoding, PDF routing, typed failure reasons, minimum-useful-length rule |
| W-12 local index empty / in-memory | ✅ | On-disk and persistent; write-back extended to the answer path; upsert, retention, commit ticker. Live: 8 docs indexed, retention pass running |
| W-13 no source-quality telemetry | ✅ | `search.upstream`, `search.engine.contribution`, `search.result_shape` — and it caught a real bug on first use (N-08) |

### New findings discovered *during* remediation

These were not in the original audit. Several are more consequential than what
it found.

| # | Finding | Status |
|---|---|---|
| N-01 | **The real cause of the SSB failure.** The page was fetched fine (620 966 B, number present); readability strips `<script>` by design, and SSB ships the figure in 29 hydration payloads totalling 461 890 chars — **74 % of the page was machine-readable data we discarded.** Not a JavaScript problem. | ✅ structured harvester; verified against the real page |
| N-02 | Stock `crossref` ships `timeout: 30` and SearXNG sets one deadline per search as `max(engine timeouts)` — so **every science search was pinned to the 20 s ceiling** | ✅ lowered to 15 s |
| N-03 | `brave.images/videos/news` declare `network: brave`, sharing one suspension bucket, and quarry genuinely issues those vertical requests — their 429s were **suspending brave itself** | ✅ verticals disabled; brave now answers 16–20 results/query with no key |
| N-04 | `QUARRY_EDGE__BROWSER_PROVIDER` is a **phantom variable no code reads**, declared in two compose files and misleading operators | ✅ closed 2026-09-16 — **and it had already produced a wrong conclusion, in this document.** Reading `BROWSER_PROVIDER=static` led to the claim that there was "no browser to escalate to". There always was: quarry-edge logs `browser driver registered (local chromiumoxide)` on every start, from the `browser-agent` cargo feature the edge image is built with (`main.rs` also accepts `BROWSERBASE_API_KEY` + `BROWSERBASE_PROJECT_ID`, which it really does read). The browser fallback was armed the whole time; the only thing stopping it was the vendor-marker detector, now symptom-based. The dead vars are removed from the root compose, with the reasoning recorded there so the misreading cannot recur. |
| N-05 | **execution-core never sent `zdr`** on `/v1/search` or `/v1/extract`, despite having it in scope — agentic runs persisted ZDR tenants' fetched content | ✅ fixed |
| N-06 | Video search **bypassed quarry-edge entirely**, reaching SearXNG un-attributed: no org scoping, no quarry token, no cache, no diversity cap, no billing. The tell: the only handler in the file without an authenticated-user extension | ✅ proxied through a new edge route; images + videos now metered |
| N-07 | **Entity-match hole in the instant-answer short-circuit**: "innbyggere i Bergen kommune" matched `kommunefakta` in `ssb.no/kommunefakta/oslo`, so Oslo's figure would be served as Bergen's — confidently, with no further search | ✅ named entities absent from the source are now fatal |
| N-08 | Every forced search sent `"intent": "answer"` — **in no vocabulary quarry accepts** — so the hint was discarded on the one path that always fires | ✅ derived from `ForcedSearchReason`; cross-service vocabulary tripwire test added |
| N-09 | `get_statistics` used contents code `Folketallet` — the page's *display label*, not a code. SSB refused every query. `Folketallet1` is the quarter's **opening** figure, so a plausible guess would have been quietly off by one quarter | ✅ `Folketallet11`, verified live (`value: [729437]`) |
| N-10 | Local index wrote unknown-org documents under the empty string, and unscoped reads could see them | ✅ unknown org now skips indexing |

### Second remediation round — 2026-09-16

Working through the leftovers surfaced a theme worth stating plainly: **of the
items that looked like product bugs, three were wrong tests.** A test that is
red for a reason unrelated to what it guards stops being read, which is the
worst possible state for a security invariant — so these are fixed as defects
in their own right, not as chores.

| # | Finding | Status |
|---|---|---|
| R-01 | **HITL approval persistence could hang.** `grpc.rs` built four channels with bare `connect_lazy()` — no connection bound — while every sibling in the crate (`auth.rs`, `capability_client.rs`, `capability_policy.rs`) bounds its own. `session_channel` is the one `create_durable_approval` uses, so with orchestration-core unreachable a human-in-the-loop pause could neither complete nor fail. Linux masked it (fast ECONNREFUSED); Windows exposed it, which is why the guarding test was dismissed as flaky. | ✅ shared `lazy_bounded_channel` used by production *and* the test — a test bounding its own channel proves nothing. Connect bounded for all four; no blanket request timeout, since inference and browser calls legitimately run for minutes |
| R-02 | **Model-authored `limit` forwarded unclamped.** execution-core passed the model's `limit` straight to quarry-edge on `web_search` and `news`, while keeping only 8 results — so anything above that was upstream work this process then discarded | ✅ clamped, following the crate's existing `search_memory` convention |
| R-03 | **Kilder interrupted for sources nobody read.** `claimsFocus` used the full source count, so the panel could pull focus to show a lead that was found but never fetched | ✅ `claimsFocus` keys on READ sources; `available` keeps the full count, because a lead is still worth *offering*. An omitted count means "none read" rather than falling back to the wider number |
| R-04 | **Go protobuf stubs were stale** — and not only for this work's new field. Regeneration restored real proto fields the checked-in stubs were missing, including a Space-authority field with its full doc comments | ✅ regenerated via the project's own `buf.gen.go-only.yaml`; no plugin-version churn; every Go module builds against the result |
| R-05 | `docs/openapi.yaml` did not document the additive `structured` field on `ExtractItem` | ✅ documented against the real Rust shape, including why `period` matters on a figure |
| R-06 | **capability-core's migration security test was line-ending-fragile.** The assertion embedded a bare `
`; the file is CRLF. The invariant it guards — the tenant-scope migration failing closed for legacy rows — was intact the whole time | ✅ line endings normalised at the read, for all five migration security tests |
| R-07 | **The executor sandbox test compared a Windows path to a POSIX one**, and fixing that exposed a second assertion that looked like a genuine environment-isolation leak: the child reported the parent's `HOME`. It is not a leak — verified by spawning `sh` from PowerShell with a fully cleared environment, where `USERPROFILE` is absent (so the clear worked) but `HOME` is still present: **MSYS's `sh.exe` fabricates it.** `env_clear()` is correct | ✅ probes a canary variable no shell can synthesise — a stricter check than `HOME` ever was. execution-core now 586/0 |
| R-08 | The news-vertical fix (recency expressed by `time_range`, never `topic=news`) was staged but undeployed | ✅ deployed and confirmed live: the query that returned `hits_total: 0` now returns 5, with `intent_hint: "fresh"` / `intent_hint_effect: "used_agreed"` |

Still open, all low-priority: vitest worker-start flakiness on this host (roughly
half the files time out on a first run; `--pool=threads` plus a retry clears it),
F-12's "Resultat tab on stop→regenerate" never re-verified live, and the
subscription broker's 60–95 s latency, which is infrastructure rather than chat
code.

### Decisions taken (§5 of the original audit)

1. **Egress** — Brave approved for non-ZDR tenants on Balance/Genius; enforced by a
   fail-closed `allow_paid_providers` boolean, ANDed with the pre-existing ZDR gate.
   *Observed in practice:* the Oslo query was served entirely by SearXNG —
   Brave is a paid **backup** and the free chain no longer underdelivers.
2. **Latency** — passage grounding on all tiers with a tier-aware wall-clock cap
   (~6 s Balance/Genius, ~3 s Budget), falling back to a labelled snippet.
3. **Authority bonus** — yes, soft, never a filter; registrable-domain matched and
   sized so bonuses alone can never carry a hit past the keep threshold.
4. **Deep research** — "read more": 16 successful reads, ceiling 24, with coverage
   reported proportionally so a bigger budget cannot silently inflate confidence.

## 0. One-screen verdict

The thin, off-topic sources are **structural, not bad luck**. Today a Verevon web
answer is grounded on ~150-character search-engine snippets from what is effectively a
single upstream engine, ordered by that engine's own order, with no page ever read
unless the model spontaneously calls `fetch_url`.

| # | Finding | Effect on source quality | Fix class |
|---|---|---|---|
| W-01 | **Snippet-only grounding.** `web_search` returns `url/title/snippet` and never a page body; nothing fetches the top hits before the model answers. **[verified]** `tool_loop.rs:2151-2180` | The model answers from 81–165 chars per source. This is the direct cause of F-13 (“confident answer on 63/84-char sources”). | pipeline |
| W-02 | **Effectively one engine.** Live: for every Norwegian *and* English test query only `google cse` answered; Brave (429, suspended), DuckDuckGo (CAPTCHA), Startpage (CAPTCHA, 1 h suspension) all failed. 24 h of logs show the same three failing continuously. **[verified live]** | No cross-engine corroboration, no diversity; when Google’s unofficial endpoint blinks, results go to zero silently. | provider strategy |
| W-03 | **That one engine is an unofficial scrape.** SearXNG’s `google cse` engine hits `cse.google.com/cse/element/v1` with a hard-coded public `cx`, `use_official_api: False`, no key, no quota we control. **[verified]** | Can break or be rate-limited at any time with no SLA; already the only thing standing. | provider strategy |
| W-04 | **No locale reaches the search layer.** SearXNG `default_lang: "en"`; model-gateway sends no `language`/`country`; quarry’s SearXNG adapter sends `country` in the `language` slot and drops `language`. **[verified]** `serp.rs:455-457`, `quarry.rs:670-693` | Norwegian questions searched with an English bias; Norwegian authorities (SSB, SNL, Lovdata, Norges Bank…) not preferred. | caller + adapter |
| W-05 | **Ranking = engine order.** Multi-provider merge is concatenation in provider order with positional re-ranking; semantic rerank (`QUARRY_EDGE__SEMANTIC_RERANK`) and autoprompt query rewriting are both **off** in the deployed config. `smart_router.rs:766-776`, compose `:90-96` | Nothing in the pipeline ever scores *relevance to the question*; the gate downstream only sees title+snippet. | config → pipeline |
| W-06 | **No citation floor on web sources.** `MIN_CITABLE_CONTENT_CHARS=150` guards KB snippets only; `web_search_citations` cites any kept hit regardless of length. **[verified]** `tool_loop.rs:3904-3917` | A 60-char snippet becomes a numbered source in the Kilder tab. | small code |
| W-07 | **Dedup is exact-URL only** (`www.`, scheme, trailing slash, `?utm_*` all count as different sources); none at all on the plain-chat path. **[verified]** `smart_router.rs:768-776` | Same page counted twice as “corroboration”; wastes the 5-result budget. | small code |
| W-08 | **Forced search ships the user’s raw sentence as the query**, hard-coded `limit: 5`, no rewrite, no retry. `run_forced_web_search`, `tool_loop.rs:3697-3818` | Conversational Norwegian (“hvor mange innbyggere er det egentlig i Oslo nå?”) is a poor search string. | caller |
| W-09 | **Signal is thrown away twice.** Quarry returns reranker `highlights` and `rank`; model-gateway drops them. model-gateway sends `intent`; quarry-edge has no such field and serde discards it. **[verified]** | The best evidence available is never used; the documented intent hint is a no-op. | small code |
| W-10 | **Deep research reads ≤ 8 of ~24 sources, then loses more silently.** Cap `DEEP_RESEARCH_MAX_PAGES=8`; JS-rendered / non-UTF-8 / oversize pages fail with no caller-visible reason; Kilder shows all 24 with only a `[not read: …]` prefix. `deep_research.rs:1021,1285`, `answer.rs:358-376` | Explains F-16 (“read 3 of 24”). Users cannot tell evidence from decoration. | pipeline + UI |
| W-11 | **Fetcher is the thin one.** Search-side page fetch = plain GET, 10 s, 5 MB, no robots, no retries, no JS, no PDF, drops non-UTF-8 (`from_utf8(..).ok()?`). The rich `/v1/scrape` pipeline (browser, PDF, readability) exists but the search/answer path does not use it. `answer.rs:310-382` | Norwegian public-sector PDFs and JS sites (regjeringen.no, many kommune sites) become “no content”. | pipeline |
| W-12 | **Local Tantivy index is in-memory and empty** (`LOCAL_INDEX_DIR` unset; wiped on every restart), Stract and Serper unset. `main.rs:719-783` | The “four-tier router” is one tier; cache-of-good-sources value is zero. | config |
| W-13 | **No source-quality telemetry.** No per-engine hit counts, drop reasons, fetch-failure reasons, or snippet-length stats reach logs/metrics. | We only found W-02 by curling the container. | observability |

Bottom line: fixing W-01, W-04 and W-05/W-06 changes the product; W-02/W-03 is a
reliability risk that needs a provider decision from you (see §5).

---

## 1. How a web answer is produced today

```
user message
  └─ sse.rs: should_force_web_search? (EN+NO recency tokens, recent year)   [tool_loop.rs:884-906]
       ├─ yes → run_forced_web_search(raw message, limit 5, intent "answer")  [3697-3818]
       └─ model decides → web_search{query, limit≤50 (default 5)}            [2128-2202]
  └─ tools.rs handle_web_search → quarry::Client::search
       POST quarry-edge:8082/v1/search  {query, limit, intent, zdr}           [quarry.rs:659-726]
         (no language, country, topic, time_range, include/exclude_domains — all accepted by the API)
  └─ quarry-edge SmartSearchRouter                                             [smart_router.rs]
       tantivy_local (empty, in-memory) → SearXNG (http://searxng:8080) → Brave (paid backup)
       merge_dedupe (exact URL) → positional rank → MAX_PER_HOST=3 demote → cache 300/900 s
       semantic rerank OFF · autoprompt OFF · LLM intent OFF
  └─ SearXNG (stock engines, default_lang en, safe_search 0, limiter off)
       general: duckduckgo ✗CAPTCHA · startpage ✗CAPTCHA · brave ✗429 · google cse ✓ (unofficial element endpoint)
       → 20 hits, snippets 42–170 chars (median ~158), no publishedDate, score = 1/rank
  └─ back in model-gateway: keep url/title/snippet(+score>0) → relevance gate on title+snippet
       keep ≥0.30, fallback top-3 “WEAK EVIDENCE”, MAX_WEB_CITATIONS=5, dropped hits listed “SET ASIDE”
  └─ model answers from snippets; fetch_url only if the model chooses (4 000 chars/page)
  └─ verification: if confidence < 0.75 and answer ≤ 300 tokens → KB re-query → optional web re-search with the ANSWER as query
```

Live evidence (2026-09-15, local stack):

* `GET /search?q=hvor+mange+innbyggere+er+det+i+Oslo&language=nb-NO` → 20 results, all
  `google cse`, `unresponsive: brave (too many requests), duckduckgo (CAPTCHA), startpage
  (Suspended: CAPTCHA)`; snippet lengths 81–165; `publishedDate: null` on every hit.
  Same picture for “what is the population of Oslo”, “Aquatiq AS Norway”, “renteendring
  Norges Bank 2026”.
* SearXNG log, 24 h: DuckDuckGo CAPTCHA ×13, Startpage CAPTCHA (suspended 3600 s) ×7,
  Brave 429 (suspended 180 s) ×7.
* SearXNG `settings.yml` is `use_default_settings: true` with **no `engines:` block** —
  engine mix, weights and languages are whatever image `2026.7.12` ships.
* quarry-edge startup: `smart_router[+]: tantivy_local` (in-memory), `searxng`, `brave
  (paid backup)`; `intent classifier: rule-only (LLM disabled)`; `BROWSER_PROVIDER=static`.

---

## 2. Root causes, ranked by impact on “does the source actually talk about what we asked”

1. **We never read the page.** Everything downstream — the relevance gate, the
   citation, the confidence score, the verification judge — operates on a ~150-char
   engine snippet. A snippet says “SSB – Kommunefakta Oslo”; it does not say how many
   people live there. Perplexity and every serious deep-research agent fetch the top
   N pages and rerank *passages*; we cite the pointer instead of the evidence.
2. **We don’t tell the search layer what we want.** No language, no country, no
   freshness, no vertical (`news`), no site preferences — for a Norwegian B2B product
   whose forced-search heuristic exists precisely for recency-sensitive Norwegian
   questions. The plumbing for all of this already exists in quarry-edge’s request
   schema and in SearXNG (`language`, `time_range`, `categories`) and Brave
   (`country`, `search_lang`, `freshness`, `extra_snippets`).
3. **No relevance scoring anywhere.** Engine order → concatenation → position. The
   only relevance judgment is model-gateway’s lexical gate on the snippet, which then
   *keeps the top 3 anyway* when nothing passes.
4. **Single, unofficial, English-defaulted upstream.** Three of four general engines
   are permanently CAPTCHA/429’d from this IP; the fourth is a scrape of Google’s CSE
   element endpoint. This is the classic self-hosted SearXNG failure mode
   (datacentre IP ⇒ bot classification) and it will not improve with tuning.
5. **Silent loss everywhere.** Fetch failures, dedup, host-cap demotion, engine
   suspension and unread deep-research sources are all invisible to the caller and
   mostly to the user.

---

## 3. What the strong systems do differently (and which parts we already have)

| Practice | Perplexity / OpenAI DR / Tavily / Exa | Verevon today | Have the pieces? |
|---|---|---|---|
| Query understanding → rewritten, multi-query search | Intent parse, 3–6 sub-queries, language-aware | Raw user text (forced) or one model query; deep research does 6 sub-queries | `autoprompt.rs` (off), `/v1/search/suggest` (unused) |
| Wide candidate net (10–30 pages) from ≥2 indexes | Own crawler + Bing/partners; Brave own index | 5 hits from one engine | Brave key present; Stract/Serper wired but unset |
| Fetch full page → extract → **passage** rerank | Cross-encoder / XGBoost quality gate over passages | No fetch; snippet gate | `/v1/scrape` (readability, PDF, browser); `rerank.rs` (LLM judge, off) |
| Authority & freshness signals | Domain authority, dated facts, load-time budget | None; `publishedDate` null; `safe_search 0` | `include_domains`, `time_range`, host diversity exist |
| Cite the passage actually used | Inline citation per claim | Cite the search hit | Citation events already carry snippet text |
| Show read vs unread | Sources = pages read | 24 rows, 3 read, same styling | `dr-unread-N` prefix already distinguishes them |
| Evaluate faithfulness | FACT/RACE-style metrics; “more retrieval ≠ more accuracy” | `verification.rs` judge over snippets | Judge exists; feed it page text |

Two research notes worth carrying into design: (a) “links + snippets are not
evidence” is the consensus reason snippet-RAG hallucinates; (b) the deep-research
evaluation literature finds that *increasing search depth degrades factual accuracy*
unless citation grounding is verified per claim — so “read more sources” without
passage-level grounding will not fix F-16.

---

## 4. Recommendations

Ordered so that each tier is independently shippable. Estimated effort is for the
change itself, not the verification pass.

### Tier A — configuration and caller fixes (days, low risk, large effect)

| # | Change | Where | Why it helps | Risk |
|---|---|---|---|---|
| A1 | Send `language`, `country`, `topic`, `time_range` from model-gateway; derive `language` from the user’s UI locale / message language, `country: "NO"` for the Norwegian tenant, `topic: "news"` + `time_range` when `should_force_web_search` fired on a recency token, and stop sending the dead `intent` field. | `quarry.rs:670-693`, `tool_loop.rs:2128-2202`, `run_forced_web_search` | Fixes W-04/W-09 at the source; SearXNG `google cse` and Brave both honour language + time range. | Low |
| A2 | Fix the SearXNG adapter to send `language` as SearXNG `language` and `safesearch`; map `country` to a locale suffix (`nb-NO`). | `serp.rs:455-457` | Currently a country code lands in the language slot. | Low |
| A3 | Turn on `QUARRY_EDGE__SEMANTIC_RERANK=1` (top_n 10–15) and `QUARRY_EDGE__AUTOPROMPT=1`, and forward quarry’s `score`/`highlights` into the gate instead of dropping them. | compose `:90-96`, `tool_loop.rs:2151-2176`, `relevance.rs` | Gives the pipeline its first genuine relevance score; the code is already written and degrade-safe. Measure latency (LLM judge ≈ +0.5–1.5 s). | Medium (latency) |
| A4 | Add a web citation floor mirroring the KB one (≥150 chars of *evidence text*, see B1) and URL canonicalisation (scheme, `www.`, trailing slash, tracking params) before dedup — on the router and on the chat path. | `tool_loop.rs:3904`, `smart_router.rs:768`, reuse `deep_research::normalize_url_key` | Closes W-06/W-07 directly. | Low |
| A5 | Tune SearXNG instead of shipping defaults: pin the `general` engine set to those that actually answer from this IP; add `language: nb-NO` engine variants (SearXNG supports per-engine language entries), set `default_lang: "nb-NO"` for the Norwegian deployment, `safe_search: 1`, and enable `enable_metrics: true`. | `apps/Ingestion Plane/config/searxng/settings.yml` | Stops paying 10 s timeouts for three dead engines on every query; gets Norwegian-language results. | Low |
| A6 | Persist the local index (`QUARRY_EDGE__LOCAL_INDEX_DIR`) so pages already read become a first tier. | compose | Cheap corroboration and stability across restarts. | Low |
| A7 | Emit source-quality telemetry: per-engine answered/suspended counts, snippet length histogram, fetch failure reason, dedup/host-cap drops, gate keep/drop counts, `fallback_used`. | quarry-edge `telemetry.rs`, model-gateway F-11 log line | W-13. You cannot manage what you cannot see; today W-02 was invisible. | Low |

### Tier B — read the page (1–2 weeks, the product change)

| # | Change | Where | Why |
|---|---|---|---|
| B1 | **Fetch-then-answer on the chat path:** after the gate keeps N hits, fetch the top 3–5 through quarry’s *real* scrape pipeline (`/v1/scrape`: readability, PDF, static→browser fallback, robots), split into passages, keep the 1–2 passages per page most similar to the question, and give the model *those* as the source text — with the citation snippet replaced by the quoted passage. Budget: ~1 500–2 500 chars per source, hard wall-clock 4–6 s with per-page timeout and graceful fallback to the snippet *labelled as such*. | new step between `gate_web_search_outcome` and `format_tool_context`; `quarry.rs` already has `scrape_readable` | This is the single change that makes “the source actually talks about what we asked” true. It also gives the verification judge real text. |
| B2 | Make the fetcher honest: surface `fetch_failed{reason}` per URL to the caller; handle non-UTF-8 (decode by charset header / `encoding_rs`), PDFs (route to `quarry-transform::pdf`), and JS-only pages (browser driver when `static` returns < 400 chars). | `answer.rs:310-382`, deep research `read_pages` | W-10/W-11; Norwegian public sector is PDF-heavy. |
| B3 | Rewrite the query before searching: one small-model call producing 1–3 search strings (Norwegian + English variant when the topic is international), entity-preserving, ≤ 12 words. Use it for the forced path and for verification escalation (which today searches with the *answer text*). | `run_forced_web_search`, `verification_query`; quarry `autoprompt.rs` can host it | W-08; multi-query is the cheapest recall win in every RAG benchmark. |
| B4 | Deep research: raise/adapt `MAX_PAGES` to “until 6 pages *succeeded*”, order reads by passage relevance rather than tier→rank, and render unread sources in a separate “Ikke lest” group in Kilder. | `deep_research.rs:996-1021,1285`, SPA Kilder renderer | Turns F-16 from a mystery into an explicit budget. |

### Tier C — provider strategy (decision needed, see §5)

| Option | What it buys | Cost / constraint |
|---|---|---|
| C1 **Brave as primary, SearXNG as fallback** (flip current order; you already hold a key) | Own index, `country=NO`, `search_lang=nb` (reported supported — verify against Brave’s code table), `freshness`, `extra_snippets` (up to 5 extra excerpts per hit → partially mitigates W-01 even before B1), lowest latency in 2026 agent benchmarks, no CAPTCHA problem. | ~$3–5 / 1 000 queries; egress to a SaaS (your `ZERO_SAAS_SEARCH` switch already models this; ZDR interplay must be checked). |
| C2 **Keep SearXNG but fix its egress** (residential/rotating proxy or the official Google JSON API) | Keeps zero-SaaS posture. | Proxies are a cat-and-mouse game; the official CSE JSON API is closed to new customers and sunsets 2027-01-01 — not a path. |
| C3 **Add a second index for corroboration** (Stract self-hosted, or Exa/Tavily for semantic “find pages about X”) | Cross-engine agreement is a strong relevance signal the gate can use. | Stract is heavy to run; Exa/Tavily are SaaS with cache-staleness caveats. |

Recommendation: **C1 + A5** (Brave primary with Norwegian locale, a pruned Norwegian-tuned SearXNG as fallback and diversity source), then **B1**. That sequence removes the single point of failure this week and fixes the evidence problem next.

---

## 5. Decisions I need from you

1. **Egress / data posture for web search.** Is sending user queries to Brave (or any SaaS) acceptable for non-ZDR tenants? `QUARRY_EDGE__ZERO_SAAS_SEARCH` suggests this was already a considered switch. If yes → C1. If no → C2/C3 and accept the SearXNG reliability ceiling.
2. **Latency budget for a grounded chat answer.** B1 adds roughly 2–5 s (parallel fetch + passage selection). Acceptable for Balance/Genius? Should Budget stay snippet-only?
3. **Authority list.** Do you want an explicit preferred-domain list for the Norwegian tenant (ssb.no, snl.no, lovdata.no, regjeringen.no, norges-bank.no, nrk.no, altinn.no, brreg.no, skatteetaten.no, mattilsynet.no …) used as a rerank bonus (not a filter)? The `include_domains` plumbing exists; a bonus needs a small gate change.
4. **Deep research budget.** Prefer “read fewer, read fully, cite passages” (literature says accuracy goes *down* with more shallow retrieval) or keep the wide 24-source list with a clear read/unread split?

---

## 6. Appendix

### 6.1 Knobs that matter today

| Knob | Default | Location |
|---|---|---|
| `QUARRY_EDGE_URL` / `QUARRY_EDGE_TIMEOUT_SECS` | unset → web off / 30 s | `model-gateway/src/state.rs:636-638` |
| `web_search.limit` default | 5 (clamp 1–50) | `tool_loop.rs:2134` |
| `MAX_WEB_CITATIONS` | 5 | `tool_loop.rs` |
| `VEREVON_RELEVANCE_KEEP_THRESHOLD` / `_FALLBACK_KEEP` | 0.30 / 3 | `relevance.rs:371,404` |
| `MIN_CITABLE_CONTENT_CHARS` (KB only) | 150 | `retrieval.rs:57` |
| `MAX_FETCH_CHARS` / `MAX_TOOL_OUTPUT_CHARS` | 4 000 / 8 000 | `tool_loop.rs` |
| `DEEP_RESEARCH_MAX_SUB_QUERIES` / `_MAX_PAGES` / `_PAGE_CHARS` / `_CORPUS_CHARS` / `_TIMEOUT_SECS` | 6 / 8 / 4 000 / 40 000 / 120 | `deep_research.rs:214-254` |
| `QUARRY_EDGE__SEMANTIC_RERANK` (+`_MODEL`, `_TOP_N`) | **0** / top_n 10 | Ingestion compose `:94-96` |
| `QUARRY_EDGE__AUTOPROMPT` | **0** | compose `:90` |
| `QUARRY_EDGE__ZERO_SAAS_SEARCH` | 0 (Brave enabled) | compose `:99` |
| `QUARRY_EDGE__LOCAL_INDEX_DIR` / `STRACT_URL` / `SERPER_KEY` | unset | `main.rs:719-783` |
| `QUARRY_EDGE__FETCH_TIMEOUT_S` / `BROWSER_PROVIDER` | 30 / `static` | container env |
| Router provider timeout / breaker / cache | 10 s / 3 fails·60 s·30 s / 300 s | `smart_router.rs:116-120` |
| Search cache TTL | 900 s (300 recency, 120 news/finance) | `search_routes.rs:132` |
| `MAX_PER_HOST` (demote) | 3 | `search_routes.rs:223` |
| SearXNG `default_lang` / `safe_search` / `request_timeout` | `en` / 0 / 10 s (20 max) | `config/searxng/settings.yml` |

### 6.2 Evidence trail

* model-gateway trace: `sse.rs:1316-1383,1659-1698`; `tool_loop.rs:884-906,2128-2202,3392-3396,3697-3818,3863-4035`; `relevance.rs:69-123`; `retrieval.rs:57,451`; `deep_research.rs:214-264,724,927-1021,1285`; `verification.rs:89-181`.
* quarry: `Quarry-v2/crates/quarry-edge/src/{routes.rs:159,search_routes.rs:34-120,153-246,491-537}`; `quarry-runtime/src/{serp.rs:20-38,104-157,426-484,smart_router.rs:104-120,443,514-520,766-776,874-935,fusion.rs:29-93,rerank.rs,autoprompt.rs,answer.rs:207-215,310-382}`; `quarry-transform/src/readability.rs:116-147`; `crates/quarry-edge/src/main.rs:719-783`.
* SearXNG: `apps/Ingestion Plane/config/searxng/settings.yml`; stock `searx/engines/google_cse.py` (`use_official_api: False`, `require_api_key: False`, `cse.google.com/cse/element/v1`, `time_range_support`, `language_support`).
* Live probes: SearXNG `/config` and four `/search?format=json` queries; `docker logs ingestion-searxng --since 24h`; quarry-edge startup log and env (secret values never printed).

### 6.3 External references used

* Perplexity pipeline (hybrid retrieval → multi-layer rerank → cited synthesis): ziptie.dev “How Perplexity AI answers work”; authoritytech.io “How Perplexity selects sources (2026)”.
* Snippet vs page grounding, query rewriting, cross-encoder reranking: dev.to “Links + snippets not enough for RAG”; Towards Data Science “Advanced RAG retrieval: cross-encoders & reranking”; Microsoft “Generative query rewriting and new ranking model”; DMQR-RAG (arXiv 2411.13154).
* Deep-research evaluation (“more retrieval ≠ more accurate citations”): “Cited but Not Verified” (arXiv 2605.06635); DeepResearch Bench (2506.11763) and Bench II (2601.08536).
* Search APIs for agents 2026: brave.com “Best search APIs for AI 2026”; aimultiple “Agentic search benchmark”; Brave Web Search API docs (`country`, `search_lang`, `freshness`, `extra_snippets`, count ≤ 20).
* SearXNG reliability: searxng/searxng issues #2515, #3927, #6596; docs.searxng.org limiter; ssdnodes “Fix SearXNG engine CAPTCHA errors”.
* Google Programmable Search JSON API: closed to new customers, 10 000/day cap, sunset 2027-01-01 (developers.google.com custom-search; expertrec 2026 guide).
* Multilingual rerankers: BAAI `bge-reranker-v2-m3` (Apache-2.0, 100+ languages, ~50–100 ms GPU) as the self-host default; Cohere Rerank 3.5 / Jina v2 as API options.
