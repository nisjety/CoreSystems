# Verevon Studio demo

## Shared Pulse implementation

The authenticated Verevon app now owns `/studio/pulse` as well. Its Marketing
sidebar opens the same Pulse dashboard, website manager, search comparisons,
GA4, Clarity, PageSpeed, discovery and persisted work queue used by this sandbox.
Browser components live in `src/features/marketing/pulse`; normalized API clients
live in `src/shared/api/marketing`. The small demo exports are compatibility
adapters, not a second implementation. Client requests include the same-origin
session cookie; no provider tokens or demo service credentials are bundled.

Unlike this optional loopback sandbox bridge, the main app requires a real
session and active workspace. Its gateway checks that the workspace matches the
explicit server-side Marketing Core organization binding before forwarding any
request. Marketing Core is currently a single-organization deployment: other
workspaces must remain unavailable until tenant-aware backend support exists.
Changing workspace remounts Pulse and cancels the previous site's requests.

The standalone design sandbox remains available at port 5174; Verevon runs at
port 5173. The main app does not proxy through the no-auth demo server.

## Standalone design sandbox

This is a loopback-only design sandbox for the Studio canvas and marketing
Pulse. It renders the existing Solid Studio page in `mode="demo"`
with the SPA's shared navbar and Studio-only sidebar navigation, but without
the authenticated app shell or production gateway. Canvas
edits stay in browser memory and reset when the page reloads. Pulse has a
read-only client for Marketing Core's normalized, site-specific projection. By
default this design server blocks `/api/*` and shows unavailable values rather
than sample analytics. An optional local bridge exposes only the Pulse
projections, without a login, on a loopback-published development server. Marketing
Core persists the organization-scoped website registry; the browser stores only
the selected website ID. Adding a site does not crawl its URL. Marketing Core selects only a
matching, verified Search Console property and otherwise returns no metrics.
The Marketing Core key stays in a server-side secret file; the browser never
receives OAuth tokens, connection IDs, internal keys, or raw provider data.
Search, account, AI, and linked-system controls remain unavailable so the
sandbox cannot cross into a protected surface.

Start it from this directory:

```sh
pnpm dev:studio-demo
```

Or run the isolated Docker service from `apps/Studio Plane`:

```sh
docker compose -f docker-compose.studio-demo.yml up --build --wait
```

Open <http://127.0.0.1:5174> for the Studio canvas or
<http://127.0.0.1:5174/studio/pulse> for the marketing Pulse. The server
returns `404` for `/api/*` and `/health*` unless the optional Pulse bridge is enabled.
It is for local design and implementation work, not a production access-control
boundary. Keep the Docker port bound to loopback.

To enable the local bridge, supply the exact same Marketing Core internal key
used by the running Marketing Core service through an approved secret file. Do
not place its value in source, `.env`, shell history, or a Vite `VITE_*`
variable. Set only `STUDIO_MARKETING_CORE_KEY_FILE` in the deployment
environment, then run the base compose file with
`docker-compose.studio-pulse.local.yml` as an overlay. The overlay joins the
existing `marketing-local_marketing-local-net` Docker network and exposes no
additional host port. Restart the demo after changing the secret file.
Marketing Core must separately have a verified, authorized Search Console
connection; the bridge does not create OAuth credentials or tokens.

```sh
cd "apps/Studio Plane"
docker compose -f docker-compose.studio-demo.yml -f docker-compose.studio-pulse.local.yml up --build --wait
```

For the unified Studio Plane stack, instead run
`./scripts/provision-local-secrets.sh` from `apps/Studio Plane` and use
`docker-compose.studio-pulse.studio.yml` as the demo overlay. It mounts the
gitignored local Marketing Core key by file path and joins the dedicated
`studio-pulse-net` bridge shared only with Marketing Core;
it does not enable or fabricate Search Console data.

```sh
cd "apps/Studio Plane"
./scripts/provision-local-secrets.sh
./scripts/studio-compose.sh --profile marketing up -d --build
docker compose -f docker-compose.studio-demo.yml -f docker-compose.studio-pulse.studio.yml up -d --build
```

