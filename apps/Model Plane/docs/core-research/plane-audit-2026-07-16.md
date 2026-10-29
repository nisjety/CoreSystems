# Model Plane Production-Readiness Audit — 2026-07-16

Scope: `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane`

Evidence cutoff: initial runtime inventory at 2026-07-16 09:47 CEST
(Europe/Oslo). Source baseline: branch `main`, commit
`ae3ee041e13d482b20e5883e79044e714bf3d216`, plus a large pre-existing dirty
worktree. Final source verification was run at 2026-07-16 15:10 CEST and is
recorded below. Live reconciliation was completed at 2026-07-16 20:05 CEST.
After the initial read-only audit, the user authorized a local Docker build and
rollout. No commit, push, volume/database deletion, daemon restart, prune,
credential reuse, or paid/destructive provider action was performed.

This is a delta from `plane-audit-2026-07-13.md`, which remains historical
evidence. Where the reports differ, this report's later dated evidence wins.

## Executive verdict

**Local integration deployment operational; not yet production-ready as an MVP
and not enterprise-ready. Do not promote the dirty-tree stack as production.**

At 20:05 CEST, all 21 Model Compose containers were running; all 19 containers
with health checks were healthy, and OTEL Collector plus Temporal UI were
running without health checks. Eleven app images are dirty-tree `working-tree`
builds, unsigned and unattested. Gateway, inference, and execution gRPC are
loopback-reachable and authenticated. This is live local integration evidence,
not an accepted candidate or rollback artifact. No signed immutable candidate
or separate known-good rollback exists.

The source now contains substantial release-blocker fixes:

- additive gateway `:9090` and inference `:9092` gRPC listeners, with `/readyz`
  failing until the socket is bound;
- artifact-v3 release tooling that snapshots release inputs and image archives
  under a signed root manifest, rejects a dirty source tree, and requires
  signing/verification keys, compatibility gates, and an external allowlisted
  runtime environment in release mode;
- authenticated, tenant-scoped cost source paths and scoped NATS credentials;
- a quarantined ambient gateway `SendMessage` RPC that returns
  `FAILED_PRECONDITION` before publication, with broad gateway NATS grants
  removed and legitimate telemetry bound to per-run event subjects;
- a content-free, idempotent approval-delivery outbox with user-scoped durable
  approval read-through plus claim/lease/retry/terminal primitives;
- an additive managed-run lifecycle with content-free terminalization
  obligation/receipt, lease recovery, scoped service heartbeat/finalization,
  opaque keyed-MAC start identities, and a legacy terminal-step bypass reject;
- browser lifecycle status propagation that permits `completed` only for an
  explicit successful loop result; denied, timed-out, and exhausted work fails,
  while cancelled/aborted work invokes Session Core cancellation rather than a
  synthetic completion;
- capability policy at both direct-step and agent dispatch, before tool
  execution, with unavailable/unattested capabilities denied and
  approval-required capabilities returning `ask`; a dedicated global-only
  health-attestation path cannot be redirected to a tenant row;
- optional Letta `POST /v1/tools/search` ranking that can only reorder the exact
  locally authorized tool intersection and is disabled for ZDR callers;
- typed hybrid knowledge-search results, explicit degraded/no-results states,
  exact-repeat suppression, and bounded reformulation guidance for multi-step
  retrieval; and
- issuer-monotonic retention policy, signed-ZDR preservation at execution
  ingress/delegation, ZDR redaction of durable tool-step payloads, strict
  deployment-owned exact-org interactive retention configuration in Auth Core,
  and fail-closed inference modalities before unattested provider I/O.

These fixes now have selected live local evidence, but not immutable production
release evidence. Approval grants are durable and can enter
claim/lease/retry/terminal states, but no restartable continuation descriptor,
authenticated dispatcher, or successful execution receipt exists; the execution
RPC therefore correctly returns `Unavailable` instead of claiming a resume. All
execution capabilities begin unavailable with `health_not_attested`, and no
global health reporter is configured or attested, so a deploy requires that
live proof. NATS has distinct named principals and no generic `model-runtime`
principal, but it has no TLS/mTLS/workload identity. Interactive credentials
default to all-ZDR; only a strict deployment-owned exact-org policy can permit
another posture, and none is provisioned. No provider deployment has
independently verified ZDR eligibility. Ordinary external inference therefore
must remain unavailable until governance supplies and evidences that exact
policy or an independently verified ZDR provider. Weakening the downstream gate
is not an acceptable availability fix.

## Evidence-state definitions

| State | Meaning in this report |
|---|---|
| **Source-verified** | Source and repeatable local tests support the claim; this alone is not deployment evidence. |
| **Live-verified** | Reproduced against a running Model Plane deployment on 2026-07-16. |
| **Blocked** | A release property is absent or lacks the required proof. |
| **Enterprise-next** | Deliberately deferred until every MVP acceptance criterion passes. |

## Release-blocker results

### Protocol listeners and rollback

