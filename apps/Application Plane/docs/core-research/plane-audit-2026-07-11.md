# Application Plane Audit

> **Historical audit.** `plane-audit-2026-07-13.md` supersedes every current-state conclusion in this file. Postgres is healthy in the July 13 runtime, leads-core is native arm64 and reached real Brreg, Convex missing functions are fixed in source, notification callers use the canonical route in source, and traffic provenance is fixed in source. This file is retained to preserve the evidence trail and corrected/refuted findings; do not use it as the release decision.

Baseline date: 2026-07-02
Live verification dates: 2026-07-10 (first pass) and 2026-07-11 (full 9-service re-verification, this pass)

Scope: `apps/Application Plane` (conversation-core + conversation-ingest-rs, convex-core, information-core, notification-core, insight-core, leads-core, social-core, zammad-foundation)

See also `APPLICATION_PLANE_STATUS.md` and `APPLICATION_PLANE_ROADMAP.md` at the plane root, and the per-service docs in this directory (all re-verified 2026-07-11).

## 2026-07-11 re-verification — executive summary

Verified with host-curl + source reading (Docker `exec`/rebuild/`logs` broken by containerd corruption). Findings graded `[live-curl]` / `[source-only]` / `[inspect]`.

### The single dominant fact: the shared application-postgres data volume is CORRUPTED

Direct `pgx` probes to `application-postgres` (:9540) return `FATAL: could not open file "global/pg_filenode.map": I/O error (SQLSTATE 58030)` — the same host containerd/overlay corruption has now reached the Postgres **data directory**. Consequence: **every DB-backed Application Plane service returns HTTP 500 on real reads/writes** (conversation-core Inbox, notification-core, insight-core, social-core, leads-core) even though each service's `/health` is 200, its code is correct, and it connected + migrated cleanly before the corruption. This is an **infrastructure/data-layer failure, not a code defect** in any of these services. Restoring/reinitialising the `application-postgres` volume (part of the operator-approved Docker maintenance) is the gating fix for the whole plane's live functionality.

### Prior red-flag findings that are now RESOLVED in source (re-verified)

