# Ingestion Plane — Current Status

> **2026-07-20 correction:** the "Docker Desktop is currently stopped" /
> 99%-full-host-volume claim in the Release decision below is stale. A live
> `docker ps` on 2026-07-20 shows the Ingestion Plane's containers (and 94
> containers across CoreSystem overall) up and healthy with zero
> unhealthy/restarting. The Docker maintenance incident this doc describes was
> resolved around 2026-07-16/17; this status page was never updated. Host disk
> headroom itself was not re-measured in this pass — only that Docker/containerd
> is responsive and containers are healthy. All other MVP blockers below
> (scoped-credential provisioning, live read-only proofs, coverage gates) were
> not re-verified in this pass and remain open as stated.

Last verified: 2026-07-13 (Application notification consumer contract refresh; broader Ingestion deployment verification remains blocked)

## Release decision

**MVP source candidate: substantially hardened, but not yet production-accepted.** The tested source closes the primary auth, tenant, import durability, Data Plane contract, quota, connector SSRF, and Bring correctness failures. Docker Desktop is currently stopped after the approved restart could not mount its 278 GiB data image on a 99%-full host volume. No prune, reset, volume deletion, rebuild, or migration was performed.

Production promotion remains blocked on the explicit Docker maintenance gate, secret/service-principal provisioning, rebuild/migrations, and read-only live E2E. SearXNG effectiveness and autocomplete's visible unavailable/live-suggestion behavior are also not proven.

## Source-fixed and tested

| Area | Current source state | Deployment state |
|---|---|---|
| Shipping | RS256 ingestion auth on every `/api/*` route; signed org/actor pinning; org-owned booking/manifest storage; service-scope checks; booking approval/idempotency fields; confirmation expiry; ZDR suppresses events/Data writes; provider `production`/`sandbox`/`mock` provenance; Bring delivery parsing fixed; Data Plane evidence now uses scoped `data-plane` tokens and idempotency. | Not rebuilt. Do not run booking smoke tests. |
| Integration Core | Shared tenant key disabled except triple-gated isolated E2E; local RS256/JWKS verification with cache; internal key cannot bypass org ownership; service writes require `integration:write` plus approval; Model execution client mints scoped tokens. | Not rebuilt. Provider token usability/provenance breadth still needs live proof. |
| Imports | Signed ingestion identity on create/read/detail/SSE; frontend gateway mints audience tokens and streams SSE without buffering; quota fails closed with typed 503; CMS/Odoo public-network guard, redirect denial and response bounds; durable Postgres payload/lease recovery replaces volatile closure-only work; real Data Plane `/v1/documents/` contract with `documents:write` token and idempotency; truthful `/ready`; ZDR persistence requests rejected; M365 orphan job false-success removed; Control subjects corrected to `aqencia.*`. | Not rebuilt; existing live DB outage remains until maintenance/redeploy. |
| Quarry | Production Edge requires a valid internal HMAC signer; production Control forces HMAC enforcement; direct browser-agent navigation is checked at the Edge and DNS guarded; existing auth/ZDR/cache suites remain green. Its production container now has an immutable root filesystem, all capabilities dropped, and bounded browser scratch space while preserving Chromium's own sandbox. | Running dev image still has prior posture. Search effectiveness remains unproven/degraded. |
| Autocomplete | Missing internal token now fails startup outside double-gated isolated E2E; constant-time bearer check; Sonic-aware readiness; canonical Ingestion Compose owns both autocomplete and its Sonic index. Rust MSRV and image toolchain are aligned at 1.91. | Not deployed; Ingestion token/Sonic/NATS/UI E2E still required. |
| Support worker | TypeScript compiles; canonical typed-user `POST /api/v1/notification-requests`, service-specific HMAC, ZDR, deterministic idempotency, 10-second timeout, strict response validation, and failure propagation have 5 passing consumer-contract tests. Capability remains dormant behind `support-automation`; notification mode defaults disabled. | Existing stale container may still run. No current workflow supplies authoritative Control organization/user mapping, and CSAT requires a separate consent-aware external-contact contract. Disabled activities complete as skipped without a durable skip metric. |
| Finspo | Existing source preserved; full Go tests and vet green. | Live `/ready` must be rechecked after Docker repair. |

## Verification completed

- `shipping-core`: `go test ./...`, `go vet ./...` — green.
- `integration-corev2`: `go test ./...`, `go vet ./...` — green.
- `imports-core`: 46 tests green on Python 3.13; compileall green; `pip-audit` reports no known vulnerabilities after upgrading FastAPI/Starlette, JWT, multipart, PDF/XML, SSE, and dotenv dependencies. Whole-app measured coverage is **48%**, below the 80% program target; changed network-policy code measured 81%, while auth/connectors still require more branch coverage.
- `autocomplete-core`: 7 Rust tests and strict clippy green on the declared Rust 1.91 toolchain.
- `Quarry-v2`: `cargo test --workspace` green; focused new browser navigation security tests green. Five Chromium SSRF live tests remain ignored because they require a browser/live network.
- Frontend gateway: 209 Rust tests and strict clippy green. `cargo audit` has no vulnerability finding; it reports one allowed `anyhow` unsoundness warning pending an upstream release.
- Model `execution-core`: 178 tests green; one live-stack browser E2E intentionally ignored.
- `finspo-core`: full Go tests and vet green.
- `support-worker`: build/typecheck and 5/5 Node consumer-contract tests green.
- Base and production Compose render successfully with synthetic required values.
- Reachable third-party Go findings were removed from Integration/Finspo by upgrading Fiber and pgx. The local scanner still reports Go 1.26.2 standard-library findings; production builders are pinned to fixed Go 1.26.5. Shipping's Go vulnerability scan is clean.
- `autocomplete-core` Cargo audit is clean after upgrading `async-nats`. Quarry's audit retains an inactive `rsa 0.9.10` lockfile advisory with no fixed release; `cargo tree -i rsa` confirms it is not in the active build graph.

## Remaining MVP blockers

1. **Docker storage capacity/recovery:** authorize a non-destructive host-space action or a destructive Docker reset/restore decision. The approved restart was attempted and failed at VM disk readiness; Docker Desktop is stopped. See `docs/runbooks/docker-content-store-recovery.md`.
2. **Provision scoped principals/secrets:** Auth Core must register separate `imports-core`, `shipping-core`, and `model-execution` credentials with only the required audiences, orgs, and scopes. Production Compose now requires the imports/shipping credentials.
3. **Live read-only proofs:** Bring Oslo→Trondheim must return nonzero transit/ETA; imports must create a synthetic document in the intended test org; cross-org reads must fail; no booking/provider write is allowed in smoke.
4. **Quarry search:** at least one SearXNG engine must return results and degraded/empty behavior must be exposed honestly. Redirect/rebinding protection still needs a browser-network interception E2E, not only initial-navigation guards.
5. **Autocomplete:** provision token/Sonic/NATS and prove a real suggestion through v3, or ship a visible typed unavailable state instead of silent empty results.
6. **Coverage/release gates:** imports-core remains below the requested 80% whole-module target. Repeat dependency audits with the release images/toolchains and capture the reports as promotion evidence; the source scans above are green or explicitly dispositioned.
7. **Integration writes:** server-side idempotency and independent approval lookup remain enterprise-near/MVP-hardening work; current enforcement trusts the scoped Model execution service to forward its durable approval ID.

## Boundaries

- Visma remains a Model Plane MCP capability. Ingestion must not claim or duplicate it.
- Channel Plane remains docs-only.
- The eight stale February documents remain untouched pending human sign-off in `apps/STALE_DOC_DELETION_REGISTER.md`.