Gateway and inference now bind their additive gRPC services and expose
listener-aware readiness in source. Compose healthchecks use `/readyz` rather
than liveness. Contract tests cover the release overlay and canonical
`deploy/.env` path.

Artifact format v3 covers all 21 Compose services and 20 unique images, builds
one Docker archive per unique image, checksums it, writes `images.lock.env`
using content-addressed `sha256:` image IDs, and snapshots Compose, NATS, OTEL,
seccomp, migrations, and cross-plane revision inputs beneath a signed root
manifest. Every Compose substitution is classified as signed public, external
secret, or artifact-owned; unknown credential/image-shaped keys fail closed.
The independently signed Auth issuer and JWKS URL require a non-placeholder
identity-evidence digest. An asserted Azure ZDR route additionally binds the
endpoint, API version, region/residency, chat/embedding catalogs, provider
order, and a non-placeholder ZDR-evidence digest. External key, evidence, and
runtime files are opened without following symlinks, bounded, ownership/mode
checked, race checked, and consumed only from private snapshots. Docker receives
a temporary 0600 secret-only runtime snapshot; signed public policy and the
immutable image lock retain precedence. The copied runner accepts only fixed
config validation or fixed no-build deployment, so caller-selected Compose,
environment, profile, service, and command arguments are rejected before
Docker. The archive contains no runtime credential values.

Candidate validation now requires a separately signed rollback with a distinct
source revision and distinct normalized image payload. Candidate-signed runtime
evidence binds the rollback root, secret schema, supplied keyset, and an opaque
non-placeholder secret-manager/version-reference digest. The current verifier
uses private signed-member snapshots and a fixed adapter, so it neither executes
mutable predecessor paths nor assumes an older v3 artifact already implements
the new verifier. The result is explicitly `rollback-config-renderable`; live
credentials, data compatibility, health, cutover, and rollback success remain
separate operator/canary gates.
`deploy/docker-compose.release.yml` removes
application `build:` stanzas and rejects mutable tags; direct workspace
production Compose is refused. The workflow deliberately refuses the current
dirty tree. Therefore **no immutable artifact exists yet**, and no rollback is
executable. That refusal is a passing safety property, not a completed rollback
deliverable.

### Authorization and NATS

Cost HTTP authorization and tenant/user scoping are implemented in source. Live
no-auth, malformed-bearer, and forged-scope cases deny; a legitimate valid
scoped Cost read is still outstanding. The
NATS configuration uses distinct named principals for actual Model Plane
workloads and removes the generic `model-runtime` principal; cost-core remains
restricted to its usage-consumer role. The broker and scoped principals are
running and connected locally. NATS TLS, mTLS, and workload identity
are not configured, so cryptographic peer-identity proof remains absent.

### Approvals and HITL

Session Core transactionally compare-and-sets a grant and inserts exactly one
content-free `approval_delivery_outbox` row. Gateway cache misses read the
durable approval and re-bind it to the authenticated organization/user before
use. The outbox has claim/lease, bounded retry/backoff, poison/terminal, and
acknowledgement-state primitives. Manual resume is distinct from approval
resume, so a generic resume cannot move `AwaitingApproval` work.

The required continuation dispatcher does not exist. A prior in-memory status
flip would not restart the exited agent loop, so it is no longer reported as a
successful resume. There is no restartable continuation descriptor or durable
success receipt showing that an authenticated dispatcher accepted the work.
After validating the durable grant, Execution Core returns gRPC `Unavailable`
and leaves delivery pending/retriable rather than falsely completed. This
preserves the real HITL gate and avoids false completion, but approval delivery
remains a P0 release blocker until a dispatcher and continuation contract pass
crash-window tests.

A generic continuation cannot safely be introduced as a content-free shortcut:
the current pause path drops exact tool arguments, and re-planning after a grant
can change or duplicate an external effect. It requires either an encrypted
durable exact-effect descriptor or a downstream-owned immutable intent/receipt
contract. Encrypted content is still retention: ZDR identities must be rejected
before descriptor creation. There is no evidenced persistent-posture policy,
envelope-encryption/KMS lifecycle, or downstream tool idempotency/receipt proof.
Consequently approval-required external-effect capabilities remain unavailable;
grant-to-quarantine is an intentional secure-MVP state, not a failed resume.

### Managed-run terminalization

Known direct-inference and browser outcomes preserve their terminal meaning in
source: a browser resource limit/timeout/denial cannot become `completed`, and
cancellation/abort uses `CancelRun`. The new Session Core lifecycle is additive:
`StartManagedRun` atomically binds the run to a keyed opaque start identity and
a metadata-only obligation; `RecordTerminalOutcome` is service-authenticated,
derives owner scope from the run, and returns an idempotent receipt; and
`HeartbeatManagedRun` supports producer liveness. Migration `0015` and the
leased recovery worker store/reconcile the obligation without raw prompt,
outcome, or idempotency content.

