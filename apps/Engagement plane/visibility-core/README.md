# Visibility Core

Visibility is the SEO/SEM search workspace in Engagement Plane. See
[Engagement ownership and migration](../README.md) for the current boundaries,
canonical route and deployment compatibility. Older sections below retain API
and environment names for existing callers.

## Visibility Explorer (current)

`/engagement/visibility` retains the source overview and adds four summary links.
The dedicated Synlighet sidebar opens Sider (`/pages`), SEO-grupper (`/groups`),
Interne lenker (`/links`) and Konkurrenter (`/competitors`). Both Synlighet and
Anbud have a back link to the shared `/engagement` landing page.

- `GET /api/v1/visibility/analysis?websiteId=...&site=...&quarter=Q4` joins saved,
  permission-checked Search Console page/query rows with discovery and the latest
  bounded audit. It does not trigger provider collection. Exact root/www aliases
  are joined without merging different paths or arbitrary subdomains. Unmeasured
  metrics remain null; site totals are never distributed across pages.
- Four deterministic groups identify query rows near page one or with low CTR,
  pages with technical review signals, and pages with few observed incoming
  links. The UI states the rule and partial coverage for each group.
- Link analysis counts only captured links from checked pages (at most 100 per
  page). Only an observed 404/410 proves a broken target. Unchecked targets and
  missing paths are explicit; neither proves an orphan page.
- `GET/POST /api/v1/visibility/competitors` reads or queues up to three explicit
  public-homepage comparisons per registered site. Commands require the current
  website revision and `refresh` or `remove`. Quarry guards every public fetch;
  no competitor account data, rankings, search volumes or traffic are inferred.
  A single durable worker refreshes daily, backs off failed attempts for an hour,
  and retains the previous successful evidence. Its private atomic repository is
  `competitors.json` beside the existing audit store and supports one writer.

All views use the authenticated gateway's organization and role checks. Browser
requests carry session cookies, never provider tokens or organization headers.
The analysis and comparison DTOs are rebuilt by the gateway and browser client;
Quarry HTML and private job IDs are excluded from browser responses. Website
revision changes invalidate queued and displayed evidence.

Coverage is deliberately bounded: the saved GSC row sample, 25 discovered pages,
and 10 audited pages do not constitute a complete crawl or Semrush-scale index.
Run discovery/audits from the overview to refresh public evidence. Google Ads,
competitor traffic estimates and paid keyword databases are not supplied by this
slice.

## Pulse connections and onboarding

The frontend observes registry metadata every 15 seconds during a three-minute
handoff window, renewed when the window regains focus. Unchanged records do not
remount reports or interrupt Sync. These reads never start provider collection.

Verevon's existing onboarding registers the URL through the same website API as
Pulse. Website creation queues public preparation, initial analytics and public
signals. The connection watcher rechecks mappings and schedules collection after
the organization's provider grants change. Opening reports remains read-only.

Pulse retains its primary chart and summary cards, with a connected-services
selector for the lower reports. Unconnected services live in the `+` extension
dialog; existing connections requiring renewal stay visible. The `sources`
catalogue checks current Integration metadata independently of report freshness:
Clarity bindings, Algolia mappings, and Cookiebot/Web Vitals installation metadata.
No provider analytics are fetched by this catalogue.

Header Sync composes the existing scoped preparation, public-signals, analytics
and configured provider collection actions. Jobs retain their own receipts, quotas
and schedules; a failed provider does not cancel the others. Switching websites
aborts browser tracking, not durable server jobs. The extension setup check can
collect already authorized Cookiebot and Web Vitals reports. Google consent
completion verifies mappings automatically; Algolia retains its discovery and
collection queue. Clarity/Cookiebot credentials and tracking installations still
require setup; a project ID or URL alone does not create them.

`marketing-core` owns the organization-scoped website registry and per-site integration
mappings, alongside read-only Pulse, search-intelligence, Search Console period
comparisons, GA4, Clarity, and PageSpeed projections. It also owns the persistent,
evidence-backed per-site marketing work queue.
Its static status HTML does not contain private analytics.

It does not store provider credentials, call Google directly, create provider
projects, or install tracking tags. Integration Core owns OAuth sessions,
encrypted grants, and fixed provider actions. Marketing can request an explicit
Google consent session through that boundary; completing consent is a human action.
Pulse calls Integration Core's fixed `gsc.sites.list` and
`gsc.search_analytics.query` actions using the selected website's server-discovered
Google connection and one approved org-scoped service bearer token file. It
only queries a property returned by `sites.list` with verified read access
that exactly matches the selected website host. A parent domain property is
not used for a selected subdomain because its date-only metrics would include
other hosts. Marketing Core never requests the submitted website URL.

## Capability status for the marketing MVP

| Workflow | Current support | Important boundary |
| --- | --- | --- |
| Search Console Pulse, query/page opportunities, and period comparisons | Read-only, per registered site | GSC dimensions can be truncated or privacy-filtered; no automatic monitoring |
| Marketing work queue | Persisted per-site GSC and technical tasks, status, activity, and recheck evidence | Explicit authoritative finding saves; newer technical reports do not automatically complete work |
| GA4 aggregate reporting | Read-only through Integration Core | Requires consent and an accessible mapped property/stream |
| Clarity aggregate reports | Read-only through Integration Core | Requires a separately authorized project export; no recordings or user-level replay data |
| PageSpeed Insights | Durable daily mobile/desktop lab reports, with manual refresh and bounded SEO checks | Homepage lab evidence only; separate from CrUX and site-wide audits |
| Bounded public page audit | Quarry-backed discovery and up to 10 captured pages; HTTP, title/description, duplicate-title, canonical-review and checked broken-link signals | Partial sample; not a complete crawl, heading inventory or authoritative index-status report |
| Page Explorer, SEO groups and link analysis | Saved Search Console, discovery and audit evidence joined per selected website | Missing values remain unknown; rule-based groups and observed graph are limited to available coverage |
| Public competitor comparisons | Up to three saved homepage comparisons, refreshed daily through Quarry | Public metadata only; no private analytics or competitor traffic/ranking estimates |
| Google Ads and Meta Ads reporting/campaign management | Not implemented as Marketing Core APIs | Do not claim an ad-management connection; provider actions and account mappings must be added behind Integration Core first |
| AI use | Same owner API is suitable for a future governed tool adapter | REST availability does not itself register Model Plane tools or grant model execution permission |