The bridge admits the fixed website registry/connection workflows and Pulse,
search, GA4, Clarity, and PageSpeed projections. It also admits the bounded
read-only `/api/v1/marketing/search/compare` workflow and the explicit
`/api/v1/marketing/work` task lifecycle described below.
Other demo API routes remain blocked. Local reads are rate-limited,
site-validated, sanitized, and never cached. Because this local bridge has no
login, any process or container that can reach the demo can read the selected
site's metrics. Do not bind it to a LAN interface or expose it through a tunnel.

## Search comparison workflow

Pulse's **Search performance changes / Endringer i søketrafikk** panel runs on
explicit request for the selected registered website. Query-drop review compares
two adjacent 28-day windows; content-decline review compares two adjacent 90-day
page windows and flags rows with fewer clicks and impressions. Both end three UTC
calendar days ago and are independent of the Pulse quarter selector.

The panel displays current/previous dates, sample counts, observed deltas, and
paginated rows. “Show all observations” includes one-period rows with missing
values rendered as `—`. These rows never receive a computed loss or zero baseline.
The local bridge requests at most 100 top rows per period, validates response
site/dimension/dates, strips unknown fields, and shares the existing read budget.
Changing site/revision aborts the old request and clears its evidence. No tasks,
provider mutations, automatic monitoring, or Model Plane registration result
from running a comparison.

Run its browser check while the service is up:

```sh
pnpm exec playwright test --project=studio-demo
```

## Persistent marketing work

In **Search intelligence → Opportunities**, **Add to backlog** explicitly saves
an observed suggestion. The BFF forwards only website ID/origin/revision,
quarter, and query; Marketing Core re-fetches the evidence. It never accepts
browser-submitted metrics. Saves deduplicate by site/query/opportunity type.

The lower workspace loads the site's persistent queue and real activity feed.
Start, complete, dismiss, and reopen actions use optimistic task revisions.
**Show closed** exposes completed/dismissed tasks. A failed load shows unavailable
counts and a retry, not a fabricated zero. Website changes clear prior evidence;
stale responses cannot overwrite the current context. The momentum card uses
completed/non-dismissed task counts, not traffic or a synthetic health score.
Calendar scheduling is not implemented and is labeled accordingly.

Work writes require same-origin JSON, bounded bodies, rate limits, a fixed owner
route, and server-side credentials. This is still a loopback-only demo boundary,
not production user authorization. The REST/action contracts do not automatically
register a model-executable MCP tool. See Marketing Core's README for persistence
limits and the API contract.

Technical work uses a separate evidence variant: **Technical pulse → Save to
backlog** is available only for failed, manual, or errored checks in a completed
report. It sends the check identity/device/report time, never browser-supplied
scores. The server verifies the selected observation and deduplicates by site,
device, and check. GSC metrics are not invented for technical tasks.

**Recheck** on a technical task explicitly starts one PageSpeed audit, polls its
cached status for at most two minutes, then asks Marketing Core to verify the
latest completed observation. No provider or verification POST is retried
automatically. If the provider cooldown returns the old report, the server
rejects verification; wait before explicitly trying again. Site/revision changes
abort the local flow. A recheck refreshes the technical panel using a GET only.

The task displays its original observation and latest verification separately;
the activity log retains recheck receipts. A pass does not mark the task done,
and a manual/error/missing check is not a pass. These snapshots persist across
Marketing Core restarts, unlike the full ephemeral PageSpeed report. No website
fix, scheduling, or model-executable write is performed by this workflow.

## Homepage technical diagnostics

**Technical pulse → Run audit** uses the existing asynchronous PageSpeed route.
Completed reports now include 11 allowlisted Lighthouse SEO outcomes, displayed
with product-owned explanations and an **All / Needs review** filter. The filter
never runs another provider measurement. Manual, not-applicable, informative,
error, and missing outcomes remain distinct from measured binary passes/failures.
Older cached reports without check data display **Not reported**, never a pass.

This is a homepage-navigation lab check, potentially following Google's
redirects, with device and measurement time visible. It is not a full-site
crawl, sitemap/404 inventory, proof of indexing, or field Core Web Vitals report.
The original PageSpeed quotas, explicit-run requirement, bounded polling,
tenant binding, one-hour cache, and source-failure isolation are unchanged.
The frontend rebuilds every check's `{ id, status, score }`; provider HTML,
titles, descriptions, details, and URLs are discarded at the boundaries.