Gateway and Execution use narrowly scoped Session Core service tokens. Direct
HTTP, gRPC, SSE, and browser paths now start/heartbeat/finalize against the
managed lifecycle. The legacy terminal `CompleteStep` control path is rejected
for managed runs, closing the terminalization bypass. SSE must retain
`finalization_pending`, not synthesize success, when a final receipt is absent.
Existing `StartRun` remains additive/backward compatible in source.

This is not release evidence: migrations through `0015` and the relevant tables
exist locally, and Auth Core has issued a runtime service token, but the full
crash/response-loss/cancel-race/recovery/caller-compatibility matrix lacks live
proof. It remains a P0 deployment gate, separate from the missing approval
continuation dispatcher. Producers still must not write the Session database.

### Capability authority

Execution Core calls Capability Core before every direct and agentic dispatch.
Policy `allow` proceeds, `ask` produces the existing approval-required state,
and deny/outage fail closed before tool code runs. Server-owned exact tool to
capability bindings prevent callers from selecting an authority mapping.
Dynamic MCP tools remain denied until a registry-backed binding exists.

Migration `0008_execution_dispatch_capabilities` seeds known dispatch names
but sets them to unavailable with reason `health_not_attested`; it does not
fabricate runtime health. The global catalog can be attested only through the
dedicated global-health scope/path, not through a tenant reporter. No reporter
is configured or attested, so a newly deployed stack will deny tools. The
policy and dispatch enforcement are source passes; cross-mode offers and live
attestation remain blocked.

### Letta and agentic retrieval

Letta's `POST /v1/tools/search` searches **tool definitions**, not memory
passages. The optional Capability Core client validates a fixed endpoint,
rejects unsafe configuration and redirects, bounds timeout/response size, and
uses Letta results only to rank an exact intersection with tenant-scoped local
tool rows. It never grants policy or execution authority, falls back to local
ranking on outage, and sends no external query for ZDR or retention-unspecified
principals. No live Letta endpoint is configured or verified.

Memory semantic retrieval is a separate Session Core → letta-bridge path. It
now preserves its detailed outcome in default context assembly: verified empty,
entries, or a bounded `DEGRADED_LETTA_*` reason. RPC refusal and timeout are
counted with content-free metrics and structured warnings rather than silently
collapsed into no memory rows. Compose now checks bridge liveness at `/healthz`,
while `/readyz` remains the semantic-capability signal. The response stays
backward compatible, so live semantic behavior and Frontend user-facing
degradation remain unverified. Tool-definition ranking must not be presented as
restored semantic memory.

The existing ReAct-style agent loop already supports multiple inference/tool
rounds. Source changes make knowledge retrieval explicitly agentic within the
MVP contract: results are typed JSON with `ok`, `low_confidence`, `no_results`,
or `degraded`; the prompt directs materially different reformulation and
backtracking; exact repeated searches are suppressed; and a dependency failure
cannot masquerade as an empty corpus. The only supported route is Data Plane's
server-managed `hybrid` search. Raw SQL, tabular, graph, vector-only, and MCP
retrieval are **not configured** and are not advertised. Adding them requires
typed owner-plane contracts, parameterized/allowlisted query DSLs, tenant
authorization, cost/row/time bounds, provenance, and ZDR tests—never raw model-
authored SQL or Cypher.

### ZDR and provider availability

Caller intent may tighten retention but cannot loosen issuer policy. Signed ZDR
is preserved at execution ingress and through delegated identity, so a caller
cannot request `false` to downgrade a signed `true` posture. Service principals
can receive non-ZDR posture only from deployment-owned exact audience
configuration; request bodies/headers cannot choose it. Auth Core now makes an
interactive exception possible only through a strict deployment-owned exact-org
configuration; absent policy is all-ZDR and malformed policy fails closed. Tool
output is available ephemerally to the current reasoning round, while durable
step records retain only classification and success/error metadata under ZDR.
Inference rejects each unattested modality before provider I/O.

No organization policy is provisioned and no provider deployment has independent
evidence of ZDR eligibility. Thus the secure source posture blocks ordinary
external inference. Product governance must provision and evidence either the
exact organization retention decision or a verified ZDR-capable provider. Until
then the correct status is `unavailable`, not an implicit downgrade and not a
fabricated provider success.

## Evidence matrix

The rows below retain the initial read-only/source history and add the later
user-authorized live local rollout. `Live local` is not immutable production
evidence.