The project-local `marketing-gsc-analysis` skill documents how agents should
use the supported GSC reads and how to report their limitations. It is not a
substitute for an authenticated tool binding.

Studio Pulse now exposes the comparison API through a bounded local same-origin
bridge and an explicit analysis panel: 28-day query changes and 90-day page
decline candidates. The multi-site workspace adapter resolves the selected
website's Google mapping for these requests, just as it does for Pulse/search.
No comparisons are scheduled. Saving a suggestion from Search intelligence is a
separate, explicit operation; running a comparison never creates tasks.

See [the unified marketing system roadmap](docs/UNIFIED_MARKETING_SYSTEM.md) for
the product scope, ownership boundaries, and remaining implementation slices.

## Website registry and ownership

Enable the registry with `MARKETING_ORGANIZATION_ID` and the absolute
`MARKETING_WEBSITES_FILE` deployment path. The live Studio overlay mounts a
private, persistent Docker volume there. `MARKETING_INITIAL_SITE` seeds only an
empty registry; browser storage is not the source of truth. This file-backed
implementation supports one writer/API replica. See the Engagement Plane README for
deployment and the complete endpoint contract.

- `GET/POST /api/v1/marketing/websites` lists or adds canonical public HTTPS root origins.
- `GET/PATCH /api/v1/marketing/websites/{id}` reads or changes a website; writes require its current revision.
- `POST /api/v1/marketing/websites/{id}/verify` rediscovers accessible Search Console and GA4 resources.
- `PATCH /api/v1/marketing/websites/{id}/mappings` saves a Clarity project ID as a pending, unverified hint.
- `POST /api/v1/marketing/websites/{id}/google/connect` and `GET .../google/status`
  manage an explicit read-only consent session through Integration Core.
- `POST /api/v1/marketing/websites/{id}/ownership/challenge` and `POST .../ownership/verify`
  are retired (`410 Gone`). Historical ownership/DNS records remain readable;
  Studio no longer issues or checks its own DNS challenge.

These endpoints use the same internal-key boundary as the projections. The fixed
organization must match the Integration bearer. Public DTOs omit private provider
connection IDs and credentials. Authorized human-facing clients and AI adapters
can use the same HTTP contract; this does not automatically register Model Plane tools.

Pass `websiteId` plus the matching `site` on Pulse, search, and PageSpeed requests.
Legacy site-only requests must also resolve to a registered website when the
registry is enabled. Changing a URL keeps its stable ID, clears ownership and all
provider mappings, and then rediscovers access for the new origin. Revision
conflicts are explicit; another site's metrics are never used as fallback.

## Evidence-backed marketing work

With the website registry enabled, Marketing Core stores `work.json` beside
`MARKETING_WEBSITES_FILE` in the existing private persistent volume. The directory
must be mode `0700`, the file mode `0600`. Atomic replacement and file/directory
sync protect updates; this implementation supports one API writer/replica, not
multi-replica shared-file access. Back up both registry and work files together.

- `GET /api/v1/marketing/work?websiteId=...&site=...` returns tasks, recent activity,
  and computed summary for exactly that registered ID/origin.
- `POST /api/v1/marketing/work` accepts only `websiteId`, `site`,
  `websiteRevision`, `quarter`, and `query`. Marketing re-reads the authoritative
  GSC opportunities and snapshots their observed metrics, period, and timestamp.
  Client-supplied analytics are rejected. Saves deduplicate by website, origin,
  query, and opportunity type; an existing task returns `200`, a new task `201`.
- `PATCH /api/v1/marketing/work/{taskId}` accepts `websiteId`, `site`,
  `websiteRevision`, task `revision`, and `status` (`todo`, `in_progress`, `done`,
  `dismissed`). Stale revisions return `409`; unchanged status creates no event.
- `POST /api/v1/marketing/work/technical` accepts exactly `websiteId`, `site`,
  `websiteRevision`, `strategy`, `checkId`, and `fetchTime`. Marketing re-reads
  the completed PageSpeed report through Integration Core. The selected report
  time must match, and only `failed`, `manual`, or `error` checks can become tasks.
  Technical evidence is a separate `pagespeed_check` variant, without GSC zeros.
- `POST /api/v1/marketing/work/{taskId}/verify` accepts exactly `websiteId`,
  `site`, `websiteRevision`, and task `revision`. Marketing reads the latest
  completed report for the original check/device. Its observation must be
  strictly newer than the original and any previous verification. A repeated,
  stale, or future observation is rejected; this endpoint never runs PageSpeed.

Responses use `{ data: { websiteId, site, tasks, activity, summary } }`. The same
internal-key boundary protects every route, with separate read/write rate limits.
Up to 500 tasks and the latest 1,000 activity entries are retained per deployment.
The local activity feed is not a replacement for Control Plane security audit.
URL changes hide old-origin tasks instead of reassigning their evidence.

