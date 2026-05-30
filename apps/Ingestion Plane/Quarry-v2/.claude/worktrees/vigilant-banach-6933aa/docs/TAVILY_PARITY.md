# Tavily Parity — Feature Matrix

This document tracks Quarry's coverage of the Tavily Search & Answer API. The
goal is functional parity for the most common use cases (search, extract,
synthesised answer) while running entirely on infrastructure we control.

Last updated: cycle 19 (Tantivy + Stract + SearXNG + Brave + SmartSearchRouter).

## Endpoint Map

| Tavily endpoint            | Quarry endpoint  | Status   | Notes                                                       |
| -------------------------- | ---------------- | -------- | ----------------------------------------------------------- |
| `POST /search`             | `POST /v1/search` | ✅ done  | Routed through `SmartSearchRouter`                          |
| `POST /extract`            | `POST /v1/crawl` | ✅ done  | Single-URL static + browser extract via PageRunner          |
| `POST /search?qa=true`     | `POST /v1/answer` | ✅ done  | `AnswerPipeline`: search → fetch → MP synth with citations  |
| `POST /search?topic=news`  | `POST /v1/search` (Fresh intent) | ✅ done  | Triggered automatically by classifier on "latest"/"breaking"/year tokens |
| `POST /search?topic=finance` | _planned_      | ⏳       | Adapter pass via SearchOptions; deferred                    |
| `/api/v1/search` (advanced)  | `POST /v1/search` w/ filters | 🟡 partial | Per-domain include/exclude not yet wired through router    |
| `/api/v1/search/typeahead`   | _autocomplete-core_ | 🚧 deferred | Sibling service scaffolded under `apps/Ingestion Plane/autocomplete-core/` |

## Search Provider Stack

`SmartSearchRouter` (built in `crates/quarry-runtime/src/smart_router.rs`)
assembles the configured providers in this priority order:

| Priority | Provider          | Tier  | When invoked                                                |
| -------: | ----------------- | ----- | ----------------------------------------------------------- |
|        1 | Tantivy local idx | free  | Always — searches our own scraped corpus first              |
|        2 | Stract            | free  | Parallel with local when intent ∈ {Default, Phrase}         |
|        3 | SearXNG aggregator | free | Parallel with local when intent ∈ {Default, Fresh}          |
|        4 | Brave             | paid  | Only when free tier returns < `min_total_results` (default 1) |
|        5 | Serper            | paid  | Same fallback gate as Brave                                  |

### Intent Classification

The router consults an `IntentClassifier`. Two production stacks are
supported:

**Tier 1 — Rule fast-path (always available):** `classify_intent(query)`
routes deterministically by query shape, no I/O, microsecond latency.

**Tier 2 — LLM-aware (opt-in via `QUARRY_EDGE__LLM_CLASSIFY_INTENT=true`):**
`CachedClassifier(60min) → HybridClassifier → MpIntentClassifier(150ms timeout)`.
The hybrid keeps the rule fast-path for obvious queries, and only consults
the LLM when rules return `Default`. The cache layer dedupes hot keys
across requests. Any LLM failure or timeout silently falls back to
`Default` — the search path is never blocked by Model Plane.

| Intent       | Trigger (rule)                                        | Trigger (LLM token)  | Strategy                                                |
| ------------ | ----------------------------------------------------- | -------------------- | ------------------------------------------------------- |
| Navigational | Pure host token (`github.com`)                         | `NAV`                | Tantivy + SearXNG; cached crawl history first           |
| Fresh        | Keywords (today, latest, breaking, news, year 2024-30) | `FRESH`              | Skip Tantivy; parallel Stract + SearXNG                 |
| Phrase       | Quoted string `"..."`                                  | `PHRASE`             | Tantivy first; widen to Stract if thin                  |
| Research     | _(LLM only)_                                           | `RESEARCH` / `RES`   | **Force fan-out** to every free engine in parallel       |
| Comparative  | _(LLM only)_                                           | `COMPARE` / `CMP`    | Force fan-out + larger limit                            |
| Local        | _(LLM only)_                                           | `LOCAL` / `LOC`      | Default topology; country bias passes to SearXNG/Brave  |
| Code         | _(LLM only)_                                           | `CODE`               | Default topology; Tantivy (dev docs) → Stract (repos)   |
| Default      | Everything else                                       | `DEFAULT` / unknown  | Tantivy → parallel Stract + SearXNG; paid backup if 0   |

### Reliability

- **Per-provider circuit breaker**: 3 consecutive failures within a 60s window
  trips the breaker; provider is skipped for 30s cooldown before retry.
- **Per-call timeout**: 10s per provider (configurable via `RouterConfig`).
- **TTL cache**: 5 min by default, keyed on blake3 hash of
  (query, country, language, limit). In-process; cleared on restart.
- **Parallel widening**: when local results < `min_local_results` (default 3),
  Stract and SearXNG are fired concurrently via `tokio::join!` and
  results are merged with first-occurrence URL de-duplication.

## Answer Pipeline (Tavily killer)

`AnswerPipeline` (`crates/quarry-runtime/src/answer.rs`) orchestrates:

1. `SearchProvider::search(query, opts)` → top-N candidates
2. `MarkdownFetcher::fetch(url)` → readable markdown per candidate
   - `SimpleHttpMarkdownFetcher` uses reqwest + `quarry-transform::readability`
   - 10s timeout, 5 MB body cap
3. `AiFormatRunner::summary(...)` → MP-backed synthesis with citations

Citations preserve provider name + URL + title, so callers can audit which
engine surfaced each fact.

## Configuration

Edge config flags (`crates/quarry-edge/src/config.rs`):

| Env var                            | Effect                                |
| ---------------------------------- | -------------------------------------- |
| `QUARRY_EDGE__LOCAL_INDEX_DIR`     | Enables Tantivy slot in router         |
| `QUARRY_EDGE__STRACT_URL`          | Enables Stract slot                    |
| `QUARRY_EDGE__SEARXNG_URL`         | Enables SearXNG slot                   |
| `QUARRY_EDGE__BRAVE_SEARCH_KEY`    | Enables Brave paid backup              |
| `QUARRY_EDGE__SERPER_KEY`          | Enables Serper paid backup             |
| `QUARRY_EDGE__MODEL_PLANE_URL`     | Required for `/v1/answer` synth        |
| `QUARRY_EDGE__MODEL_PLANE_TOKEN`   | Optional bearer for MP                 |
| `QUARRY_EDGE__LLM_CLASSIFY_INTENT` | `true` enables LLM intent classifier (requires Model Plane URL) |
| `QUARRY_EDGE__LLM_CLASSIFY_MODEL`  | Optional override (e.g. `claude-haiku-4-5`) |

Setting none of the search providers leaves `/v1/search` and `/v1/answer`
returning 501 Unsupported with a clear hint in the response body.

## Out of Scope (deferred)

- **Real-time financial news topic** (`topic=finance`)
- **PDF / file extract** beyond what the existing PageRunner already covers
- **Sonic-backed typeahead** — scaffolded as a separate service:
  `apps/Ingestion Plane/autocomplete-core/`. It will consume `SearchIssued`
  + `HostDiscovered` events emitted by Quarry to build its zero-latency
  prefix index.