| Claim | Command or source | Expected | Actual | Timestamp (Europe/Oslo) | Classification | Limitations |
|---|---|---|---|---|---|---|
| Initial runtime inventory — superseded | read-only `docker ps` and `lsof` for `:9090`, `:9092`, and `:9093` | Inventory without mutation | No Model Plane container or listener at the initial cutoff | 2026-07-16 16:02 CEST | Real local Docker/host | Historical; superseded by the authorized rollout |
| Initial Compose preflight — superseded | `./scripts/compose.sh config --services`; read-only volume and dependency inventory | Validate configuration without a build/start | Blocked by absent scoped NATS config and unhealthy/unreachable dependencies | 2026-07-16 17:52 CEST | Real local config/Docker host | Historical; runtime-only credentials/network repair later allowed local start |
| Local Docker integration state | `docker ps`; `docker inspect` health/restart state | 21 services running; checked services healthy | PASS: 21 running; 19/19 health-checked healthy; OTEL and Temporal UI running without checks; restart count zero for Model apps | 2026-07-16 20:05 CEST; reconfirmed 22:25 CEST | Real live local Docker | Dirty-tree images; not a production release |
| Live image provenance | `docker image inspect` labels/IDs | Trace exact running images | 11 application images labeled `working-tree-ae3ee041e13d`; unsigned and unattested | 2026-07-16 19:48–20:05 CEST | Real local images | Commit label does not attest dirty-tree file contents; no retained artifact |
| Gateway/inference/execution gRPC | loopback TCP plus descriptor-specific `grpcurl` on `:9090/:9092/:9093` | Listeners reachable; no auth denied | PASS: all reachable; unauthenticated calls rejected | 2026-07-16 20:03 CEST | Real live local gRPC | Production overlay removes host ports; old/new client matrix still open |
| Control Auth and Session service identity | service-token exchange; canonical claim check; `RoutingPolicy/GetPolicy`; file-registry focused suites/build/Compose parse | Negative issuance denies; exact principal/token succeeds; production secret input is file-backed and fixed-tenant | PASS live issuance/session call; PASS source 93/93 and 91.58% registry-module lines; non-development `allowAnyOrg` file rejects | 2026-07-16 19:50–20:05 CEST plus later source checks | Real live Control/Model plus source/test | Running development registry remains runtime-only/dynamic; fixed-tenant production file not deployed |
| Data cross-plane reachability | Model-side DNS/TCP/HTTP probes to retrieval, graph, wiki | Shared-bus services ready | PASS: readiness 200 and gRPC TCP reachable for `50052/50053/50054` | 2026-07-16 19:39–19:46 CEST | Real live Data/Model | Business retrieval answer/citation E2E still open; Quarry absent |
| Live authorization negatives | Gateway, Cost, Capability HTTP; Session/Letta gRPC; forged approval decisions | No auth, malformed bearer, and forged scope deny | PASS: HTTP 401 and gRPC `Unauthenticated`; approval counts unchanged | 2026-07-16 19:46 CEST | Real live local | Valid scoped Cost read remains outstanding |
| Capability dispatch authority | Auth-issued execution token; `EvaluatePolicy` exact and wrong org | Exact identity reaches policy; unattested/cross-org deny | PASS: `cap.retrieval.query` denied `health_not_attested`; wrong org denied | 2026-07-16 19:50 CEST | Real live local | 27 enabled rows all unavailable; no reporter/allow-path proof |
| ZDR/provider contradiction | Auth-issued Data retrieval token; Inference `Infer` with request `zdr=false`; logs by request ID | Signed ZDR wins and no provider call occurs | PASS: gRPC `FailedPrecondition`; two providers skipped; zero provider attempts | 2026-07-16 20:05 CEST | Real live Control/Data/Model | Safe denial only; ordinary chat still has no eligible provider |
| Letta degraded state | `GET :8088/healthz` and `/readyz` | Liveness separate from semantic readiness | 200 live; 503 `DEGRADED_SEMANTIC_UNVERIFIED` ready | 2026-07-16 19:46 CEST | Real live local | No authenticated semantic success; provider-backed embedding route is not ZDR-attested |
| Durable outbox/migrations | read-only Postgres schema/state queries | Required tables/migrations visible without side effects | Migrations through `0015`; approval and terminalization outbox tables exist; four requested approvals and zero delivery rows | 2026-07-16 19:46 CEST | Real live local Postgres | No positive approval continuation, crash recovery, or rollback proof |
| Control/Data audit reachability | Auth JWKS; Audit health/ready; shared-bus consumers | Model dependencies reachable | JWKS/Auth and Audit health 200; Model audit/usage consumers ready; global Audit ready 503 only because Application NATS is absent | 2026-07-16 19:39–19:46 CEST | Real live cross-plane | Application Plane absence remains external to this rollout |
| Rust release-critical suites | `cargo test -q -p model-gateway -p inference-core -p execution-core -p session-core --all-targets` | All source suites pass | PASS: Execution 225 library + 17 integration; Inference 127 + 15; Gateway 409 + 41; Session 119 + 17; six Session DB and one Quarry live test ignored | 2026-07-16 15:10 CEST | Real source/test | No container, Postgres, or cross-plane client probe |
| Incremental Rust validation | Gateway `--lib`; Execution `--lib`; Session `--bin`; Inference `--all-targets`; package format/checks | Current hardening compiles and regressions pass | PASS: Gateway 420; Execution 228; Session 125 passed/7 ignored; Inference 127 library + 15 integration | 2026-07-16 after 15:10 CEST | Real source/test | Gateway integration/periodic ticker and full browser-dispatch clock tests, Session release-DB cases, and all live probes remain outstanding |
| Rust format/lint | `cargo fmt --all -- --check`; targeted `cargo clippy` with `-D warnings` | Formatting and configured warning gate pass | PASS | 2026-07-16 15:10 CEST | Real source/tooling | Clippy command carries documented legacy rule allowances; raw all-lints debt is not claimed clean |
| Execution capability gate | Rust suite plus live Capability policy probes | deny/ask/outage stop before dispatch; allow proceeds | PASS in source; live valid identity denied `health_not_attested` and wrong organization denied | 2026-07-16 20:05 CEST | Real source/test and live local | No trusted health reporter or healthy allow-path proof |
| Approval outbox/read-through | Session and gateway targets plus read-only local Postgres inspection | one content-free delivery per grant; owner-bound durable cache fill; undurable pause rejects before state/event | PASS in source; migrations through `0015`, four requested approvals, zero delivery rows; no decision/effect issued | 2026-07-16 19:46 CEST | Real source/test and live local schema | No release Postgres or continuation descriptor/dispatcher/success receipt |
| Browser terminal classification | Execution/browser gateway target tests plus live container health | Only explicit success completes; denial/timeout/exhaustion fail; cancel/abort cancel | PASS in focused/all-target source tests; Browser Broker is healthy locally | 2026-07-16 20:05 CEST | Real source/test and live local health | No authenticated browser action E2E, Quarry, or durable terminalization recovery proof |
| Managed-run terminalization | Session Core focused unit/bin tests and disposable PostgreSQL legacy-terminal regression; Gateway full 420-test library suite plus focused lifecycle/E2E tests | Additive obligation/receipt/heartbeat, scoped producers, no legacy terminal bypass, no raw start-key persistence | Source pass: lifecycle/migration/worker and producer integration implemented; legacy managed-run `CompleteStep` rejected | 2026-07-16 after 15:10 CEST | Real source/test; isolated disposable PostgreSQL for one regression | No release migration, runtime Auth Core token, live recovery/caller-compatibility, or rollback proof; periodic five-minute ticker/full browser background dispatch lacks clock-driven coverage |
| Ambient NATS publish quarantine | `cargo test -p model-gateway send_message_is_quarantined --lib` | public RPC cannot emit raw wildcard NATS messages | PASS: RPC and direct handler reject with `FAILED_PRECONDITION`; telemetry uses the bound run-event subject | 2026-07-16 12:04 CEST | Real source/test | Scoped broker/principals are live; the quarantine RPC and hostile ACL matrix were not live-exercised |
| Capability/Letta policy | `go test ./...`; `go vet ./...` in capability-core | Authz, fail-closed availability, safe local intersection, bounded Letta client | PASS; new global-health coverage: `GetGlobal` 100%, `AttestAvailabilityGlobal` 90% | 2026-07-16 15:10 CEST | Real source/test | Tool-definition search uses a test server; live semantic memory is separately degraded |
| Cost authorization | `go test ./...`; `go vet ./...`; live HTTP negatives | Unauth/cross-tenant deny; scoped valid access passes | Source PASS; live missing/malformed bearer and forged identity headers deny | 2026-07-16 20:05 CEST | Real source/test and live local | Legitimate positive scoped live read remains outstanding |
| Orchestration event identity | `go test ./...`; `go vet ./...`; local NATS connection inspection | Scoped event paths compile and regressions pass | PASS; scoped Model principals are connected locally | 2026-07-16 20:05 CEST | Real source/test and live local | No TLS/workload identity or hostile-principal proof |
| Auth retention issuance | Focused Auth Core Jest retention/token suite; `pnpm build` | Caller cannot select persistent posture; absent policy is all-ZDR; strict deployment policy alone selects an exact org | PASS, 72 focused tests; production build passes | 2026-07-16 after 15:10 CEST | Real source/test, cross-plane | No provisioned organization policy or provider proof |
| Frontend contract build | `next typegen`; `pnpm typecheck`; `pnpm build` in Frontend Plane/verevon | Generated Model Plane contract and production UI build pass | PASS; 0 first-party type errors (354 ignored third-party `@blocksuite` errors) | 2026-07-16 15:10 CEST | Real source/tooling, cross-plane | Does not prove tool-mode UX in a live browser |
| Scoped NATS config | all Model Plane `scripts/tests/*.sh` contract tests plus live connections | Named credentials and subject least privilege parse | PASS; scoped principals connected | 2026-07-16 20:05 CEST | Real source/config and live local | No TLS/workload identity or hostile-principal proof |
| Immutable release workflow | `scripts/tests/release-contract-test.sh`; `release-runtime-partition-test.sh`; `release-input-security-test.sh`; Bash syntax | Signed artifact-v3 snapshot; dirty source, unsafe inputs, placeholder evidence, mutable candidate/predecessor paths, and unsigned runtime/Compose overrides cannot become release input | PASS on final tree: fixed operations, public/secret/artifact partition, independent Auth/JWKS and full ZDR binding, O_NOFOLLOW private snapshots, snapshot-first candidate binding, distinct rollback revision/image payload, deterministic gate-swap/mutation rejection, legacy-v3 adapter, and config-renderability evidence | 2026-07-16 22:24 CEST | Real local source/tooling | No signed candidate/rollback archive, live secret proof, restore/cutover rehearsal, or immutable artifact store |
| Generated protocol | `buf generate` in `proto/` | Checked-in generated bindings retain additive contracts | PASS | 2026-07-16 15:10 CEST | Real source/tooling | `buf lint` fails legacy package/input violations; `buf breaking` lacks a usable baseline |
| Critical coverage | `cargo llvm-cov -p model-gateway -p inference-core -p execution-core -p session-core --all-targets --summary-only` | Changed critical modules reach 80% or gaps are explicit | Not sufficient: 59.52% total lines; browser agent 77.90%, runtime loop 65.43%, tool bridge 70.67%, browser run 62.49%, session flow 64.53%, SSE 56.43%, approval delivery 35.32% | 2026-07-16 15:10 CEST | Real source/test | Session database paths need a real Postgres run; release authority must close or accept gaps |
| ZDR tool persistence | Execution Core tests | Durable step output/error contains no content under ZDR | PASS | 2026-07-16 12:04 CEST | Real source/test | Prompt/session/cache/trace/provider live E2E still absent |
| Letta memory degradation | Session Core context/adapter tests plus live health/readiness | Empty differs from RPC/timeout degradation; liveness differs from semantic readiness | PASS: source distinction plus live 200 liveness and 503 `DEGRADED_SEMANTIC_UNVERIFIED` readiness | 2026-07-16 20:05 CEST | Real source/test and live local | No authenticated semantic success or Frontend degraded UX; tool search is not memory search |
| Credential exposure response | prior read-only process inspection | No secrets printed; exposed values revoked after secure migration | Values omitted and not reused; rotation not performed | 2026-07-16 carry-forward | Real operator incident | Operator rotation/revocation required |

