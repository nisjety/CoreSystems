---
name: marketing-gsc-analysis
description: Use Marketing Core's verified, read-only Search Console projections to review query opportunities, compare performance drops, content decay, or branded-query patterns for a registered site.
---

# Marketing Core: Search Console analysis

Use the per-site Marketing Core API when it is available. It resolves the
registered website and its verified Search Console property server-side; do
not call Google directly, ask for bearer tokens, or substitute another site's
property. Requests must go through an authenticated Studio owner API or an
approved server-side client. Never expose `X-Internal-API-Key` in a browser or
chat. This skill provides analysis instructions; it does not itself grant an
AI model permission to invoke Marketing Core.

## Available reads

- `GET /api/v1/marketing/search?websiteId={id}&site={origin}&quarter=Q1|Q2|Q3|Q4`
  returns up to 100 query rows, 100 page rows, and deterministic opportunities
  for a completed portion of a quarter. Opportunity rules are review signals,
  not guaranteed traffic gains or automatically created work.
- `GET /api/v1/marketing/search/compare?websiteId={id}&site={origin}&dimension=query|page&currentStart=YYYY-MM-DD&currentEnd=YYYY-MM-DD&previousStart=YYYY-MM-DD&previousEnd=YYYY-MM-DD&rowLimit=1..1000`
  compares adjacent, equal-length windows (maximum 90 days each). `dimension`
  defaults to `query`; `rowLimit` defaults to 500. The route returns observed
  current/previous rows, deltas for rows present in both windows, and
  `not_comparable` rows when one side is absent. It uses finalized GSC data and
  refuses future or incomplete dates.

Use the current registered `websiteId` and its exact canonical HTTPS root
origin together. Do not infer property ownership from DNS verification or
reporting access.

## Workflows

### Opportunities

Use `/search` and report observed queries meeting the API's rules: at least 100
impressions, CTR below 5%, and average position at most 20. The response's
`opportunities[].type` is the assigned review category. Present impressions,
clicks, CTR, position, and suggested review separately from facts. Never
promise a traffic uplift.

### Drops and content decay

Use `/search/compare` twice when needed: `dimension=page` for page decline and
`dimension=query` for query decline. For a rolling 28-day report, select two
adjacent 28-day windows ending at least three days ago when practical. For
content decay, use two adjacent 90-day windows, also ending at least three days
ago. The API validates the range but does not choose dates for you.

Sort comparable rows by `clicksChange` ascending. A negative value is a loss
between the two observed windows, not proof of a cause. For content-decay
candidates, require both `clicksChange < 0` and `impressionsChange < 0`. Treat
position changes as a clue to investigate, not a causal diagnosis. Do not
calculate a percentage when previous clicks are zero; `clicksChangePercent`
will be omitted in that case.

Rows with `state=not_comparable` have no trustworthy delta. Do not coerce the
missing side to zero. If `rowsAtLimit` is true, the response is truncated at
the requested limit. Even below that limit, Search Console can omit anonymized
queries and these dimensional rows are not an exhaustive replacement for
Pulse's aggregate totals. State this coverage limit in every report.

### Branded versus non-branded queries

Before classifying terms, ask the user to confirm the brand terms, product
names, and common variants. Then use query rows and classify them case-
insensitively against only those confirmed terms. Report the classified row
counts and clicks as an observed subset. Do not claim a percentage of total
organic clicks: GSC query rows may be anonymized, omitted, or truncated, and
the aggregate Pulse total is not broken down by the same rows.

## Boundaries and unavailable work

- Search and comparison endpoints are read-only. Never automatically save work
  or edit site content merely because a recommendation was produced.
- On an explicit request to save a specific `/search` opportunity, an authorized
  client can POST `/api/v1/marketing/work` with `websiteId`, exact `site`, current
  `websiteRevision`, `quarter`, and the exact observed `query`. The server
  re-fetches evidence and deduplicates the task. Do not submit invented metrics
  or substitute comparison rows: comparison-to-task saving is not implemented.
  GET `/api/v1/marketing/work?websiteId={id}&site={origin}` reads saved tasks and
  activity. Status changes require the current website and task revisions; on
  `409`, reload instead of silently overriding. These API contracts do not grant
  model execution permission or install an MCP tool. See
  [Marketing Core's work contract](../../../marketing-core/README.md#evidence-backed-marketing-work).
- Marketing Core currently has no scheduled drops/decay monitor, technical
  crawler, sitemap/robots/404 audit, or Google Ads/Meta Ads campaign-management
  API. PageSpeed is an on-demand lab measurement, not a crawler or field CWV
  report. Say when a requested workflow is unavailable; do not simulate it.
- Ads writes, OAuth consent, provider connection setup, and tracking
  installation require separate governed workflows and explicit human approval.
