# Data Plane v2 Core Research

Generated: 2026-06-07
Updated: 2026-07-15 (final isolated Auth/User/Control, browser, HTTP/gRPC, broker, and multi-store ZDR checkpoint)

This directory contains the current service-level research notes for Data Plane v2.

## Secure-MVP current state — 2026-07-15

**Secure-MVP candidate in source; not production-ready.** The final disposable
source build passed the real Auth/User/Control Verevon journey, Playwright **2/2**,
HTTP **28/28**, gRPC **31 methods / 124 assertions**, supported signed broker
delivery/redelivery, and the six-store restrictive-ZDR final-state comparison.
The final Documents signed-claim/single/bulk/source-object guards were rebuilt
and exercised. The shared deployment was not replaced and still predates this
source.

The six-store check proves stable final state in PostgreSQL, Qdrant, Dragonfly,
Quickwit, MinIO, and NATS. NATS/Dragonfly additionally use monotonic no-write
counters; PostgreSQL/Qdrant/Quickwit/MinIO still need per-operation mutation
telemetry to exclude a transient insert-then-delete. Production scoped-broker
provisioning, credential rotation, database-backed coverage, controlled rollout,
and safe post-deploy verification remain open.
In these notes, `implemented`, `tested`, `built`, `deployed`, `reachable`, and
`effective` remain separate claims.

Sanitized program evidence recorded on 2026-07-10:

- the final Data Rust workspace gate passed **299 tests with 15 explicit
  infrastructure ignores**; strict all-target all-feature clippy passed. This
  includes 22 graph tests, 43 Quickwit tests, and 76 retrieval library tests.
  Create/Delete/Bulk retrieval gRPC mutations fail before storage.
- documents, wiki, quality, and orchestrator completed their recorded Go test
  suites with `-race`; `go vet ./...` passed. Documents and wiki also recorded
  no reachable `govulncheck` finding.
- the safe HTTP matrix harness contract passes and the rebuilt disposable stack
  passed all 28 endpoint checks covering seven route families and four
  credential shapes. This is isolated endpoint evidence, not shared deployment
  evidence.
- measured auth-package coverage is documents 95.5%, quality 82.8%,
  orchestrator 91.1%, and wiki 85.2%.
- Data and Model RustSec audit scripts pass; Quarry's production-graph audit has
  no known vulnerability (two unmaintained-crate warnings remain). No full Rust
  coverage percentage was measured, so the >=80% gate is not proven for every
  changed Rust security module.
- Control's versioned authorization decision path has 41 focused tests and
  measured controller/token coverage of 88.46% statements, 84% branches, 95%
  functions, and 88.2% lines. After the production dependency refresh, the
  focused policy/token suites passed 30 tests, the build passed, and both pnpm
  and npm production audits reported no known vulnerabilities.
- the final security review also closed a whole-signing-key-directory mount,
  static-service user impersonation/unbounded authz, standard Model chat/embed
  ZDR downgrade, and host-exposed diagnostic/optional dependency defaults.
- Auth Core user/service Model tokens now require and sign restrictive
  `zdr:true`; Model computes signed-claim OR request posture. No caller can
  downgrade it while an authoritative organization policy is absent.
- Model service-token issuance now emits a tenant-bound Control audit event
  containing its bounded reason/scopes and returns 503 if the audit publisher
  is unavailable. Audit-core persistence remains runtime-unproven. Auth Core's
  recorded full suite passes 99/99 with build and no-fix lint; focused changed
  security modules exceed 80% coverage.
- Model Gateway explicitly distinguishes scoped services from users: canonical
  service ID/reason plus exact `models:invoke` is accepted only on POST chat and
  embeddings; all user/delegated/Data/session/retrieval routes deny services.

Current release blockers:

- production NATS subject ACLs/scoped credentials, including the Documents GDPR
  durable, require coordinated provisioning and post-deploy proof; the real
  disposable consumer's scoped bind/ACL/ACK/redelivery/recovery matrix is green;
- strict mutation telemetry is missing for four of the six ZDR stores;
- Quickwit destructive execution remains 501 pending trustworthy task completion
  and crash/retry proof; no destructive action was tested;
- database-backed coverage is below 80% for Quickwit jobs, quality evals,
  orchestrator jobs, and the full wiki events package;
- shared images still predate source; surfaced local credentials require rotation
  before a controlled rollout and safe synthetic-tenant verification;
- unverified Execution/Model listeners and unsupported unsigned consumers must
  remain disabled, and grant mutation remains unavailable until resource-owner
  authorization exists.

The older live-audit sections in individual files are preserved as historical
root-cause evidence. Where they conflict with this section, they are superseded;
their prior deployment claims do not describe the current dirty worktree or a
verified rebuilt runtime.

Latest plane audit:

- `plane-audit-2026-07-10.md`

See also, at the plane root: `../../DATA_PLANE_STATUS.md` (current-state snapshot) and `../../DATA_PLANE_ROADMAP.md` (fix plan).

**Historical gate note**: the pre-fix live audit proved credential and tenant
boundary failures across multiple services. The customer-specific evidence is
redacted here; the finding remains release evidence, while the old runtime state
is superseded by the un-deployed source fixes described above.

Runtime-service notes in scope (current source overlays and final isolated
evidence added; no shared deployment claim):

- `documents-api-go.md`
- `index-engine-rs.md`
- `embedding-engine-rs.md`
- `retrieval-engine-rs.md`
- `graph-index-rs.md`
- `quickwit-adapter-rs.md`
- `wiki-store-go.md`
- `data-orchestrator-go.md`
- `data-quality-go.md`

Optional / feature-flagged, not deployed in `docker-compose.yml` by design:

- `colqwen-reranker.md` (new 2026-07-10 — real, wired ColQwen2.5 MaxSim reranker, off by default, meant for a separate GPU host)

Support and scaffold note:

- `retrieval-eval-py.md` (confirmed 2026-07-10: a genuinely empty, never-git-tracked directory — not a paused build, just nothing there)
