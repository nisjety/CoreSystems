# Unified Studio marketing system

## Product intent

Studio combines Marketing Core, Social Core, and Leads Core into one workspace
for humans and AI. A registered website is the shared context. Marketing owns
SEO, AIO, SXO, GEO, search intelligence, marketing measurement, and marketing
recommendations. Social owns social publishing and its execution lifecycle;
Leads owns lead records and lead workflows. Studio presents their results and
actions together using owner APIs, without crossing databases.

Triodelab is the first validation website. Its public development app runs at
`http://localhost:3000/`; the marketing UI runs at
`http://127.0.0.1:5174/studio/pulse`. Private provider metrics use the registered
`https://triodelab.no/` context. A local rendering of a page does not have a
separate Google analytics history and must not silently replace this mapping.

## Shared execution boundary

1. Control Plane identifies the user, organization, permissions, and entitlements.
2. Integration Core owns provider consent, credentials, renewal/revocation, and
   fixed provider actions. Marketing does not store raw provider tokens.
3. Marketing Core owns website mappings, normalized reporting, analysis rules,
   and the evidence-backed marketing task lifecycle.
4. Studio UI and a future Marketing MCP adapter call the same owner contracts.
   Model Plane governs AI execution. MCP cannot accept an arbitrary organization
   or treat possession of an internal service key as a user's authorization.

“Connect once” means reuse an authorized provider grant across permitted tools
and Studio workflows. It still needs asset selection, provider-specific scopes,
renewal handling, and additional consent when capabilities expand. A Google
reporting grant does not authorize Microsoft, Meta, CMS writes, or ad spending.

## Delivery slices

| Slice | State | Reviewable outcome |
| --- | --- | --- |
| Website registry and provider mappings | Implemented | Stable website ID/revision, server-side source mapping, selected-site panel refresh |
| GSC, GA4, Clarity, PageSpeed reads | Implemented, provider access dependent | Real normalized reports with explicit unavailable states |
| GSC comparisons in Studio | Implemented | Trigger 28-day query changes or 90-day page decline analysis; review dates, comparable/one-period rows, deltas, and coverage limits |
| Marketing work queue | Implemented | Save GSC opportunities or actionable technical checks; retain original evidence, deduplicate, update/reopen work, and append newer technical verification receipts |
| Technical audit | First increment implemented | 11 normalized Lighthouse SEO check outcomes through existing PageSpeed; full robots policy, sitemap traversal, headings inventory, and broken-link crawling remain planned |
| Scheduled monitoring | Planned | Durable runs/snapshots, provider quotas, retries, site-scoped comparisons, reviewable alerts |
| Ads reporting | Planned | Google/Meta/LinkedIn account mappings and spend/performance projections through Integration Core |
| Campaign operations | Planned | Draft/validate/approve/execute with budget constraints, idempotency, and audit receipts |
| Governed Marketing MCP | Planned | Authenticated tool discovery and calls over the same owner API, scoped to the caller's organization/site/capabilities |
| CMS workflows | Planned | WordPress/Strapi discovery, draft changes, preview/approval, and tracked publication |
| GEO/AEO/SXO analysis | Partial inputs only | Combine source-attributed content checks, performance, consented UX aggregates, and measured AI referrals/citations where sources support them |

## Skills and reusable capabilities

Skills supply versioned analysis instructions, prerequisites, interpretation,
and output guidance. Backend code supplies authorized data access, deterministic
checks, jobs, storage, and side effects. Installing a skill does not implement an
API or register an AI tool. Each workflow needs a documented owner contract,
validated input/output, source/period/coverage metadata, tests, and a Studio UI.

The current `marketing-gsc-analysis` skill covers observed opportunities,
comparisons, and brand classification over the returned query sample. It must
not claim exhaustive keyword totals, explain causes from a correlation alone,
or calculate a loss when a row is absent from one period.

External MCP/skill repositories are capability references. Before reusing code,
verify the actual implementation, license, API support, and tenant/credential
boundaries. An MCP wrapper does not replace provider integration or evidence.

## First slice verification, 22 October 2026

The comparison route is available in the local Marketing Core Docker service.
Studio sanitizes the response and binds it to the requested website, dimension,
and adjacent periods. A workspace regression test covers per-site Google routing
and refusal to use a legacy connection for an unregistered website. Missing
observations retain unknown values; private nested fields are discarded.