- **conversation-core** — the WhatsApp/Messenger "replies silently never send" **prod bug is fixed** (commit f007642b): `AddMessage` now sends-first-then-persists via integration-corev2 and returns HTTP 502 `send_failed` instead of a phantom "reply sent" with an undelivered row. `conversation-ingest-rs` `/internal/ingest/email` **fail-open is fixed** (auth middleware precedes body parse; boots keyless-refusing; constant-time compare). HITL AI-action execution is real + idempotent (atomic approved→executed claim, UNIQUE send-audit dedup). Org isolation is IDOR-clean (org from session-set `x-org-id`, not a client header — closes the AI-First `x-velion-org-id` IDOR). v3 Inbox is native conversation-core (no Zammad mapping; that was velionv2).
- **social-core** — both prior findings **refuted/fixed**: "HITL decorative on live writes" is false — `ensurePublishApproved` is enforced at schedule, enqueue, AND worker execution (re-reads the authoritative `social_approvals` row, blocks revoked approvals, never trusts a client flag; unit-tested). "metrics + catalog have zero v3 gateway route" is fixed — gateway `social.rs` now proxies `/metrics`, `/catalogs`, `/catalogs/:id/products`. Real per-provider publishing routed exclusively through integration-corev2 (no local provider secrets); IDOR-guarded catalog reads.
- **convex-core** — the CRITICAL hardcoded `change-me-internal-service-secret` default is **removed/fail-closed** (commit 49dc5720).
- **notification-core** — the "route mismatch" is a **caller bug, not a service bug**: the real entrypoint is `POST /api/v1/notification-requests` (live 401 without key = exists+gated); support-worker posts to the wrong `/v1/notifications` path. Feed / unread-unseen counts / preferences / channel-config / subscriber-upsert are all implemented + DB-backed (the README's "non-goals" framing is stale).
- **insight-core** — the prior AI-First "real but UNWIRED / v3 404" finding is **STALE**: it IS wired to v3 (gateway `insights.rs` + `briefs.rs`, SPA `/insights` route tree) and consumes REAL data via three genuine NATS producer legs (conversation→inbox, social→social, model-plane-agents→agents RUN_*/ACTION_*) with a no-fabrication discipline + Preview gate. Zero stub/mock markers in non-test source.

### Findings by service (2026-07-11)

| Service | State | Key findings |
|---|---|---|
| **conversation-core** :3160 | Real; DB-blocked | Inbox + ticketing workbench + HITL AI-action queue, key-gated, IDOR-clean. All 5 DB-backed endpoints 500 (postgres volume corruption). Prior WA/Messenger + ingest-fail-open bugs fixed. |
| **conversation-ingest-rs** :3161 | Real; fail-closed | Email/channel inbound bridge; auth now fail-closed (folded into conversation-core audit). |
| **convex-core** :3210/3211 (+:3006 deployer sidecar) | Live projection layer | `onOrganizationMemberRemoved` **called-but-undefined** → member removals never mirror (Phase-1 finding STILL OPEN, HIGH). Two more missing modules: `api.jobs.*` (dead legacy), `api.imports.recordCompleted` (breaks import.completed projection). Loose tsconfig hides all three from `tsc`. `change-me` default fixed. `:3006` reset-per-connection is by design (it's a `npx convex dev` deployer, not an endpoint). |
| **information-core** :3190 | Real, honest, key-gated | Read-only aggregator over Yr/Vegvesen/RSS/Bring; a live Model Plane agent-tool target (info_tools). **MEDIUM honesty bug**: `/api/v1/traffic` `trafficVolume`/`averageSpeed` are FNV-hash-fabricated from the station ID (Vegvesen only supplies id/name/coords) yet presented — and relayed by the agent — as real measurements. |
| **notification-core** :3140 | Real, broader than its README | Feed/counts/preferences/channels/subscriber all implemented + DB-backed. Route mismatch is a caller bug (real route `/api/v1/notification-requests`). DB reads 500 (volume corruption). Novu in stub mode (no key) so external delivery off in this env. `/healthz` vs `/health` mismatch will falsely mark it down on the admin dashboard. |
| **insight-core** :3163 | Real, wired, honest | Analytics/briefs, 3 real NATS producer legs, v3-wired. `/overview` 500s (volume corruption); daily-brief scheduler disabled at runtime (NOTIFICATION_CORE_URL unset in compose). |
| **leads-core** :3164 | Real, company-only guardrails proven | Brreg lead-builder (only `/enheter,/underenheter,/regnskap`, never `/roller`; `TestCompanyOnlyInvariant` proves no person data leaks onto company tables); person data contained to one `provider_leads` table (org-scoped, count-only audit, erasure). **HIGH**: all 3 Brreg endpoints 502 in-container (host reaches Brreg 200) — the Rosetta-TLS class, needs native rebuild. "Metered" = count-only audit, not an enforced quota. |
| **social-core** :3162 | Real, HITL enforced | See resolved findings above. All DB-backed endpoints 500 (volume corruption). Graph API version drift (compose v23.0 vs code v25.0, cosmetic). |
| **zammad-foundation** | Not a runtime service | Deployment/bootstrap package for a self-hosted Zammad (own isolated postgres, opt-in separate compose, NOT running). Zero Application-Plane consumers (conversation-core owns v3 Inbox), but NOT dead — velionv3 gateway Agent Console, Ingestion support-worker, and velion-web support UI target `zammad-railsserver:3000` (all degrading gracefully, tokens empty). Footguns: example `ZAMMAD_EXPOSE_PORT=8088` collides with model-plane-letta-bridge; example webhook config ships `active:true` pointing at a nonexistent `integration-core` receiver. |

### Docs

All 9 per-service core-research docs written/refreshed (2026-07-11; new: insight-core.md, leads-core.md, social-core.md). Top-level docs updated in place (update): `APPLICATION_PLANE_DEEP_DIVE.md` (added social/insight/leads to the topology), `convex-core/README.md`, `convex-core/CONVEX_INTEGRATION.md`, `convex-core/DEPLOYMENT.md` (stale `ai-core:8000`→`model-gateway:8080`, `org-core-service`→`org-core`), `convex-core/IMPLEMENTATION_COMPLETE.md`, `notification-core/README.md` (non-goals now shipped). The register's existing `update`/`review` verdicts for the convex-core docs are confirmed applied.

---

## Prior passes (preserved below)

## Live Docker verification addendum — 2026-07-10

Conversation, social, insight, leads, information, notification, and Convex-related containers were healthy. Authenticated read-only v3 probes returned HTTP 200 for tickets, social accounts, insights, and knowledge fan-out. Social account reads exposed four accounts across connected organizations; three tokens were available and one Meta token was expired. No publish, approval, invitation, ticket mutation, or billing operation was performed.

The authenticated surface is not proof of non-mock behavior: information-core traffic station identifiers/coordinates are sourced from Vegvesen, while `trafficVolume` and `averageSpeed` are deterministic hashes without a provenance flag. Social and conversation services still rely on shared internal keys plus forwarded organization/user headers, and durable content lacks complete ZDR/retention propagation. The human ticket/social dispatchers are live, but the Model Plan catalog still omits most ticket, conversation, workflow-policy, and approval operations.

This is a plane-local audit report. It focuses on Application Plane projection/runtime services and does not rework cross-plane shared docs.

## Current Shape

Application Plane owns collaborative/realtime workspace projections and notification/application-facing services. It must not become the authority for identity, billing, durable knowledge, ingestion, or reasoning. Its current service set includes Convex projection/runtime assets, conversation services, information/insight/leads/notification/social services, and Zammad foundation assets.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `go test ./...` in `conversation-core/conversation-core-go` | Pass | Checked service tests passed. |
| `go test ./...` in `information-core` | Pass | Checked service tests passed. |
| `go test ./...` in `insight-core` | Pass | Checked service tests passed. |
| `go test ./...` in `leads-core` | Pass | Checked service tests passed. |
| `go test ./...` in `notification-core` | Pass | Checked service tests passed. |
| `go test ./...` in `social-core` | Pass | Checked service tests passed. |
| `pnpm typecheck` in `convex-core` | Blocked | pnpm exits before the script because `esbuild@0.27.0` has ignored build scripts pending approval. |
| `pnpm lint` in `convex-core` | Blocked | Same pnpm ignored-build policy block. |
| `./node_modules/.bin/tsc --noEmit` in `convex-core` | Pass | Direct TypeScript check passed. |
| `./node_modules/.bin/eslint "convex/**/*.ts"` in `convex-core` | Pass | Direct ESLint check passed. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `gofmt -l conversation-core/conversation-core-go information-core insight-core leads-core notification-core social-core` | Fail | Formatting drift in conversation, information, and notification Go files. |
| Go vet over checked Go services | Pass | conversation, information, insight, leads, notification, and social pass vet. |
| `go test ./...` over checked Go services | Pass | conversation, information, insight, leads, notification, and social tests pass. |
| `staticcheck` over checked Go services | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |
| `npx -y knip --no-progress` in `convex-core` | Fail | Reports unused `convex.config.ts`, `nats-subscriber.js`, unused dependency `nats`, and a package entry-file mismatch for `index.js`. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P2 | Convex-core pnpm script gates are blocked by package build-script policy. | `pnpm typecheck` and `pnpm lint` fail with `ERR_PNPM_IGNORED_BUILDS` for `esbuild@0.27.0`. | Approve or document pnpm build-script policy, then rerun scripts through pnpm. |
| P2 | Application Go services have formatting drift despite tests and vet passing. | `gofmt -l` reports drift in conversation-core, information-core, and notification-core files. | Run `gofmt` as a mechanical cleanup and keep the gate in CI. |
| P2 | Convex-core has small but concrete dead-code/package-entry scan findings. | Knip reports unused `convex.config.ts`, `nats-subscriber.js`, unused dependency `nats`, and missing package entry `index.js`. | Confirm whether `nats-subscriber.js` is operational tooling; fix package entry metadata or remove stale files/deps. |
| P2 | Application Plane lacks a current top-level orientation doc outside service-local notes. | No `apps/Application Plane/README.md` or `APPLICATION_PLANE_ARCHITECTURE.md` was found; core-research README is service-local. | Add or designate a top-level Application Plane orientation doc if this plane is actively onboarded by agents. |
| P2 | Application projection authority needs explicit guardrails. | Cross-plane rules say Application may project/mirror state but does not own lower-plane authorities. | Add docs/tests for projected state vs canonical authority in inbox, social, conversation, and notification surfaces. |
| P2 | Application gateway naming is confusing beside the current Velion v3 Frontend gateway. | Existing core research includes `velion-gateway-rs`; current Velion v3 also has `apps/Frontend Plane/velionv3/apps/gateway`. | Clarify whether Application `velion-gateway-rs` is transitional, legacy, or still live for onboarding. |
| P3 | Go service tests are green for checked services, but Convex runtime/integration was not exercised. | Commands above. | Add Convex runtime tests or smoke checks after pnpm policy is settled. |
| P3 | Go static analysis is blocked by local analyzer/toolchain skew. | Staticcheck reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain and rerun. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Convex generated/runtime behavior | Direct `tsc` and ESLint pass, but pnpm script wrappers are blocked. | Resolve build-script approval and run official scripts plus Convex dev/codegen checks. |
| Realtime integration coverage | Go service tests passed, but NATS/Convex realtime integration was not run. | Run compose or integration smoke tests with shared bus dependencies. |
| Zammad foundation status | Not tested in this pass. | Run its package scripts or bootstrap smoke tests if it remains active. |

## Quality Gate

- Checked Go services: pass.
- Checked Go service vet: pass.
- Checked Go service format: fail.
- Convex direct typecheck/lint: pass.
- Convex pnpm scripts: blocked by ignored-build approval.
- Convex Knip: fail.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Runtime/integration stack: not run.

## Recommended Remediation Order

1. Resolve Convex pnpm ignored-build approval and rerun official scripts.
2. Clarify Application `velion-gateway-rs` vs Frontend Velion v3 gateway ownership.
3. Add or designate a top-level Application Plane orientation doc.
4. Add projection authority tests/docs for realtime/application surfaces.
5. Run Convex/runtime integration smoke tests.
