# Ingestion Plane — Roadmap

> **2026-07-20 correction:** item 1 under "Approved maintenance work still to
> execute" below ("Docker Desktop is stopped") is stale — a live `docker ps` on
> 2026-07-20 shows Ingestion Plane containers up and healthy, zero
> unhealthy/restarting. The maintenance incident was resolved around
> 2026-07-16/17. Items 2-5 (scoped credentials, image/revision labeling,
> deploy order, post-deploy acceptance matrix) were not re-verified in this pass
> and remain open as stated.

> **2026-08-14 Quarry correction:** the local Chromium browser-network gate is
> now closed by focused proxy/DNS tests and real-browser redirect, iframe,
> XHR/fetch, image, and script-navigation fixtures. This does not promote
> Browserless, Browserbase, or Kernel, and it does not close native AX/OOPIF,
> artifact transfer, dialog approval, or provider-meter gates. Quarry remains
> first-party; no Firecrawl dependency or fallback is part of this roadmap.

Read `INGESTION_PLANE_STATUS.md` first. This roadmap separates the secure MVP gate from enterprise-next work.

## MVP gate

### Completed in tested source

1. Shipping Bring promise parsing, endpoint authentication, tenant-owned booking persistence, confirmation expiry, ZDR suppression, provider provenance, and scoped Data Plane handoff.
2. Integration shared-key tenant bypass closure, local JWT verification, scoped service writes, and Model execution token minting.
3. Imports signed identity, tenant-scoped reads/SSE, durable Postgres job recovery, fail-closed quota, SSRF controls, corrected Control subjects, truthful M365 behavior, ZDR rejection, and live Data Plane contract/token shape.
4. Quarry production HMAC fail-closed startup plus local-Chromium per-session DNS-pinned egress and request-level SSRF/DNS/redirect/frame/XHR/subresource proof; remote browser capabilities remain false until separately proved.
5. Autocomplete fail-closed config/auth and dependency-aware readiness.
6. Support automation declassified to an opt-in dormant profile; Finspo regression suite preserved.
7. Base/production Compose validation with required production credentials and dependency remediation for reachable Python, Go, gateway, and autocomplete advisories.

### Approved maintenance work still to execute

1. Treat `docs/runbooks/docker-content-store-recovery.md` as the historical incident/recovery record; the Docker-stopped state was resolved around 2026-07-16/17.
2. Re-measure host capacity and record current image/config/revision evidence before promotion work.
3. Rehearse a non-destructive Docker/containerd recovery in an approved window if operational acceptance requires it.
4. Bring up dependencies in order: Postgres/Dragonfly/NATS → Temporal/Qdrant/MinIO/SearXNG → Control/Data/Model → Ingestion APIs/workers → Gateway/frontend.
5. Build revision-labelled images, run migrations once, and deploy only the affected services.

### Post-deploy acceptance matrix

1. Auth negative matrix: no token, garbage token, wrong audience, conflicting tenant header, and cross-org resource ID all fail.
2. Shipping read-only: carrier modes are explicit; isolated Oslo→Trondheim Bring quote has a real nonzero promise. Never create/confirm/cancel a booking in smoke.
3. Imports: readiness proves DB/Auth/Data; synthetic upload reaches Data Plane; job detail and SSE remain org-pinned; restart recovery resumes a queued synthetic job.
4. Quarry: missing/garbage bearer fails; Control unsigned request fails; ZDR/no-ingest public test scrape persists nothing; SearXNG returns at least one real result or a typed degraded response; local Chromium advertises isolated egress only with its pinned proxy; remote drivers are refused; action receipts match the observed deny/allow result.
5. Integration: cross-org access fails; read action works with a synthetic provider; no write without independently captured approval/idempotency evidence.
6. Autocomplete: real suggestion reaches v3 or the UI renders typed unavailable.
7. Finspo readiness recovers; support automation remains absent unless its entire dependency contract is deliberately enabled.

### Source gates still open

- Raise imports security-critical coverage and add endpoint-level tests for readiness, restart recovery, connector redirects, and Auth/JWKS failures.
- Add native AX/OOPIF stale-root and child-target proof, artifact upload/download quarantine/correlation/malware/type/size proof, and signed dialog-approval replay. Promote each remote browser provider only after equivalent request-level egress tests. Keep provider cost unknown until an authoritative meter exists.
- Add provider-health states (`configured`, `token_usable`, `expired`, `sandbox`, `verified_live`) consistently across Integration.
- Add server-side Integration action idempotency and authoritative approval lookup.
- Expose the OCI revision already embedded in release images through readiness/diagnostics.

## Enterprise-next (after MVP acceptance)

- Workload identity/mTLS, automated rotation, HA/DR, multi-region/residency.
- Formal retention/legal hold, customer-managed keys, audit export, compliance evidence.
- Carrier/provider certification, contract/load/chaos testing and advanced rate/cost controls.
- Large crawl/import backpressure and zero-downtime rolling migrations.
- Connector governance/SCIM and formal SLO/error-budget operations.

## Documentation hygiene

Do not delete or archive the February document cluster without approval. Keep `docs/core-research/plane-audit-2026-07-11.md` as historical live evidence; append newer source/deployment evidence rather than rewriting old observations as if they were live.