Completion is completed tasks / non-dismissed tasks, not traffic growth or a
provider health score. Empty and dismissed-only queues return null completion.
Closed tasks can be shown and reopened in Studio. Scheduling, task deletion,
general-purpose editable tasks, and Model Plane/MCP tool registration remain
separate future workflows. Provider accounts, websites, and ad spend are not
modified by any work-queue operation.

Technical tasks preserve their original check status, binary/null score,
measurement time, device, and Lighthouse version. Rechecks add a separate latest
`verification` and a `verified` activity receipt with the observed result and
recording time. All seven check states are preserved. A passing check never
automatically completes the task; a failed check never silently reopens it.
Manual completion remains an explicit user decision. Initial technical saves
deduplicate by website/origin/kind/device/check, independently of GSC tasks.

The additive evidence variant remains in the protected version-1 work file;
existing GSC records are retained. Older binaries do not understand technical
records: keep a protected pre-upgrade backup before rolling back. The local
deployment keeps `work.pre-technical-2026-10-22.json` beside the work file.

## Technical page diagnostics

The existing PageSpeed run/read contract now includes optional `report.checks`
with `{ id, status, score }` entries. Integration Core projects only 11 fixed
Lighthouse audit IDs: `document-title`, `meta-description`, `http-status-code`,
`is-crawlable`, `robots-txt`, `canonical`, `hreflang`, `link-text`,
`crawlable-anchors`, `image-alt`, and `structured-data`. Missing checks remain
`not_reported`; legacy cached reports without checks remain readable.

Only Lighthouse binary score `1`/`0` maps to `passed`/`failed`. `manual`,
`not_applicable`, `informative`, `error`, and `not_reported` keep null scores;
they are not passes or failures. The manual structured-data check is not an
automated schema-validation result. Marketing validates unique IDs, the bounded
list, and every status/score pair. Provider descriptions, HTML, node details,
screenshots, URLs, and error stacks are never forwarded as check content.

Studio shows localized explanations and review guidance using its own fixed
labels. Results apply to the requested origin's navigation (which can redirect),
device, and observed timestamp, not the selected quarter or the entire website.
Robots syntax and page crawlability are Lighthouse observations, not proof of
Google indexing or bot-specific access. Sitemap traversal, broken-link crawling,
headings inventory, and complete historical audit storage are not implemented
by this increment. Explicit technical tasks retain only their selected original
check and recheck receipts, not whole Lighthouse reports. Google performs the measurement
through Integration Core; Marketing Core does not fetch arbitrary website URLs.

The existing one-hour in-memory report expiry, explicit-run requirement,
provider quota limits, tenant scoping, and no-raw-response retention are unchanged.
Reads and panel mounting never initiate a new audit.

