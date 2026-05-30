# Quarry Presets

Phase 5 surfaces business workflows as normalized option bundles on top of the core `/v1` primitives instead of dedicated vertical endpoints.

## `/v1/research`

`POST /v1/research` accepts an optional `preset` field. Presets only provide defaults. Explicit request fields still win.

Supported presets:

- `lead-enrichment`
- `ecommerce-monitor`
- `competitive-monitor`
- `finance-research`
- `content-seeding`
- `data-migration`
- `site-observability`

Current preset behavior:

- selects default search sources
- selects default scrape/analyzer formats
- sets default `limit`
- sets default `timeout`
- sets default `maxIterations` for iterative follow-up research rounds

Example:

```json
{
  "query": "Quarry competitors",
  "preset": "competitive-monitor"
}
```

Resolved defaults currently map roughly as follows:

- `lead-enrichment`: `web` search + `markdown` and `branding`
- `ecommerce-monitor`: `web` and `images` + `markdown`, `branding`, `pageStatus`
- `competitive-monitor`: `web` and `news` + `markdown`, `seo`, `pageStatus`
- `finance-research`: `web` and `news` + `markdown`, `seo`, `pageStatus`
- `content-seeding`: `web` and `news` + `markdown`, `seo`
- `data-migration`: `web` + `markdown`, `html`, `links`
- `site-observability`: `web` + `markdown`, `seo`, `wcag`, `pageStatus`

## Iterative Research

When `maxIterations` is greater than `1`, `research` now runs additional follow-up search rounds. Follow-up queries are derived from:

- preset focus terms
- extracted source content, titles, and snippets

The status payload includes:

- `steps`
- `sources`
- `report`
- `data.preset`
- `data.iterations`
- `data.maxIterations`
- `data.queryPlan`

## `/v1/crawl`

`POST /v1/crawl` now also accepts `preset`.

Current crawl presets:

- `site-observability`
- `competitive-monitor`
- `ecommerce-monitor`
- `data-migration`

`site-observability` is the main Phase 5 workflow preset. It currently:

- defaults crawl scope to the broader site
- adds `markdown`, `seo`, `wcag`, and `pageStatus` outputs
- enables page-level `changeTracking`
- supports delayed execution through `scheduleAt`
- enriches crawl completion webhooks with:
  - `changes`
  - `pageStatus`
  - `httpStatus`
  - `alerts`
  - `errors`
  - `robotsBlocked`

Example:

```json
{
  "url": "https://example.com",
  "preset": "site-observability",
  "scheduleAt": "2026-04-03T08:00:00Z"
}
```

The create response includes resolved crawl options, including:

- `preset`
- `scheduleAt`
- `maxDiscoveryDepth`
- `scrapeOptions.formats`
- `changeTracking`

## `/v1/search`

`POST /v1/search` now also accepts `preset`.

Current search presets:

- `lead-enrichment`
- `ecommerce-monitor`
- `competitive-monitor`
- `finance-research`
- `content-seeding`
- `data-migration`
- `site-observability`

Preset behavior:

- defaults source selection when `sources` is omitted
- enables scraping automatically when no explicit scrape settings are supplied
- selects default scrape/analyzer formats
- sets default `limit`
- sets default `timeout`
- defaults `blendMode` to `ranked`

Supported explicit source families on `/v1/search` and `/v1/research`:

- `web`
- `news`
- `images`
- `github`
- `documents`
- `index`

Each source object can also include:

- `weight` for ranked blending
- `limit` for per-source caps

`blendMode` currently supports:

- `ranked`
- `interleave`

Example:

```json
{
  "query": "Quarry founders",
  "preset": "lead-enrichment"
}
```

## `/v1/extract`

`POST /v1/extract` now also accepts `preset`.

Preset behavior:

- sets default `limit`
- sets default `timeout`
- enables prompt-driven web search for workflows like `lead-enrichment`
- selects default scrape/analyzer formats
- carries the normalized preset through status payloads

Example:

```json
{
  "prompt": "Find Quarry contacts and leadership",
  "preset": "lead-enrichment"
}
```

## Go Client Helpers

Phase 5 now includes a minimal public helper package at `pkg/client`.

It exposes:

- typed async create methods for `crawl`, `search`, `extract`, and `research`
- preset helper constructors:
  - `SiteObservabilityCrawl`
  - `LeadEnrichmentSearch`
  - `LeadEnrichmentExtract`
  - `CompetitiveMonitorResearch`

Example:

```go
cli := client.New("https://quarry.internal", "api-key")
resp, err := cli.StartCrawl(ctx, client.SiteObservabilityCrawl("https://example.com", nil))
```
