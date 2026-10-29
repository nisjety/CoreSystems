# Visibility: expanded evidence and connect-once operations

## Collection inventory

`GET /api/v1/visibility/collection?websiteId=...&site=...&quarter=...`
returns the current website/revision's normalized dataset inventory. The same
read-only action is defined in Verevon's shared action contracts and forwarded
by the authenticated, organization-scoped gateway. The response contains source,
coverage, count units, reporting resource, reporting period, freshness/status,
available attempt/success/next-run receipts, collection mode and history policy.
It contains no OAuth grants, connection credentials, raw terms or page content.

The inventory reads existing saved projections and current Integration metadata.
It never falls back to uncached provider reporting adapters, schedules jobs,
or starts an audit. Final website revision checks reject mixed-site results.
Missing counts remain null. Empty source coverage, an observed zero-row report,
an expired report, an account requiring attention and an unimplemented external
dataset are different states. A status-read timestamp is not a collection time.
First-party Web Vitals uses the export's original observation timestamp, with
development and production represented separately.

There are 22 current inventory entries: Search Console, GA4, Clarity, two
Lighthouse strategies, two CrUX form factors, Common Crawl, site audits, search
history, Trends, three Algolia components, two Cookiebot components, two Web
Vitals environments, competitor page comparisons and three external evidence
gaps (market keyword volumes, competitor rankings and backlink indexes).
The capability and history descriptions expose existing limits; this inventory
does not extend provider access, historical retention or create those missing
market datasets. Advertising reporting remains outside the current collectors.

The Synlighet `Datagrunnlag` panel presents the inventory, a gaps filter and a
website/account management handoff. Status refresh preserves the expanded view;
site, revision and quarter changes clear old data and cancel pending requests.

Verification on 29 October 2026: the live Triodelab owner returned 22 datasets,
nine available, two without CrUX coverage, seven requiring setup, one failed
Common Crawl check and three external-source gaps. Go tests (including focused
race checks), gateway contract/projection tests, UI/client regression tests,
lint, typechecking and build passed. An isolated Playwright component check used
that real owner response, verified filtering/refresh/management handoff and
390 px horizontal containment without console errors. Saved browser test
sessions had expired, so a fresh signed-in workspace browser check was not
performed. Runtime owner and gateway health checks passed. The folder rename to
`apps/Engagement plane` required repairing legacy Studio compatibility links
and Compose build contexts; existing volumes were preserved.

Visibility owns site analysis and its work queue. Integration Core remains the
credential and provider-action owner; Quarry remains the public fetch owner.
No owner reads another plane's database. No new provider permissions are needed
for these Search Console reads or public captures.

## Current bounded audit

- Up to 100 captures per audit, two concurrent captures globally. The run has a
  15-minute deadline; it stops early on authority or quota denial.
- Discovery returns up to 250 safe URLs, with sitemap indexes followed through
  four levels and at most 20 sitemap documents. The XML parser rejects DTDs,
  handles namespaces/entities, and caps entries and nodes.
- Audits also follow captured internal links breadth first, recording up to
  1,000 known candidate URLs. `coverage` separates discovered, attempted,
  checked, failed and pending URLs. It records inventory/extraction truncation,
  sitemap coverage and the stopping reason. This is not unlimited full-site
  coverage; large sites require subsequent budget/resume work.
- Structural evidence includes heading sequence/H1s, declared meta/header
  noindex/nofollow, robots response status, redirect hops, same-site hreflang and
  observed return links, JSON-LD document parsing/types and HTML word counts.
- Noindex is a review signal, not automatically a defect. Missing optional
  hreflang/schema and unvisited return-link targets are inconclusive. JSON-LD
  parsing does not establish Google rich-result eligibility. Robots response
  status does not establish Google indexing status.
- Raw HTML and JSON-LD property values are not stored in Visibility. The
  `seo_page_v1` extension is optional. Old 10-page audits stay readable and do
  not acquire invented structural checks or resolution proofs.

## Search history and configurable groups

`GET/POST /api/v1/visibility/search/research` reads saved results or schedules
collection. A serial, restart-safe background loop collects daily, retries
provider failures after one hour, and checks current Integration authority
before reads and after collection. Storage failure also backs off for an hour.

The source is the last 90 reporting dates ending three UTC dates before today,
with Search Console `date, query, page` dimensions and `dataState: final`.
At most five pages of 1,000 tuples are requested. The cap is explicit; even
below it, anonymous queries and Google's selection mean coverage is partial.
Missing rows/dates are not zero-filled. Exact www aliases are joined; unrelated
subdomains and private routes are excluded. Positions are impression-weighted.