Contract sources: [Lighthouse SEO audit IDs](https://github.com/GoogleChrome/lighthouse/blob/main/core/config/default-config.js)
and [Lighthouse audit result modes](https://github.com/GoogleChrome/lighthouse/blob/main/types/lhr/audit-result.d.ts).

## GA4 and Clarity reporting

`GET /api/v1/marketing/ga4?websiteId=...&site=...&quarter=Q4` returns fixed
read-only GA4 aggregate totals and daily rows through Integration Core. The
private registry mapping selects the connection, property, and stream; Integration
rechecks the web stream origin and forces exact hostname/stream filters. Period
users are queried independently, never summed from daily users. The projection
includes property timezone and bounded quality flags. Empty reports retain null
totals rather than manufacturing zeros.

`GET /api/v1/marketing/clarity?websiteId=...&site=...` reads an operator-bound
Clarity project export through Integration Core. Its fixed rolling 72-hour UTC
window is independent of the quarter selector. It returns aggregate Traffic
metrics and optional `metrics.ux`: average scroll depth, average active/total
seconds per session, and percentages of sessions with dead clicks, rage clicks,
quick backs, excessive scrolling, script errors, or error clicks. Missing
measurements remain null; a measured zero remains zero. Historical snapshots
without UX remain readable. Recordings, heatmap images and raw browsing data are
not returned. Pulse links to the mapped project's authenticated recordings and
heatmaps views; access there remains controlled by Microsoft Clarity.
Credentials, fixed provider egress,
cache, and quota handling belong exclusively to Integration Core. See the Studio
Plane README for the opt-in protected secret mounts and activation prerequisites.

Both routes require a current registered website ID and canonical matching
origin, validate all query fields, reject writes, apply read limits, and return
explicit unavailable reasons with null metrics. Google connection success does
not imply that the account can access an existing GA4 property. A pending Clarity
project ID is not an authorized export credential.

Google reporting access is not ownership. Search Console `siteOwner` remains
provenance metadata; `siteFullUser` still permits authorized reporting. Provider
reports and mappings do not require a separate Studio DNS/Google-owner gate.
They require the current registered site/revision and the provider's own active
account/resource authorization. GA4 alternate-host reporting is limited to the
registered root/www pair and revalidates the same connected property/stream
before changing the filter. This grants no provider write permission. GA4 discovery
needs `analytics.readonly` consent and the Analytics Admin API, while Clarity
requires separate project authorization. Automatic Search Console/GA4/Clarity
creation and tracking installation are not implemented by these read-only flows.

## Local endpoints

- `GET /` — public static status HTML
- `GET /health` and `GET /ready` — public JSON health
- `GET /api/v1/marketing/status` — internal-key protected JSON status
- `GET /api/v1/marketing/pulse?site=https%3A%2F%2Faquatiq.com%2F&quarter=Q1|Q2|Q3|Q4`
  — internal-key protected daily GSC clicks and impressions for a current-year
  quarter; quarter defaults to the current quarter. `site` is required and
  accepts only a public HTTPS root origin, never a page path, IP, localhost,
  port, credentials, query, or fragment. Current-quarter data ends yesterday
  (UTC). Unavailable source state is explicit and never represented by invented
  zero metrics.

Set `INTERNAL_API_KEY` and optionally `PORT` before starting the service.
The shared Integration URL may be configured before authorization; Pulse then
returns `not_configured`. To enable live reads, also set the connection ID and bearer file below. The bearer token
must be a secret-manager-mounted file, not an environment variable value or a
checked-in `.env` entry:

```text
INTEGRATION_CORE_URL=http://integration-core:3026
GOOGLE_CONNECTION_ID=<approved Google connection ID>
GOOGLE_GSC_PROPERTY=<optional preferred verified property, or leave unset>
INTEGRATION_BEARER_TOKEN_FILE=/run/secrets/integration-bearer-token
```

Pulse's response is `{ "data": { "site": "https://aquatiq.com/", "source":
"google_search_console", "status": "connected" | "unavailable", "period":
{ "startDate": "YYYY-MM-DD", "endDate": "YYYY-MM-DD" }, "totals":
{ "clicks": 0, "impressions": 0 } | null, "days": [], "propertyScope":
"domain" | "url_prefix" } }`. A verified,
successful empty Search Console query returns measured zeros; unconfigured or
failed sources return `totals: null`, `days: []`, and a reason code. The route
never returns the provider response, OAuth tokens, or connection ID. It must
only be exposed through an authenticated same-origin frontend gateway; the
static Marketing Core page remains public but has no analytics data.

`GOOGLE_GSC_PROPERTY` is a backward-compatible preference when that property
is present in the connected account and matches the selected site. It no
longer restricts the site selector to aquatiq.com. The response's
`propertyScope` lets the UI distinguish domain-wide from URL-prefix metrics.

For the existing loopback Docker stack, `apps/Engagement plane/docker-compose.marketing-pulse.local.yml`
is an optional overlay that mounts the already-approved org-scoped bearer as
a Docker secret and sets the verified Google connection ID. The preferred GSC
property is optional; each selected site must still match a verified property.
It does not obtain a token or make the unauthenticated Studio demo
an analytics gateway. Without those inputs, Pulse returns `not_configured`.

For the unified Engagement Plane stack, use
`apps/Engagement plane/docker-compose.marketing-gsc.studio.yml` instead. That
overlay attaches Marketing to `inter-plane-bus` and uses
`http://integration-api:3026`, the Ingestion Plane service name. It requires
`STUDIO_MARKETING_GOOGLE_CONNECTION_ID` and the approved
`STUDIO_MARKETING_INTEGRATION_BEARER_FILE` path; the bearer content stays in
the mounted file. Social Core is a separate, organization-scoped source for
publishing and provider metrics, not a substitute for GSC's site-scoped
search data.

The local managed alternative uses `docker-compose.marketing-bearer.studio.yml`
and `docker-compose.marketing-live.studio.yml`. A protected
`.local/marketing-integration.json` manifest lets the Studio launcher select
both automatically. The separate `integration-bearer-refresh` executable uses
Marketing's fixed-organization Auth Core service principal to renew a
five-minute `ingestion`/`integration:read` bearer. The API mounts the token
volume read-only and never receives the issuing service key. Both token and
expiry metadata are written atomically with mode `0600`; renewal failure fails
readiness and never turns an expired token into a working connection. Google
OAuth tokens remain encrypted in Integration Core's persistent database.

Refresher configuration (server-side file paths, never token values):

```text
AUTH_CORE_URL=http://auth-core:3011
MARKETING_INTEGRATION_ORGANIZATION_ID=<approved organization ID>
MARKETING_INTEGRATION_SERVICE_KEY_FILE=/run/secrets/marketing_plane_service_key
INTEGRATION_BEARER_TOKEN_FILE=/run/marketing-bearer/token
```

`integration-bearer-refresh --once` performs one audited renewal;
`--check` verifies local token binding and remaining lifetime without printing
the token. Integration Core independently verifies the JWT signature and
organization on every request.

## Search intelligence

`GET /api/v1/marketing/search?site=https%3A%2F%2Faquatiq.com%2F&quarter=Q4`
returns a second, independently loaded read-only projection. It uses the same
internal API key, organization bearer, and exact verified-property selection as
Pulse. After verification, two fixed Integration Core actions request the top
100 query rows and top 100 page rows, with finalized Search Console data. The
whole operation has an eight-second deadline; a failed dimension produces an
explicit unavailable state instead of a partial report.

The `{ data }` envelope contains `site`, `source`, `status`, `period`, `queries`,
`pages`, `opportunities`, `rowLimit`, `queriesAtLimit`, `pagesAtLimit`, and, when
available, `propertyScope` and `updatedAt`. Rows contain only `key`, `clicks`,
`impressions`, `ctr` (a ratio from 0 to 1), and `position` (the reported average).
Opportunities additionally have an allowlisted `type`. Neither raw provider
payloads nor connection IDs or tokens cross this boundary. The endpoint can be
used by authorized human-facing clients or AI tools without an MCP server.

Suggestions are deterministic review heuristics over the returned query rows:
at least 100 impressions, CTR below 5%, and average position at most 20.
Positions up to 3 with CTR below 3% suggest snippet review; positions above 3
through 10 suggest title/meta review below 3% CTR, otherwise title testing;
positions above 10 through 20 suggest content review/expansion. They are not
saved tasks, automated edits, causal diagnoses, or forecasts of additional
traffic. An empty suggestions array is a valid result.

