# Engagement Plane

Verevon's Engagement workspace combines separate product owners behind shared
identity and Integration Core connections. The search workspace is **Visibility**
at `/engagement/visibility`; `/studio/pulse` redirects to that authenticated route.
Norwegian UI labels are **Engasjement → Synlighet**. Anbud is entered from that section and opens its own contextual sidebar while
Engagement stays selected in the rail. Radar remains inside Anbud, without a
separate Engagement shortcut; existing authenticated `/anbud` routes remain valid.

| Core | Owns | Does not own |
| --- | --- | --- |
| Visibility | SEO/SEM visibility, Search Console, GA4, Clarity, technical audits, search trends, evidence-backed search work | Social publishing, person records, tender workflows |
| Marketing | Cross-channel campaign plans, briefs, coordinated launches, approvals and execution policy | Duplicate search reporting or provider credentials |
| Social | Social accounts, publishing, approvals, monitoring and metrics | Search-engine optimization |
| Leads | Lead generation, potential customers and website inquiry records | Visibility dashboard |
| Anbud | Tender discovery, matching and follow-up | Search campaigns |

For search advertising, Visibility owns search-market analysis and performance;
Marketing owns campaign approval/budget orchestration. Integration Core owns
provider OAuth and credentials. A reporting operation cannot publish an ad or
activate spend. Search trends are interpreted in Visibility; broad campaign or
social trends remain with their respective owners.

## Current capability and next search work

Existing data sources, registered-site permissions, audit/work queues, sync
history, alerts and weekly reports are retained. Search Console reports the
site's observed search impressions, clicks, queries and positions. Pytrends is
an explicitly unofficial development fallback for relative interest. Neither
source is advertised as absolute market-wide keyword search volume.

Google Ads reporting/execution, market-wide search volume and competitor search
analysis are not implemented yet. They require explicit provider capability and
account/resource selection behind Integration Core; do not fill those panels
with sample data or treat a Semrush reference as access to Semrush data.

Marketing Core's campaign runtime is not implemented in the new directory yet.
Its README defines the ownership boundary, rather than launching a second copy
of Visibility. Social, Leads and Anbud remain independent, running cores.

## Migration and development startup

`visibility-core/`, `social-core/`, `leads-core/` and `anbud-core/` contain the
active source trees. Compose files, deployment scripts, broker configuration,
observability and protected local deployment files now live here too. The old
Studio Plane directory and its compatibility links are retired. Engagement's
`marketing-core/` remains a separate future campaign owner.
The Docker Compose project is `engagement-plane`. Its database runs as
`engagement-postgres`, backed by `engagement-plane_postgres-data`. Other
persistent state, including Visibility reports, belongs to `engagement-plane_*`
volumes as well. The service key `marketing-core`, protected `.env`/secret mounts,
service principals, API action IDs and `MARKETING_*` deployment settings remain
stable. The search service identifies as `visibility-core`, with that DNS alias
on the inter-plane bus. Those wire identifiers are compatibility contracts, not
a second product owner.

Use the canonical Engagement launcher, which loads each core's environment and
the protected local secrets before selecting the enabled overlays:

```sh
STUDIO_ANBUD_ENABLED=1 STUDIO_PLANE_LINKS=1 SHARED_NATS_ENABLED=1 \
STUDIO_WEB_VITALS_ENABLED=1 \
sh scripts/engagement-compose.sh --profile marketing up -d
```

The root Compose file defines the Engagement stack. Use the launcher for
protected local configuration and enabled overlays. The old Studio database and
report volumes were copied with writers stopped; existing database names, roles,
password verifiers and report contents were retained. Do not start an empty data
volume or restart both projects against the same database as a rename shortcut.

The protected `.local/migrations/studio-to-engagement-*` directory contains the
pre-migration configuration, logical PostgreSQL backup and verified storage
manifest. Original `studio-plane_*` volumes are retained as inactive rollback
copies; ordinary startup never mounts them. Do not prune those backups until the
migration has been independently accepted. Integration Core's provider grants
remain in its existing Ingestion Plane database and are not moved into Engagement.

For the complete local development runtime (Control, Verevon, Quarry-v2 and
Integration Core, followed by Engagement), use:

```sh
node scripts/engagement-runtime.mjs config
node scripts/engagement-runtime.mjs up
node scripts/engagement-runtime.mjs status
```

This does not start Model Plane or the retired standalone Studio demo. Control,
Ingestion and Verevon launchers use this directory's `.local/secrets` directly.
Those files are private, ignored by Git and must never be copied into source or
printed by a resolved Compose configuration. `config` uses `--quiet` for that
reason. Historical deployment notes are retained under `docs/legacy-studio/`;
they are not startup instructions for the current stack.

`/api/v1/visibility/*` is the canonical browser API namespace. The gateway enforces
the same real Control Plane session, canonical membership, site and role checks
as the legacy `/api/v1/marketing/*` routes. It never forwards browser scope or
provider credentials. The upstream service accepts both namespaces during
migration. Lead outcomes are excluded from the Visibility namespace and UI.

Future removal of legacy deployment and action identifiers needs a separate
migration of registered callers, scopes, volume backups and scheduled jobs.