Saved reports contain daily keyword/page relationships and derive query, page
and weekly aggregates. This is a rolling 90-day dataset, not a market-wide rank
or keyword-volume database. Reports and up to 20 editable group rules per site
live in the protected org-bound `search-research.json` file (32 MiB deployment
budget). Overlapping landing pages are review candidates only: at least three
observed overlap days and two pages with at least ten impressions each.

Group rules specify query/page, literal case-insensitive text, minimum
impressions and maximum average position (zero means no position ceiling).
`POST /api/v1/visibility/search/rules` saves/removes a rule. The UI and action
contracts use the same backend operations.

## Competitor page pairs

Keep up to three public competitor origins per registered site, with five
explicit page pairs per competitor. Existing homepage pairs remain compatible.
Each selected own/competitor URL must stay in its corresponding public origin
or exact www alias. Queries, private paths and cross-origin redirects remain
subject to Quarry/owner validation. Comparisons refresh daily, retain the last
successful result on failure, and keep up to 20 dated results per pair.

The history records capture changes, titles, metadata and structural evidence.
It is not competitor traffic, keyword gaps, backlink coverage, market volume or
proof that a content change caused an outcome. Those need additional sources.

## Evidence-backed work and planning

Audit findings can be saved directly from Pages, technical SEO groups and
confirmed broken-link views. The server resolves saved audit/finding IDs; it
never accepts client-submitted findings as proof. New structural rules reuse
that existing audit verification history.

Custom search groups can save `search_group_review` tasks referencing the
server's saved rule and measured row. Recheck evaluates the original rule
against a newer authorized report. An absent row is inconclusive; an observed
row no longer matching is a passed condition. No result automatically marks a
task done or claims that a code fix caused a search-performance change.

`PATCH /api/v1/visibility/work/{taskId}/planning` saves priority, due date and
Control user ID with task and website revision checks. Assignment is verified
against live active-org membership by Verevon's BFF before forwarding to the
trusted owner. Planning changes have durable activity receipts. Dates appear in
the existing calendar view; backlog orders by priority and due date.

## Google discovery and selection

After an authorized Google connection, matching Search Console and GA4 assets
are discovered as before. `GET /api/v1/visibility/websites/{id}/google/resources`
offers matching properties/streams when a choice is needed. POST to the same
route selects a resource by an opaque, resource-stable ID. The server freshly
rediscovers access, resolves the selected GA4 reporting host and saves the
mapping. A remembered choice survives grant refresh; a missing/revoked choice
is not silently replaced by another asset. Connection IDs/tokens stay private.

Status distinguishes no active connection, missing reporting consent, no
matching property and incomplete discovery. No DNS verification gate is added.
Clarity export credentials, Cookiebot and other providers without an equivalent
usable OAuth reporting flow still need their provider-specific setup once.
This does not create provider properties, publish tags or enable advertising.

## Organization routing

The default dev binding remains the configured organization-isolated Visibility
owner. Verevon additionally supports an optional protected deployment file via
`VISIBILITY_TENANT_ROUTES_FILE`. Its schema is:

- `version: 1`
- `tenants`: up to 64 objects with `organization_id`, fixed `url` and
  `service_key` (a real 32–512-character owner credential).

The file must be absolute, regular, at most 64 KiB and private mode 0600/0400.
Mount it server-side into the gateway; never use a `VITE_` variable. Each route
must point to an independently org-bound owner with its own protected storage
and Integration/Quarry authority. Public HTTPS origins and approved local owner
names are supported. Unknown organizations do not inherit the default owner's
authority. Invalid configuration fails closed. Browsers cannot submit a tenant
header, destination or service key. Actual tenant-owner provisioning is still a
deployment operation; adding a routing entry does not grant provider access.

The frontend remounts on authenticated user/org changes and remembers website
selection per user and organization. Switching workspaces cannot reuse a prior
workspace's loaded private panel state.

## Verified development run, 28 October 2026

- Deployed the changed Visibility owner, Verevon gateway and Quarry edge using
  their existing plane Compose stacks. Existing volumes and account grants were
  preserved. All three services reported healthy.
