# T2 / 01 Acquisition API surface: cross-source comparison

64 reference records (Firecrawl 39, Apify 16, Codex in-app browser 6, Aside 3) form 33 capability clusters. All 64 are assigned and none are excluded. They are compared against 44 Verevon baseline records. Clusters file: `01-acquisition-api-surface.clusters.json`.

## Shared baseline (table-stakes)
- **Synchronous single-URL scrape with inline content** (C-001). Firecrawl and Apify have it. Verevon has it **partially**: Quarry returns artifact refs, so getting content needs a second call, and the request has no `formats`, `timeout` or `maxAge`.
- **Scoped site crawl** (C-006). Firecrawl and Apify have it. Verevon has it **partially**: `/v1/crawl` accepts only url, max_pages, max_depth, ingest and zdr. robots.txt is enforced only in map. Crawls are refused under ZDR.
- **Web search API** (C-011). Firecrawl, Aside and Codex have it. Verevon has **shipped** it and is ahead here.
- **MCP exposure and a CLI for external agents** (C-026, C-027). Apify, Aside and Firecrawl have them. Verevon has **neither** (MCP is explicitly deferred).

## Best-of-breed
| Capability | Best source | Why |
|---|---|---|
| Inline sync scrape (C-001) | Firecrawl fc-001 | In-request pipeline under a per-team semaphore. Formats are returned inline. Queue time is reported in metadata. |
| One strict options contract (C-002) | Firecrawl fc-002 / engine-102 | A zod strictObject reused by every endpoint, with cross-field rules. Caution: it silently defaults TLS verification to off. |
| Timeout that resumes instead of failing (C-004) | Firecrawl fc-028 | A 408 response says `processing_continues` and includes Retry-After. The SDK re-attaches to the job, so no work is duplicated. |
| Scoped crawl (C-006) | Firecrawl fc-008 | Include/exclude regex with size caps, sitemap modes, a robots.txt override gated by team flag, and a limit clamped to remaining credits. |
| NL crawl plan + preview (C-007) | Firecrawl fc-009 | An LLM drafts the crawl options, which are re-validated and can be previewed before the crawl starts. This matches Verevon's "Model proposes / Quarry executes" rule. |
| Job lifecycle (C-008) | Firecrawl fc-010/011 | Status with cursor pages capped at 10 MiB, a warnings list, a per-URL errors list (including robots-blocked URLs), a list of ongoing jobs, and cancel. |
| Agent guidance (C-022) | Codex IAB-001 + Firecrawl fc-029 | Codex refuses a command until the model has read its docs. Firecrawl adds bounded, sanitised next-step hints to responses. |
| MCP (C-026) | Apify 045/044 | Hosted Streamable-HTTP MCP server with category tool loading, dynamic discovery and OAuth. Aside shows a credential-safe local variant. |
| Hosted browser sessions (C-029) | Firecrawl fc-006/005 | Two timers (absolute TTL plus inactivity TTL), replay, and a scrape-to-interactive-session bridge. |
| Outbound webhooks (C-019) | Firecrawl fc-025 (contract only) | Event filters and metadata echo. Its delivery is weaker than Quarry's dispatcher. |

## Differentiators
- **Agent-oriented guidance** (C-022). Codex and Firecrawl have it. Verevon has `next_actions` only on error responses.
- **Firecrawl-only edges:** the processing_continues resume contract, NL crawl preview, the Monitor API (multiple targets with check history), the scrape-to-session bridge, and vertical indexes (papers, developer content). Vertical indexes are tier **skip**.

## Verevon's biggest gaps (priority tier)
1. **C-008 Async job lifecycle.** Quarry has no `GET/DELETE /v1/crawl/:id`, no errors route and no result pagination. Its job events stream drains once and closes. Temporal pause/resume/cancel signals already exist but no public route sends them (Ingestion Plane, effort M).
2. **C-006 Scoped crawl.** Needs the scoping vocabulary, robots.txt enforced in the frontier, sitemap seeding, ZDR posture carried into the orchestrator, and a way for service tokens to start crawls under approval (effort L).
3. **C-001 Inline sync scrape.** Add `formats` and an inline mode, plus `timeout` and `maxAge`. This is the natural ZDR path (effort M, depends on the C-002 options contract).
4. **C-017 Structured extraction.** The `prompt` field is dead code. Schema extraction needs a Model Plane format grant that the shipped token manifest denies. Fix: add a json format to ScrapeOptions and grant that scope (effort M).

Recommended next: the options contract (C-002), batch fan-out (C-005), NL crawl preview (C-007), a live job stream (C-009), search with content hydration (C-012), a research-job contract plus a browser-grant fix for production (C-015), monitors (C-018), tenant webhooks (C-019), client idempotency keys (C-020), hints/doc gating (C-022), OpenAPI coverage (C-024), MCP (C-026), and session TTL reaping with a scrape-to-session bridge (C-029).

## Verevon strengths (keep)
- **Search (C-011).** A multi-provider smart router (Tantivy, Stract, SearXNG, Brave, Serper and Data Plane vectors) with ZDR gating, Tavily-style parameters, and image/video/similar/suggest verticals. This beats Firecrawl's self-host fallback chain and the hosted search that Codex and Aside rely on.
- **Error envelope (C-021).** `ApiError` carries `retry_after_seconds` and `next_actions`, and the `ErrorCode` enum includes browser-specific codes: ActionUnknown, ChallengeDetected and TargetRepairRequired.
- **Webhook delivery.** HMAC signing, a backoff schedule, a dead-letter queue and a retry route make it stronger than Firecrawl's single delivery attempt. The missing piece is tenant scoping.
- **Governed browser execution.** One action per call, with receipts and procedure replay checks, is stricter than the code-execution APIs in the reference sources.
- **Batch scraping.** Verevon is ahead of the Codex in-app browser, which declares bulk content fetching but refuses it.
- **Team usage/queue endpoints (C-028).** At parity.

## Caveats
- Every Apify record is DOC (public docs). The Codex export capability (C-030) is DOC-only. Confidence was not raised for these.
- **C-014 (file parse)** and **C-030 (Google Workspace/YouTube export)** have status **unknown**. They need checking in the Data Plane import and in integration-corev2.
- The C-020 idempotency finding and the C-021 envelope location come from a quick read-only grep of quarry-edge (SRC), not from a baseline record. Whether the gateway maps errors consistently to its `{error:{code,...}}` form is INF.
- Prevalence is computed over 4 sources, two of which are browsers rather than scraping APIs. "Table-stakes" is adjusted for that.
- Some baseline facts (the production broker-grant failure, the denied Model Plane grant) are SRC plus high-confidence INF from the baseline records. They were not re-verified at runtime.