Search Console's API returns top rows, not an exhaustive keyword inventory.
Do not sum these grouped rows to replace Pulse's overall totals, or interpret
`queriesAtLimit: false` as proof that all queries are present. Google uses its
Pacific reporting calendar for these date strings. See the official
[Search Analytics query contract](https://developers.google.com/webmaster-tools/v1/searchanalytics/query).

### Period comparisons for drops and decay

`GET /api/v1/marketing/search/compare?websiteId=...&site=...&dimension=page&currentStart=YYYY-MM-DD&currentEnd=YYYY-MM-DD&previousStart=YYYY-MM-DD&previousEnd=YYYY-MM-DD&rowLimit=500`
provides read-only adjacent-window comparisons for the `query` or `page`
dimension. The two windows must be contiguous, equal length, completed, and no
longer than 90 days each. The default dimension is `query`; row limit defaults
to 500 and is capped at 1,000. The response is `{ data: { site, source, status,
dimension, currentPeriod, previousPeriod, rows, rowLimit, rowsAtLimit,
comparableRows, unmatchedRows, propertyScope, updatedAt } }`.

Each row exposes its observed `current` and `previous` records. Deltas are
returned only for keys present in both periods. A key present in one period
only has `state: not_comparable` and no inferred zero or delta. Sort comparable
`clicksChange` values ascending to review declines; content-decay candidates
must also have negative `impressionsChange`. `clicksChangePercent` is omitted
when previous clicks are zero. This is a bounded top-row comparison, not an
exhaustive lost-query inventory; `rowsAtLimit` and Google's privacy filtering
must be disclosed in reports. Use windows ending at least three days ago when
possible to reduce reporting-lag issues.

The project-local `.agents/skills/marketing-gsc-analysis/SKILL.md` maps the
opportunity, drops, decay, and brand-query workflows to these read-only
projections. It is an instruction guide, not an AI permission grant. Model
Plane action registration and owner authorization are still required before
an AI model can invoke these endpoints directly.

## PageSpeed lab diagnostics

`GET /api/v1/marketing/pagespeed?site=https%3A%2F%2Faquatiq.com%2F&strategy=mobile`
reads a cached audit status. `POST /api/v1/marketing/pagespeed` accepts the same
`site` and `strategy` as a strict JSON object and explicitly starts an
asynchronous public-page measurement. Both require the internal Marketing key;
the local BFF injects it server-side and never exposes it to browser code.
Strategies are `mobile` and `desktop`. Only canonical public HTTPS root origins
are accepted in this first slice; adding a site does not trigger a crawl.

This source needs only a validated Integration URL and an approved org-scoped
read bearer. It does not depend on a Google connection ID or Search Console
ownership. Private GSC projections keep their existing verified-property rules.
Marketing never calls Google or the submitted website directly: Integration
Core owns the fixed provider endpoint and its optional file-mounted PageSpeed
API key. See the Engagement Plane launcher instructions for that secret mount.

The typed projection contains `site`, `source: google_pagespeed`, `strategy`,
`status: idle|running|complete|failed`, optional timestamps/reason, and a compact
nullable Lighthouse lab report. Provider quota errors are explicit. Results are
point-in-time lab measurements, not quarter metrics or field Core Web Vitals;
TBT is not INP. Reads and polls never start audits. Integration's bounded cache
is ephemeral and expires after one hour or process restart.

## CrUX reporting-origin discovery

Adding a website enrolls public-signal collection through Integration Core.
For each CrUX device report, Integration checks the entered HTTPS origin first.
Only when coverage is missing does it try the exact leading-www counterpart,
in either direction, with at most two requests per device. The successful
provider-validated `reportingOrigin` travels with the saved report and is shown
in Pulse. Device reports are never merged across origins. Quota, configuration,
timeout and invalid-response failures do not trigger fallback requests.

The registered website and revision stay unchanged. Older snapshots without `reportingOrigin` remain
readable as legacy exact-origin reports. New daily checks rediscover coverage;
opening Pulse reads the existing snapshot and does not call Google.

Common Crawl uses the same exact pair, with serialized paced queries and a
bounded per-host timeout. Missing data and host-query failures can trigger the
counterpart; throttles and invalid capture payloads cannot. A successful archive
includes `reportingHost`, and every returned capture must match that host. No
archive bodies are downloaded, and the retained record limit remains 20.
The canonical index may return the exact www counterpart in the first query.
Those captures are validated against their actual host. If both hosts appear,
the requested host is preferred and the omitted counterpart records are
disclosed as truncation; unrelated subdomains remain rejected.

Google discovery uses only resources visible to the connected account. Search
Console accepts an exact prefix counterpart or the covering root domain and
records `reportingProperty` with its original domain/prefix scope. GA4 discovery
requires one unambiguous property/stream and uses Integration's
`ga4.reporting_host.resolve` action to check sessions for the two exact hosts in
that stream over the last 28 completed UTC days. It prefers the entered host if
it has sessions, then its counterpart, then the verified stream default URI.
Private counts stay inside Integration. Manual reporting-host choices remain
available; normal reports revalidate the accessible stream and exact host filter.

An authorized Clarity project binding can serve its same-organization www alias;
the report remains project-wide. Cookiebot tries its paired domain only when both
configured domain resources are absent, with the same domain-group credential.
PageSpeed follows the website redirect through Google and records the measured
`reportingOrigin`; Quarry already permits discovery within the exact pair.
Algolia remains index-scoped, and first-party Web Vitals remains bound to its
explicit installation/export origin. Host discovery does not create credentials,
tracking installations, account grants or missing measurements.

## Local Docker verification

The Engagement Plane owns the isolated two-service Docker harness. It creates
ephemeral secrets in the process, validates both HTML pages and health
endpoints, and removes its containers afterward:

```sh
cd "apps/Engagement plane"
./scripts/smoke-marketing-local.sh
```

For a live, read-only provider check after OAuth connections exist, mount a
verified org-scoped bearer from your approved Auth Core flow and pass its file
path. The command accepts either Google or Meta, or both:

```sh
export INTEGRATION_CORE_URL=http://localhost:3026
export INTEGRATION_BEARER_TOKEN_FILE=/absolute/approved/secret-file
export GOOGLE_CONNECTION_ID=...
"apps/Engagement plane/scripts/test-provider-readiness.sh"
```

The live command reads only connection readiness and never creates ads,
campaigns, or Search Console changes. It accepts HTTPS endpoints, with an
HTTP exception only for loopback development URLs.

## Cookiebot and Algolia read-only site sources

### Algolia OAuth follow-on implementation contract (23 October 2026)

Algolia's advertised MCP dynamic registration accepted an Integration-owned
public client (HTTP 201) for the fixed local callback
`http://localhost:3026/api/v1/providers/algolia/callback`. Its protected client
registration is configured only in Integration Core with
`ALGOLIA_MCP_OAUTH_CLIENT_FILE`; it contains `version:1`, `clientId`,
`redirectUri`, and `resource:https://mcp.algolia.com/mcp`. No CLI identity is
borrowed, no client secret is sent, and scope is only `public`. OAuth uses PKCE
S256 and resource indicators on authorization/token/refresh; the resulting
bearer is used only against the fixed Productivity MCP endpoint, not REST.
The user must enable Productivity MCP in Algolia and grant consent. Provider
access is read-only but account-wide within that user's permissions; Studio
limits actual calls to fixed read-only application/index metadata, aggregate
search analytics and the two bounded search-term shortlist tools.

Shared cross-plane contracts for this follow-on:

- Existing Integration provider `algolia` connect-session/status/callback and
  encrypted OAuth storage gain a custom public-client MCP adapter. Do not
  overwrite the latest organization account if provider identity is absent:
  preserve independently identified grants. Generic token-leasing routes must
  not expose this resource-bound token to downstream cores. Local disconnect
  must hide access immediately; do not claim provider-side revocation until its
  actual endpoint contract is established.
- Integration user endpoints (verified user identity, organization permissions;
  administrator and same-origin enforcement additionally at Verevon):
  `GET /api/v1/providers/algolia/connections/:id/applications` returns
  `{success:true,data:{applications:[{id}]}}` from the fixed authenticated
  application-list operation (legacy direct-tool schemas may supply an enum).
  `GET .../:id/indices?applicationId=...` returns
  `{success:true,data:{indices:[{name}]}}`. Never forward tool descriptions,
  search records, raw schema, unbounded query logs or tokens. Wrong org/inactive grant
  fails closed. MCP tools are fixed allowlisted calls, never arbitrary dispatch.
- Integration internal read endpoints use a verified org-scoped bearer with
  `integration:read`, not caller headers/internal-key scope bypass:
  `POST /api/v1/insights/algolia/access` exact `{connectionId}` returns exactly
  `{success:true,data:{connectionId,status,generation}}`; status is
  `active|reconnect_required`, generation is a bounded opaque credential/grant
  version (no token hash). It makes no provider call. Wrong-org connection is
  denied. `POST /api/v1/insights/algolia/report` accepts exact
  `{websiteId,site,websiteRevision,connectionId,applicationId,indexName}` and
  returns the existing minimal site-source Report envelope. Region is
  `provider` for MCP-managed routing (not a guessed US/DE region). Revalidate
  connection/org/generation before and after collection. Exact RPC schemas
  must be checked against authenticated `tools/list`; unsupported schemas yield
  unavailable, never guessed/invented analytics.
- Marketing stores a separate private site/revision-scoped mapping, not keys:
  `GET /api/v1/marketing/site-sources/algolia-mapping?websiteId=...&site=...`
  returns `{data:{websiteId,site,websiteRevision,status,connectionId?,applicationId?,indexName?}}`;
  status is `not_mapped|mapped|reconnect_required|access_check_required`.
  `PUT` same path accepts exact identity plus `connectionId,applicationId,indexName`;
  `DELETE` accepts exact `{websiteId,site,websiteRevision}` and disconnects the
  site mapping only (not the whole account grant). Writes require current
  registered website revision. Mapping validation checks the real allowed app/index
  through Integration, not URL-based guesses. Mapping does not mutate website
  revision or unrelated Google/Clarity settings.
- Existing Marketing site-source GET/sync prefers the explicit OAuth mapping
  for Algolia, validates live Integration access on cached reads, and privately
  binds snapshots to both mapping fingerprint and access generation. A changed
  or disconnected grant must hide old snapshots immediately. A mapping tombstone
  prevents fallback to legacy file credentials after site disconnect. Never
  pass API keys/tokens through Marketing or the browser. Preserve existing file
  fallback only for sites with no OAuth mapping/tombstone. Report DTO remains
  backward compatible, with optional bounded `topSearches` and `noResultTerms`
  observations and Algolia `region:"provider"`.
- Verevon adds guarded BFF routes mirroring the two user discovery endpoints
  under `/api/v1/integrations/algolia/connections/:id/...`, uses its existing
  user `ingestion` audience token, never a fabricated org header, and projects
  allowlisted metadata. Mapping routes use Marketing's existing internal-key
  boundary with current canonical org/admin and exact same-origin write guards.
  The UI offers explicit sign-in, grant selection, application/index selection,
  save mapping, local site disconnect, and aggregate fetch. No API-key fields
  are required for the Algolia OAuth path. Cookiebot remains honestly labelled
  Data API-key onboarding; no public user-OAuth flow has been verified.

Authenticated tool schemas and real analytics remain a human-consent gate.
Do not label the provider connected or display numbers until those actual
checks succeed. Do not add third-party tracking or create/change applications,
indices, keys, subscriptions or billing as part of connection setup.

This slice connects existing provider installations; it does not install a CMP,
change consent, install site search, or write an Algolia index. Integration Core
owns credentials and fixed upstream endpoints. Marketing owns validated,
site/revision-scoped aggregate snapshots. Pulse uses the same backend contracts.
These manual API-key providers are not represented as OAuth connections.

Shared implementation contract (v1):

- Integration: `POST /api/v1/insights/site-sources` with the exact JSON body
  `{websiteId, site, websiteRevision, provider}` (`cookiebot` or `algolia`).
  Requires a verified organization bearer with `integration:read`.
  Response is exactly `{success:true,data:Report}`.
- Marketing: `GET /api/v1/marketing/site-sources?websiteId=...&site=...&provider=...`
  reads a snapshot; `POST /api/v1/marketing/site-sources/sync` accepts the same
  strict identity/provider body as Integration. Both require the internal key;
  the Verevon BFF independently enforces organization access, and sync requires
  administrator authority and a same-origin request. GET never calls providers.
- Reports contain `websiteId`, `site`, `websiteRevision`, `provider`,
  `observedAt` (RFC3339), `window:{startDate,endDate}` (last seven completed UTC
  dates), and `status`: `not_loaded|not_configured|connected|partial|unavailable`.
  `not_loaded` is a Marketing-only absence/expiry state, not evidence of missing
  provider configuration. Missing/unconfigured data never becomes zero.
- Cookiebot payload `cookiebot:{reportingDomain,consents,inventory}`:
  `consents:{status,reportedDays,totals?}` where totals are nonnegative safe
  integers `{optIn,optOut,preferences,statistics,marketing}`; inventory is
  `{status,scannedAt?,total?,categories?,thirdParty?,secure?,httpOnly?}` with
  categories `{necessary,preferences,statistics,marketing,unclassified}`.
  Component statuses are `connected|no_data|access_denied|plan_required|rate_limited|unavailable`.
  Consent totals are events, not unique visitors. Missing dates are not filled.
  Inventory reflects the provider's scan date, not proof of current compliance.
  Connected consent totals require 1–7 reported dates. Connected inventory
  requires a valid `scannedAt`, categories summing to total and counters no
  greater than total; an explicitly empty scan can legitimately report zero.
- Algolia payload `algolia:{indexName,region,analytics,topSearches?,noResultTerms?}`
  with region `us|de|provider`
  and analytics `{status,reportedDays,searches?,noResultSearches?,noResultRate?}`.
  Component statuses match Cookiebot. `noResultRate` is 0..1, or explicit null
  for an observed zero-search window. Counts and daily coverage come from the
  v2 `searches/noResultRate` aggregate endpoint, not raw query logs.
  Nonzero searches require reported dates; an explicit zero-search aggregate
  may have zero reported dates. Counts and ratio must agree (tolerance 1e-6).
  This first slice accepts bounded ASCII index names `[A-Za-z0-9_.-]{1,128}`.
  Each optional shortlist is `{status:"connected"|"unavailable",limit:20,rows?}`.
  Connected requires an actual array (including an observed empty array);
  unavailable omits rows. Each row contains exact `query`, safe integer `count`
  and optional measured `averageResults`. Empty queries are retained; queries
  are bounded to 512 Unicode codepoints/2,048 UTF-8 bytes, reject controls and
  duplicates, and are never logged. Missing legacy components are unavailable,
  never fabricated zeros. Any connected Algolia component makes the report
  connected; components must be interpreted independently. These are bounded
  search terms, not user/session identities or record contents; terms may still
  contain user-entered information and require appropriate organization access.
- Only the matching provider payload may appear; no payload for `not_loaded`
  or `not_configured`. Failed components contain no metrics. Both successful
  Cookiebot components yield connected, one successful component partial, none
  unavailable. Explicit empty valid upstream results remain distinguishable
  from failures. API inputs and machine receipts reject unknown fields, wrong
  identities, invalid periods, unsafe counts and oversized responses. The BFF
  and frontend project allowlisted fields rather than relaying upstream JSON.
- Marketing persists at most 64 compact snapshots in a private atomic file
  beside its website registry, with up-to-six-hour freshness (also invalidated
  at UTC date/window rollover) and 60-second sync cooldown. The receipt limit
  is 128 KiB, the private cache file limit is 8 MiB. Reads revalidate current
  website/revision; explicit Algolia OAuth mappings also check current Integration
  grant metadata and the snapshot's private mapping/generation/digest proof.
  Revocation/remapping hides old observations immediately, without a provider
  analytics call. Legacy file-bound provider permissions are checked on refresh.
  Expired derived snapshots are pruned on write and by the collection worker
  while it runs (including prior UTC windows); a stopped worker does not promise
  physical deletion exactly at the freshness boundary. Audit/history files are
  unrelated and are not pruned by this mechanism.

### Connect-once Algolia collection

Saving a validated OAuth application/index mapping queues its first report;
it does not synchronously fetch search terms. The registry, mapping and grant
remain the authority. Two joined workers reconcile only existing explicit
Algolia mappings, recover interrupted jobs, refresh after six hours or at the
next UTC date boundary, and use bounded retry backoff. Unlinking, a changed
site/revision/index or a revoked grant cancels or invalidates old work. This
does not auto-enroll unrelated accounts, legacy credential bindings or Cookiebot.

- `GET /api/v1/marketing/site-sources/collection?websiteId=...&site=...&provider=algolia`
  reads lifecycle metadata only; GET never queues work or collects analytics.
- `POST` to the same path accepts exactly
  `{websiteId,site,websiteRevision,provider:"algolia"}` and returns `202`.
  Both use the internal-key boundary; the BFF additionally requires current
  organization access and administrator/same-origin authorization for writes.
- The `{data}` DTO contains identity, `status`, optional `nextRunAt`,
  `lastAttemptAt`, `lastSuccessAt` and sanitized `reason`. Status is
  `idle|queued|syncing|ready|needs_attention|reconnect_required`; reasons are
  `partial_report|provider_unavailable|not_configured|reconnect_required|context_changed`.
  Ready requires all three analytics/shortlist components; a valid partial
  report remains accessible, with `needs_attention`/`partial_report`.
- Private `site-source-jobs.json` (mode `0600`, directory `0700`) persists at
  most 64 jobs/128 KiB beside the registry. It contains no query text or keys.
  Atomic writes, storage-fault status and throttled sanitized diagnostics avoid
  silently claiming durable progress. As with other private files, this is a
  single-writer deployment, not a distributed queue.
- Existing manual `/site-sources/sync` remains compatible. OAuth report budgets
  are 32 seconds for the signed Integration client, 35 seconds for service sync
  and a scoped 38-second HTTP response deadline. Metadata checks keep their
  smaller budgets; no global server timeout or credential scope is widened.

Integration configuration uses `MARKETING_PROVIDERS_BINDINGS_FILE`, a private
regular JSON file with `{version:1,installations:[...]}`. Each installation has
`organizationId`, `websiteId`, `websiteRevision`, `site`, and optional
`cookiebot:{domainGroupId,domain,culture,apiKeyFile}` and
`algolia:{applicationId,indexName,region,analyticsKeyFile}`. Bindings and key
files must be protected regular files, not symlinks. Empty installations are a
valid unconfigured deployment. Account mappings are explicit and are checked
again around network requests. Secrets remain only in Integration Core.

Cookiebot uses its Data API key (in the documented URL path, never logged or
returned), the registered exact domain, and categories 1–5. We retain only
aggregate consent and scan counts, never cookie values, IPs or script contents.
Algolia requires a key with the `analytics` ACL for the explicitly mapped index
and account analytics region. No Admin key, search installation or index-write
permission is requested. Both adapters use bounded responses, fixed hosts,
timeouts and no redirects; errors are sanitized.

## Pulse collection health and first-party outcomes

Collection metadata includes each source's `lastSuccessAt`, using its actual
observation timestamp. Failed attempts retain that history without returning old
metrics. Manual retries, scheduled retries and restarts preserve history only for
the same website/revision/provider mapping. Older files without this optional field
remain readable. A source cache does not renew its freshness: the next collection
is scheduled before its observation expires, with bounded retry delay for expired
cache data. Full-year snapshots and saved opportunities support a bounded year of
observations, alongside quarters.

Pulse exposes collection health with last success, last attempt, stale status,
next attempt and connection settings. Status reads never trigger provider collection;
existing role and organization checks continue to gate all explicit writes.

`GET /api/v1/marketing/outcomes` projects aggregate first-party receipts owned by
Leads Core. The server-only `LEADS_OUTCOMES_URL` and
`LEADS_OUTCOMES_READ_TOKEN_FILE` select a protected read-only installation. Input
contains only the registered website, quarter and production/development environment.
The URL, revision, period and environment must match the normalized response.
No provider fields, contact information, write credential or raw response enters
Pulse. Confirmed first-party receipts include today in Europe/Oslo; Google's delayed
reporting window remains unchanged. Missing installation/booking support is shown
as unavailable rather than zero. The current installation is development only.

The full work-cycle integration test uses the real audit rules, persisted audit
ledger and work queue: missing title → saved warning task → in progress → fixed
capture → passing recheck → unavailable capture rejected → regression recorded.
Both passing and failing receipts survive reopening storage. This controlled test
never inserts fake findings into a live website audit.

### Durable website quality reports

Visibility Core owns `lab-reports.json` alongside the site registry in its existing private deployment volume. Only normalized Lighthouse scores, fixed technical checks, timestamps and collection receipts are stored; Integration Core retains credentials and provider execution. Both mobile and desktop are prepared automatically for registered sites. The scheduler checks once a minute and queues a new measurement when the last measurement is 24 hours old. GET requests only read saved state.

The last successful report survives restarts and failed refreshes. Responses include optional `cache` metadata (`stale`, `refreshStatus`, `lastSuccessAt`, `lastAttemptAt`, `nextRefreshAt`, `refreshReason`). First collection returns running/no scores; failures are never converted to zero. Failed provider attempts retry after one hour. Manual refresh uses the existing permission-checked PageSpeed command and a one-minute cooldown. Jobs are bounded to two workers, three minutes each; interrupted jobs resume after restart. Reports are isolated by organization, registered site, current revision and device. This is a single-process file repository, matching the current development registry; horizontal replicas need a shared durable job/store implementation.