Test counts are runner output, not coverage claims. Database-gated ignored tests,
legacy Buf debt, and absent live systems are reported rather than hidden.

## Finding register

Deployment states: **source-fixed** means tested source only; **partial** means
important implementation remains; **blocked** means no deploy authorization or
required external evidence.

| ID | Severity | Exploitability / impact | Owner | Fix | Regression test | Deployment | Verification |
|---|---|---|---|---|---|---|---|
| MP-001 | P0 | Protocol removal could break deployed clients during promotion | Runtime + release | Additive listeners, loopback integration probes, production private-network contract | Gateway/inference/execution listener contract | live-local; production blocked | Live listeners/auth pass; immutable old/new caller release proof absent |
| MP-002 | P0 | Rollback would require an unverified rebuild | Release engineering | Produce, verify, retain, and rehearse signed artifact-v3 candidate and separate rollback artifacts with external runtime-env validation | Release contract + archive verify/restore | tooling source-fixed; blocked | Dirty-tree refusal passes; no artifact |
| MP-003 | Critical | Cost data/operations risk cross-tenant disclosure if deployed without current auth | cost-core | Verified identity and tenant/user/service scope | Go unit/auth matrix, vet, live negative probes | source-fixed; live partial | Live negatives pass; valid scoped read outstanding |
| MP-004 | Critical | Generic/in-memory approval resume could bypass or falsely report HITL continuation | session/execution/gateway | Keep identifier-only grant queue quarantined; before enablement, add encrypted exact-effect descriptor or verified immutable intent, scoped dispatcher, and receipt | Wrong-user/replay/terminal/no-false-resume, ZDR rejection, encrypted-descriptor, and crash-window tests | partial; operator-gated | No policy/KMS/downstream receipt evidence; no dispatcher/descriptor |
| MP-005 | Critical | Tool dispatch could bypass catalog/policy | execution + capability | Mandatory policy call before every dispatch; exact server binding; global-only health attestation | deny/ask/outage-before-dispatch tests plus live policy probe | live fail-closed; blocked to enable | Valid identity denies unattested and wrong-org; no reporter/allow path |
| MP-006 | High | Unauthenticated/overbroad NATS peers could forge usage/events | Platform + event owners | Distinct named principals/subjects, no generic `model-runtime`; add TLS and workload identity | Config contract, parser, and live connection validation | partial; blocked | Broker/principals live; no TLS/workload identity or hostile-peer proof |
| MP-007 | High | Tool catalogs can claim unhealthy/unavailable tools | Capability + Frontend owners | One status/reason contract at offer, policy, and dispatch | Capability policy and future cross-mode E2E | partial | Dispatch is enforced; UI/live derivation unproved |
| MP-008 | High | External Letta ranking could disclose a query or elevate a remote tool name | capability-core | Non-ZDR-only call; exact local intersection; remote result never authorizes | Redirect/bounds/ZDR/intersection/outage tests | source-fixed; optional | No live Letta; API ranks tools only |
| MP-009 | High | Semantic memory could silently look empty when dependency is down | session/letta-bridge | Bounded degraded status/metrics and separate liveness/readiness; verify real memory or UX degradation | Empty/RPC/timeout distinction, Compose health, live readiness | live degraded; release partial | 200/503 explicit degradation; no authenticated semantic success or Frontend UX |
| MP-010 | High | Default all-ZDR posture plus no provisioned org policy or verified provider makes inference unavailable | Identity policy + provider owner | Strict deployment-owned exact-org policy or independently verified ZDR provider; preserve signed ZDR and reject unattested modalities pre-I/O | Retention policy/token tests plus live no-provider probe | fail-closed live; availability blocked | Signed ZDR override and zero provider attempts proven; no eligible route |
| MP-011 | High | ZDR content may persist in tools, sessions, memory, traces, caches, or bridges | All content owners | Propagate one monotonic posture and redact/reject all durable/external boundaries | Tool-step test passes; full ZDR E2E required | partial; blocked | Tool persistence source pass only |
| MP-012 | High | Raw model-authored SQL/graph/MCP retrieval would create injection, exfiltration, and cost risk | Data/Model capability owners | Typed allowlisted owner-plane contracts with row/time/cost/provenance limits | Negative DSL, tenant, ZDR, budget, timeout tests | not configured | Honest source contract |
| MP-013 | Medium | Agent can waste rounds or treat retrieval outage as no evidence | execution-core | Typed result envelope, reformulation guidance, exact duplicate suppression | Knowledge tool/agent loop tests | source-fixed | Only hybrid route; no live RAG E2E |
| MP-014 | Medium | Legacy proto lint debt weakens compatibility automation | Contract owners | Stage compatible package/input cleanup and establish real breaking baseline | `buf lint` and `buf breaking` | open | Generation passes; quality gates fail |
| MP-015 | High | Third-party connector credentials appeared in process arguments | Operator + connector owner | Move out of argv, rotate/revoke, verify clean process listings | Secret/process scan and rotation drill | operator-gated | Values intentionally omitted |
| MP-016 | High | Visma may be falsely presented as a Verevon capability | MCP/product owner | Reject malformed records; show `not_configured`; onboard a real governed server | Schema/discovery/auth/HITL E2E | not configured | No live bridge/server |
| MP-017 | High | Ambient gateway `SendMessage` could publish to broad authorized NATS subjects without capability, HITL, or run binding | gateway + NATS owners | Quarantine RPC before publication; remove broad ACL grants; use per-run telemetry subject | `send_message_is_quarantined` and telemetry-subject tests | source-fixed; broker live | No live quarantine-RPC or hostile ACL proof |
| MP-018 | P0 | Producer crash/ambiguous terminal response can strand a governed run or falsely permit later presentation without a durable receipt | Session Core + gateway + Execution | Additive managed lifecycle now exists; prove obligation/receipt, scoped producer auth, lease/retry/reconciliation, and receipt-before-success | Focused lifecycle tests plus remaining crash/response-loss/cancel-race/duplicate/no-side-effect matrix | local migration applied; blocked | Tables and Auth issuance live; no immutable release recovery/rollback proof |
| MP-019 | High | Browser denial, timeout, cancellation, abort, or resource exhaustion could be represented as success | execution-core + gateway | Typed browser outcome mapping; failure/cancellation terminalization; reject unsupported browser approval | Browser unit/integration and gateway classifier tests | source-fixed; Browser Broker healthy | No authenticated browser action/Quarry/terminal-recovery proof; MP-018 remains |
| MP-020 | High | An ignored local Model Plane environment file contains a non-placeholder shared NATS credential | Operator + platform | Revoke/rotate through secret manager; replace local values with managed references; verify old credential denied/new scoped credentials work | Non-printing secret/process/history scan and rotation drill | operator-gated | Value intentionally neither read nor printed |
| MP-021 | P0 | Runtime-only secrets/network repair cannot yet be reproduced as a managed release, and Quarry is absent | Runtime + Identity + Data + Ingestion owners | Deploy the source-tested private Auth registry file and managed scoped NATS secrets; retain network contract; provide or explicitly disable Quarry capabilities | Compose, dependency, NATS, and auth smoke matrix | file path source-fixed; live-local production blocked | Model/Control/Data healthy and reachable; source file loader 93/93 but not deployed; NATS durability/Quarry remain |
| MP-022 | High | A Data Plane internal credential appeared in operator output during validation | Data/Model operators | Coordinated secret-manager rotation and scoped service restart; never print the value | Old credential denied, new credential accepted, non-printing secret scan | operator-gated | Rotation required before production |
| MP-023 | P0 | An artifact-contained runner could previously accept a caller-supplied unsigned trailing Compose overlay after verifying the signed payload | Release engineering | Restrict public and copied runners to fixed config validation and fixed no-build deployment operations | Unsigned `-f` overlay/direct-runner/wrapper and extra-argument negatives | source-fixed; not released | Release contract passes; no immutable artifact exists |
| MP-024 | Critical | A stolen `allowAnyOrg` workload key can request a service token for a caller-selected organization without a separately verified tenant delegation | Control Identity + cross-plane callers | Secure MVP rejects dynamic principals outside development/test and uses explicit per-principal tenant assignments; later dynamic use needs signed delegation | production-loader negative plus wrong-tenant/fixed-tenant matrix; future delegation replay/audience/expiry tests | production source fail-closed; live not migrated | Current live development issuance remains dynamic; source rejection and 93 focused tests pass; production fixed-tenant file absent |
| MP-025 | P0 | A caller-relative runtime env path could make artifact validation read one file while Docker consumed a same-named file from another working directory | Release engineering | Require an absolute path, resolve it physically once, and pass that exact path through validation and Docker | Nested-CWD twin-file regression | source-fixed; not released | RED reproduced; full release contract passes after fix |
| MP-026 | P0 | Symlink, mode, size, or read-time races on signing/evidence/runtime inputs could swap trusted release material or expose secrets | Release engineering | O_NOFOLLOW descriptor walk, ownership/mode and size policy, before/after inode metadata checks, private snapshots, temporary 0600 Docker runtime input | Symlink/FIFO/oversize/mode/hostile-umask/mutation tests | source-fixed; not released | Focused input suite and final release contract pass |
| MP-027 | P0 | Unsigned public-policy drift or all-zero evidence digests could claim Auth/ZDR proof that did not exist | Release engineering + provider/identity owners | Partition every Compose key; independently sign issuer/JWKS and full provider route; reject unknown credential/image keys and placeholder evidence digests | Public/secret/artifact drift, missing/zero Auth/ZDR/rollback evidence, legacy-alias and route mismatch tests | source-fixed; evidence still operator-gated | Final partition and release contracts pass; no actual Auth/ZDR evidence bundle supplied |
| MP-028 | P0 | A candidate could bind itself, the same image payload, a mutable gate/path, or an unrendered predecessor as rollback | Release engineering | Derive candidate gate/revision/digest from one signed private view; require distinct revision/image/root, trusted signatures, current fixed legacy-v3 adapter, and candidate-bound secret schema/keyset/version reference | Deterministic gate-swap, self/same-revision/same-image/signature/runtime-evidence/concurrent-mutation/render tests | tooling source-fixed; no artifacts | Final release contract passes; live rollback drill and immutable store absent |

