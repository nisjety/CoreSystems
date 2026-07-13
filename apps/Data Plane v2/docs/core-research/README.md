# Data Plane v2 Core Research

Generated: 2026-06-07
Updated: 2026-07-11 (secure-MVP remediation and final source verification; supersedes the earlier passes)

This directory contains the current service-level research notes for Data Plane v2.

## Secure-MVP current state — 2026-07-11

**Not production-ready.** Security fixes are implemented and test-proven in the
dirty source worktree. An earlier checkpoint built the migrator plus nine Data
service images with verified revision/build labels, but later source changes have
not been rebuilt because Docker's content store now returns blob input/output
errors. Current source has not been deployed, reached, or shown effective through
the isolated Docker/live matrix. In these
notes, `implemented` means present in source, `tested` means the named local test
command passed, and neither word implies `built`, `deployed`, `reachable`, or
`effective` in a running environment.

Sanitized program evidence recorded on 2026-07-10:

- the final Data Rust workspace gate passed **299 tests with 15 explicit
  infrastructure ignores**; strict all-target all-feature clippy passed. This
  includes 22 graph tests, 43 Quickwit tests, and 76 retrieval library tests.
  Create/Delete/Bulk retrieval gRPC mutations fail before storage.
- documents, wiki, quality, and orchestrator completed their recorded Go test
  suites with `-race`; `go vet ./...` passed. Documents and wiki also recorded
  no reachable `govulncheck` finding.
- the safe HTTP matrix harness has a 28/28 synthetic contract result covering
  seven route families and four credential shapes. It has **not** run against a
  rebuilt Compose stack, so it is tooling evidence, not endpoint evidence.
- measured auth-package coverage is documents 95.4%, quality 82.8%,
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
  is unavailable. Audit-core persistence remains runtime-unproven. Auth Core
  passes 76 tests and builds, but its repository-wide lint baseline and
  controller-only branch coverage remain open gates.
- Model Gateway explicitly distinguishes scoped services from users: canonical
  service ID/reason plus exact `models:invoke` is accepted only on POST chat and
  embeddings; all user/delegated/Data/session/retrieval routes deny services.

Current release blockers:

- unsigned legacy consumers are fail-closed/disabled by default in embedding,
  index, graph, Quickwit adapter, documents GDPR, and orchestrator cost paths.
  This contains untrusted asynchronous mutation but leaves the corresponding
  pipeline functions ineffective until signed, caller-scoped events and NATS
  authorization are implemented.
- Quickwit rebuild mutations return 501; only an authenticated, tenant-scoped
  dry-run preview exists. Durable job state, audit, rate limiting, approval,
  resumability, and concurrency protection are not complete.
- retrieval and Control now share a versioned decision contract in source, but
  a real authorized bearer has not been accepted end to end by rebuilt services.
- legacy static Control credentials intentionally cannot perform delegated-user
  or grant/visibility calls. Signed caller+user+tenant+resource delegation is
  required before explicit sharing is functional again.
- Model Gateway, Execution Core, and Inference Core legacy gRPC listeners are
  contained off by default behind a surface-specific gate plus the insecure-dev
  gate. Verified replacement authentication and tenant/ownership pinning are
  not yet proven end to end, so Data-to-Model service identity remains a release
  boundary.
- the revised Data images built locally with matching non-placeholder provenance
  labels, but no restart/deployment, private-network reachability check, or
  isolated live auth/ZDR/admin matrix has completed.

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

Runtime-service notes in scope (current source overlays added; rebuilt live state
not yet verified):

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
