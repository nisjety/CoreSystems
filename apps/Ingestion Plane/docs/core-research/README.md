# Ingestion Plane Core Research

Generated: 2026-06-07
Updated: 2026-07-11 (full 7-service re-verification; host-curl + source only — Docker `exec`/rebuild/`logs` blocked by containerd corruption)

This directory contains the current core-level research notes for the active Ingestion Plane services.

## 2026-07-11 re-verification headline

The user's opening complaint — velion chat can't state a shipping time for Oslo→Trondheim — traced to a live-confirmed **Bring delivery-time parsing defect** in shipping-core (dropped Bring's top-level `expectedDelivery` promise → `transit_days:0` / `0001-01-01`). **Fixed in source this pass** (`internal/carrier/bring/{wire,bring}.go` + 2 new production-shaped tests, full suite green); needs a shipping-core rebuild to go live (Docker rebuild currently blocked). Full detail and the other findings (Quarry unauthenticated control plane, imports-core live DB outage + wrong document-service URL, integration-corev2 has no Visma provider, autocomplete-core can't start) are in `plane-audit-2026-07-11.md`, `../../INGESTION_PLANE_STATUS.md`, and `../../INGESTION_PLANE_ROADMAP.md`. Every app container shows "(unhealthy)" only because the exec-based healthcheck fails under the corrupted containerd store — the processes serve traffic (11/12 endpoints return 200).

Latest plane audit: `plane-audit-2026-07-11.md` (renamed from `-07-02`; retains the 2026-07-02 baseline and 2026-07-10 pass below the new 2026-07-11 synthesis).

Primary cores (all re-verified 2026-07-11): `shipping-core.md` (new), `quarry-v2.md`, `integration-corev2.md`, `finspo-core.md`, `imports-core.md`, `autocomplete-core.md`, `support-worker.md`.

<details><summary>2026-07-10 Live Runtime Status (prior pass, preserved)</summary>

## 2026-07-10 Live Runtime Status

The Docker stack was running and was checked with non-mutating HTTP probes on 2026-07-10. This is the current runtime addendum to the historical 2026-07-02 plane audit; where the two disagree, the dated live result below is authoritative for the inspected local deployment.

| Area | Live result | Current interpretation |
|---|---|---|
| Shipping | Real Bring production rates returned for Oslo `0150` to Trondheim `7010`; DHL and UPS test environments also returned rates; FedEx sandbox authorization failed. | Carrier connectivity is real but mixed across production, sandbox, and explicit mocks. `is_mock=false` does not prove production. |
| Bring delivery time | Upstream returned 2-4 working days for sampled services, but shipping-core exposed `transit_days: 0` and a zero date. | The adapter reads only `alternativeDeliveryDates[0]`; Bring returned delivery fields at the top level. |
| Integration Core | 20 providers in the catalog, 11 connections marked active, 8 live discoveries succeeded and 3 failed token resolution. | `active` is not an effective-health guarantee. |
| Imports Core | Health passed and write routes rejected missing internal auth. Job detail/event reads still reach lookup without auth. No jobs existed to test a successful read. | Job GET/SSE authorization remains a blocker. |
| Quarry v2 | SearXNG search and a ZDR/no-ingest Bring scrape succeeded. | Web acquisition is real, but the local edge accepts any non-empty bearer because dev bypass is enabled. |
| Quarry Control | Health passed; unauthenticated `GET /v1/jobs` returned 200 with HMAC enforcement disabled in the dev deployment. | Control must not remain host-exposed in this posture. |
| Social connector path | Four accounts were visible across connected organizations; three tokens were available and one Meta token was expired. | The connector path is real, but connected status must be separated from token health. No publish was attempted. |

The running images are not revision-labelled. Shipping-core was built on 2026-07-04 and the Velion gateway on 2026-07-08; current source registers shipping reliability and recommendation endpoints that both running images return as 404. Rebuild and revision labelling are required before source status can be treated as deployment status.

Prominent blockers:

- Shipping-core and the Velion shipping proxy expose carrier and quote endpoints without user/session or tenant enforcement.
- Booking, label, tracking, manifest, and audit routes share the same unauthenticated shipping router in current source. Do not exercise them against live carrier credentials until authorization is added.
- Imports job detail and progress SSE routes have no authorization dependency.
- Quarry Edge is in dev bearer-bypass mode; Quarry Control has HMAC enforcement disabled and is published on all interfaces.
- The repository smoke scripts are not a reliable current E2E gate: the Ingestion Makefile uses stale ports, the Integration smoke creates connect sessions, and the Quarry smoke omits auth/ZDR.

Current target: new Velion v3 ingestion work should target `Quarry-v2`. Legacy `Quarry/` references still exist in top-level tooling and are tracked in the audit.

</details>