Comparisons do not create tasks. The separate work-queue slice below supports
explicit saves. Schedules, localhost crawling, ads management, and a governed
Marketing MCP remain subsequent owner workflows.

## Work-queue slice verification, 22 October 2026

Studio now saves an observed Search intelligence opportunity through its bounded
same-origin BFF into Marketing Core. The server re-fetches source evidence rather
than trusting submitted metrics. Per-site tasks, optimistic status updates,
deduplication, real activity, and calculated completion are persisted beside the
website registry. These operations do not mutate Google or the website.

The local browser flow saved Triodelab's actual `nettside utvikler` suggestion,
tested start/return-to-open, and verified the task/activity remained after a
Marketing Core Docker restart. The finding remains open; no SEO improvement or
task completion is claimed by this lifecycle test.

## Triodelab tracking release, 22 October 2026

The separate `triodelab/triodelabs` repository was normally pushed to `main` at
`affff86b6ee871aed4304a943d227a9a89cf3925` and the automatic Vercel production
deployment reached Ready (`dpl_7kxEwqCGnWJF7mmoaKNhRzic7G4V`). The live
`www.triodelab.no` site includes GA4 `G-ZEWPHF6X3K` and Clarity `z1m2pzgxwm`
behind Statistics consent; Marketing consent remained off in the browser test.
Cookiebot automatic Consent Mode remains disabled so the application's consent
boundary controls collection, and Clarity body masking is enabled.

The in-app browser observed GA4 `/g/collect` and Clarity `/collect` responses
returning HTTP 204 from the live page. This verifies collector acceptance, not
completed report processing. GA4 reporting delay and Integration Core's cached,
rolling 72-hour Clarity export can delay what appears in Pulse. No analytics
values are synthesized to fill that gap.

## Homepage technical-check increment, 22 October 2026

Integration Core now projects 11 fixed Lighthouse SEO audit outcomes from the
existing PageSpeed measurement. Marketing Core validates the bounded status/score
contract; Studio strips nested provider extras and displays localized labels,
coverage limits, and an All / Needs review filter. No new website-fetch endpoint,
provider credential, or automatic audit trigger was introduced.

The live mobile `https://triodelab.no/` measurement returned 10 binary passes and
one manual structured-data review at 14:28 Europe/Oslo (Lighthouse 13.5.0).
Pulse displayed the real report through Integration Core and Marketing Core, and
the review-only filter retained just the manual item. Scores and check counts
describe this observation, not a durable or whole-site health claim. Sitemap and
broken-link crawling still require a separate Quarry-governed capture workflow.

Verification: 243 Studio tests, typecheck, focused lint, and production demo build
passed. Marketing's Pulse tests/race checks passed with 90%+ package coverage;
Integration's PageSpeed race suite passed with 98.1% coverage. Scoped security
review found and resolved a cross-service status enum mismatch before the real
end-to-end measurement.

## Technical task and verification loop, 22 October 2026

Actionable `failed`, `manual`, and `error` checks can now be explicitly saved as
technical tasks. Marketing re-reads the exact completed PageSpeed report before
saving; the client supplies identities and the report timestamp, never metrics.
Legacy GSC work is preserved. Kind/device/check-aware deduplication keeps the two
evidence types separate, including null unscored checks without invented GSC data.

The task's Recheck action starts one provider audit, polls GET status with a
two-minute bound, then requests a server-authoritative verification. The observed
time must be strictly newer than the original and any prior verification. New
receipts persist independently of full ephemeral audit reports. A passing check
does not complete the manual task; unresolved/manual/error/missing results never
become an inferred pass. The latest panel is refreshed via GET after verification.

The in-app browser saved Triodelab's actual mobile structured-data review
(`18:52:53` Europe/Oslo), rechecked it (`18:54:03`), and displayed its still-manual
result with the task open. Both the task and verification receipt remained after
a Marketing Core restart. The pre-upgrade protected work-file backup was retained.
This did not change Triodelab's code or fabricate a resolved SEO issue.

The backend suite, vet, and race checks passed (workqueue coverage 91.8%). Input,
site/revision, source freshness, pointer isolation, legacy storage, and mixed
evidence regression tests cover the new boundaries. Multi-page crawling,
schedules, general assignment/prioritization, and governed AI writes remain
subsequent slices, not implied by this completed technical-task increment.