- Started an audit from Synlighet: 37 captures completed, zero known pending
  URLs and zero reportable findings in that run. The normalized page view joins
  the root www redirect alias and therefore displays 36 distinct content URLs.
  The additional structural checks were present in the captured evidence.
- Search Console supplied 110 observed queries and 17 landing pages for
  2026-07-28 through 2026-10-25. No rows met the cannibalization-candidate rule.
  A manual sync updated the saved report and the UI confirmed the new result
  automatically; polling reads saved state rather than starting additional jobs.
- Saved the custom group `Utvikling · topp 20` and a real review task for
  `nettside utvikler`. Assignment was checked against Control membership;
  high priority and the 2026-11-04 due date survived reload and appeared in the
  calendar. Rechecking against a newer authorized report recorded manual review,
  left the task open and added a durable activity entry.
- Compared Triodelab's `/tjenester/web-losninger` with Synlighet's
  `/tjenester/nye-nettsider/`. Both returned HTTP 200, their actual page metadata
  and structural evidence were shown, and a dated history entry was retained.
  A failed attempt during the Quarry restart was retried from the UI.
- Google resource discovery in the signed-in workspace returned the existing
  Search Console domain property and GA4 stream without another sign-in.
  This read-only verification did not replace the working website mappings.
- Browser checks covered desktop and 390 px widths, vertical scrolling and
  absence of horizontal document overflow on the research view. No browser
  console errors were recorded in these flows.
- Passed the Visibility Go suite (`go test -p 1 ./...`), focused Go race tests,
  gateway all-target tests, Quarry edge/transform tests, focused Vitest suites,
  TypeScript checking and the frontend production build. New sync polling tests
  also cover cancellation when site/workspace context changes.

These checks validate this development deployment and its configured workspace.
Larger resumable crawls, evidence older than the rolling research window,
additional tenant deployments and provider-specific account setup remain
explicit operating/provisioning work. No competitor keyword-volume, backlink
or advertising dataset is inferred from the public page comparisons.

## Collection reliability follow-up, 29 October 2026

Public-source refreshes now retain a separately validated `cachedReport` while a
new job is queued/running or has failed. It is bound to the same website/revision,
keeps its original observation time and is removed after 48 hours, including
when collection is paused. It is not presented as new successful evidence.
Normal reports still become stale after 24 hours. Existing enrolled jobs with
transient per-provider timeouts/backpressure retry after six hours rather than
waiting a full day. Reads remain pure saved projections; provider collection is
not triggered by opening the page. Context/authority errors clear browser data.

Integration Core serializes and paces Common Crawl requests as before. Following
a timeout, it can query one older collection from validated public metadata. Only
actual captures can replace the timeout, with their actual archive ID retained;
an empty older collection cannot prove absence in the newer collection. Quota
responses do not trigger collection fallback. The current latest archive remains
upstream-unavailable for Triodelab; a successful older metadata/index request
returned no captures. No archive data was fabricated or WARC content fetched.

Triodelab's development Web Vitals installation was rebound to the active
workspace/site revision. Port 3000 belongs to another local project, so the
collector now runs at `http://localhost:3001`. Only ports 3000/3001 are admitted
for local development; Integration's backend port must match the reporting port.
Production origin, TLS, consent and retention restrictions remain unchanged.
No credentials were rotated. A clean Playwright/Chrome document sent nothing
before explicit local consent, then actual LCP, INP and CLS reached Dragonfly,
Integration Core and Visibility (one measurement per metric, development only).
Import remains an explicit authorized operation, not an automatic Web Vitals job.

The internal-link view now identifies source/destination pairs and separately
explains unique destinations. Triodelab's live audit has 638 distinct pairs and
36 distinct destinations; duplicate copies of a pair are already removed by the
owner. Shared menu links across different source pages are distinct edges.
Competitor tables additionally present captured word counts, H1/headings,
noindex/nofollow declarations, structured-data types/errors, same-site language
alternates and redirects; missing capture evidence stays unknown. The Skai pair
was refreshed successfully with structural evidence. Opportunity reporting links
to the existing SEO groups (near page one, low CTR, technical and internal-link
review), without inferring market volume or competitor rankings.

Verification: focused Go/race suites, gateway all-target tests, frontend unit
suites, typechecking and production build passed. Lint reports existing warnings
and no errors. Browser plugin controls were unavailable; Playwright with installed
Chrome verified actual local collection and isolated UI components using live
owner responses at desktop/390 px, with no page errors or mobile document
overflow. This is not a fresh signed-in workspace browser verification.