## MVP acceptance status

| # | Acceptance criterion | 2026-07-16 status | Required evidence to pass |
|---|---|---|---|
| 1 | Backward-compatible protocol and safe rebuild | **Live local listeners pass; release/rollback fail** | Immutable artifact and old/new caller matrix |
| 2 | Authn/authz on sensitive HTTP/gRPC/NATS | **Live partial** | Complete boundary inventory, verified tenant delegation, remaining positive cases, NATS transport identity |
| 3 | cost-core scoped access | **Source pass; live negatives pass** | Legitimate valid scoped user/service read |
| 4 | Compaction durability/ZDR | **Partial** | Run six DB-gated tests and live-safe retry/concurrency/metrics probes |
| 5 | Letta memory works or is observably degraded | **Live degraded API/readiness pass; UI absent** | Frontend degraded UX or ZDR-safe semantic success |
| 6 | MCP containment and honest Visma | **Source partial; live absent** | Registry/execution/live negative matrix and `not_configured` UX |
| 7 | Non-divergent capability semantics | **Dispatch fail-closed live; catalog/UX partial** | Health reporter plus gateway/model/Frontend contract and E2E |
| 8 | HITL cannot be bypassed | **Live bypass negatives pass; delivery incomplete** | Continuation descriptor/dispatcher/success receipt and live positive delivery |
| 9 | ZDR end to end | **Live fail-closed; no usable route** | Provision/evidence strict org policy or ZDR provider and prove every content boundary |
| 10 | Suites, coverage, contract/E2E/live evidence | **Partial: focused suites and selected live probes pass** | DB/E2E/browser, Buf gates, accepted critical-coverage gaps, immutable artifact |
| 11 | Deployment and rollback runbook executable | **Documented; not executable** | Artifact, restore rehearsal, exact thresholds and operator sign-off |
| 12 | Docs match evidence | **Pass for the 20:05 CEST live snapshot and 22:24 CEST release-tooling verification** | Update after any artifact, production deployment, provider, or policy decision |

## Release decision

The binding decision is
`grpc-safe-rebuild-decision-2026-07-16.md`: local integration is authorized,
but production promotion remains blocked. The
minimum release blockers are an immutable verified artifact, complete required
credentials, release-database migrations/tests, authenticated health
attestation, durable approval continuation delivery, durable managed-run
terminalization migration/reconciliation proof, a safe ZDR/provider policy
outcome,
semantic-memory behavior, live negative/positive probes, and a rehearsed
rollback.

## Enterprise-next

Enterprise work remains deferred until the MVP table is fully green. The
separate roadmap covers workload identity/mTLS, fine-grained ABAC, HA/regional
failover, DR drills, SLO/error budgets, enterprise audit retention/export,
policy-as-code, key/secrets lifecycle, SBOM/provenance/signing, capacity/cost
controls, data residency, and compliance evidence. This audit makes no
enterprise-readiness claim.
