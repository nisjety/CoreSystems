# Model Plane — Production-Readiness Roadmap

Evidence date: 2026-07-16 (Europe/Oslo). Read
`MODEL_PLANE_STATUS.md`,
`docs/core-research/plane-audit-2026-07-16.md`, and
`docs/core-research/grpc-safe-rebuild-decision-2026-07-16.md` first.

**Secure MVP is the only active release program. Enterprise-next begins only
after every MVP acceptance criterion has evidence. The user-authorized local
Docker integration rollout does not authorize production promotion.**

## Current source progress

The 2026-07-16 source includes additive gateway/inference gRPC listeners with
bind-aware readiness, authenticated cost paths, distinct NATS principals,
artifact-v3 release tooling with fixed Compose operations, signed public versus
external-secret versus artifact-owned configuration partitions, and evidence-
bound Auth/JWKS/ZDR policy; approval outbox/read-through and lease primitives,
mandatory capability-before-dispatch policy, optional Letta tool-definition
ranking, typed multi-step hybrid retrieval, signed retention preservation, and
ZDR durable tool-output redaction. It now also includes additive managed-run
terminalization (durable obligation/receipt, recovery worker, and scoped
heartbeat), keyed-MAC opaque managed-start identities, bounded gateway
idempotency retention, and a strict deployment-owned exact-org retention policy
input in Auth Core. Browser lifecycle mapping now preserves non-success outcomes
rather than fabricating completion. Final source
verification completed at 15:10 CEST: Model Plane Rust suites, focused Go/Auth
suites, frontend typecheck and production build, release contracts, protocol
generation, and diff hygiene passed. The dated audit records exact counts and
the 59.52% all-target Rust line-coverage result. Production MVP does not mean
100% coverage; the stated risk-based target is 80% measured coverage on changed
security/business-critical modules, with gaps explicitly accepted rather than
hidden. Those are not deployment evidence; coverage gaps, Buf quality debt, and
all live gates remain release blockers.

At 2026-07-16 20:05 CEST, the user-authorized local integration rollout has all
21 Model containers running, all 19 health-checked containers healthy, and the
gateway/inference/execution gRPC contracts loopback-reachable. The eleven app
images are dirty-tree `working-tree` builds, unsigned and unattested. Control
Auth uses the canonical issuer and a least-privilege Model/Data principal
registry; Data retrieval, graph, and wiki are reachable over
`inter-plane-bus`. The registry and scoped credentials currently exist only in
the running container configuration. Source now provides a private file-backed,
fixed-tenant production registry and rejects `allowAnyOrg`, but that source and
the managed secret file are not deployed.

There is still no signed candidate or rollback artifact, live compaction
success, eligible ZDR provider, capability health attestation, successful
approval continuation, or Quarry runtime. Artifact v3 correctly refuses the
dirty tree and release mode requires signing/verification keys, compatibility
gates, and an external allowlisted runtime environment. Approval delivery has claim/lease,
retry, and terminal primitives but no continuation descriptor, dispatcher, or
successful execution receipt. Managed-run terminalization is source-implemented
but has no release-Postgres migration, live service-token, caller-compatibility,
or recovery evidence. Global capability health has no configured or attested
reporter. Interactive identity defaults to all-ZDR; no exact-org retention
policy or verified ZDR provider route is deployed. Those are MVP blockers, not
enterprise backlog.

## Secure MVP program

### Phase 0 — Freeze launch and create immutable recovery evidence

Owner: release engineering + Model runtime. Priority: P0.

1. Preserve the dirty worktree. Do not reset, clean, checkout, or revert user
   changes.
2. Obtain authority to isolate an exact reviewed revision and record the full
   diff, toolchain, dependency locks, migrations, and Compose config hash.
3. Move any credential still present in process arguments to an approved secret
   mechanism, then rotate/revoke the previously exposed connector values. Never
   print or reuse them.
4. Confirm storage headroom without implicit prune/deletion.
5. Run all three release suites and build a signed artifact-v3 candidate with
   `scripts/release-artifact.sh` only after supplying managed signing and
   verification keys plus non-placeholder Auth/ZDR/rollback evidence.
6. Supply signed public policy and a separate secret-only runtime file under
   the artifact's public/secret/artifact partition. Bind rollback secret schema,
   actual keyset, and an opaque trusted secret-manager version/reference digest;
   never bundle credential values in the artifact.
7. Keep artifact execution to the fixed `config --quiet` validation and fixed
   `up -d --wait --no-build` deployment operations. Do not permit caller-
   selected Compose files, env files, profiles, services, entrypoints, or
   commands after signature verification.
8. Produce a **separate** accepted rollback artifact with a different revision
   and image payload. Verify its signed root through private snapshots, restore
   both without rebuilding, confirm IDs match their locks, and then prove live
   credentials/data/health because `rollback-config-renderable` is not readiness.
9. Define numeric canary and rollback thresholds plus observation windows.

Exit: clean isolated source, signed candidate and separate rollback artifacts,
verified roots, external-runtime-env validation, restore rehearsal, and
operator sign-off exist. Until then, local integration testing may continue,
but do not promote, canary, or call the working-tree stack a production release.

### Phase 1 — Complete protocol and identity compatibility

Owner: gateway, inference, execution, Session, Auth, Frontend, Data callers.
Acceptance criteria: 1 and 2.

Local status: all three additive listeners are live, unauthenticated calls deny,
the canonical Control issuer is accepted by Session, and a Data principal
reaches Inference. Frozen descriptors, old/new caller coverage, and immutable
candidate proof remain open.

1. Preserve the additive gateway `:9090`, inference `:9092`, and execution
   `:9093` gRPC contracts. No flag-day removal.
2. Generate/freeze descriptors and run old-client/new-server plus
   new-client/compat-server tests for every required RPC and field.
3. Keep `/readyz` false until each listener is bound and its required dependency
   posture is known.
4. Complete exact audience/scope issuance and callers for Session, inference,
   execution, capability, cost, Data Plane, MCP, browser, and background work.
5. Replace `allowAnyOrg` key-only organization selection with an independently
   signed, audience-bound tenant delegation or explicit per-principal tenant
   assignments. Production source now rejects the dynamic shape, so explicitly
   assigned tenants are the secure-MVP path; a caller-provided `orgId` is never
   sufficient authority.
6. Deny caller-supplied tenant/user/org headers without a validated identity.
7. Resolve legacy `buf lint` violations through a staged compatible change and
   establish a non-empty `buf breaking` baseline.

Exit: contracts compile/generate/lint against a real baseline, every sensitive
entry point has an identity matrix, and candidate listeners pass authenticated
shadow tests.

### Phase 2 — Finish authorization and event identity

Owner: cost-core, session-core, capability-core, gateway, NATS owners.
Acceptance criteria: 2 and 3.

Local status: no-auth, malformed-bearer, and forged-scope probes deny at
Gateway, Cost, Capability, Session, and Letta; scoped NATS principals connect.
A legitimate valid scoped Cost read and cryptographic NATS workload identity
remain open. The live Control registry is runtime-only, and multi-tenant
principals can still choose an organization using only their workload key;
durable secret-file deployment and explicit fixed-tenant registry migration
remain open. Production source rejects that dynamic shape; verified delegation
is needed only before re-enabling dynamic multi-tenant workers.

1. Apply Cost Core auth with compatible callers; prove no token, malformed
   token, wrong audience/scope/tenant/user, forged headers, internal credential
   misuse, valid user, and valid service cases.
2. Apply Session/Capability migrations to disposable production-shaped
   Postgres before release. Run the six ignored Session database cases.
3. Preserve verified user/org predicates on every approval/memory/run by-ID and
   list path; validate exact retry and concurrency.
4. Deploy the distinct named NATS users and least-privilege subjects; do not
   reintroduce the generic `model-runtime` principal. Prove subject, envelope,
   payload, principal, and tenant equality.
5. NATS currently has no TLS, mTLS, or workload identity. For secure MVP, add
   TLS and workload-bound producer/consumer credentials or an equivalent
   cryptographic peer identity. A shared password plus payload validation is
   not sufficient authority.
6. Make security mutation and audit persistence atomic or outbox-backed.

Exit: live 401/403/200 evidence exists for every boundary and NATS provenance
cannot be forged by another workload credential.

### Phase 3 — Complete durable HITL continuation

Owner: Session Core + Execution Core + gateway. Priority: P0. Acceptance
criterion: 8.

Source already provides one content-free approval-delivery row per granted
approval, owner-bound durable cache read-through, and claim/lease, bounded
retry/backoff, poison/terminal, and acknowledgement-state primitives. It still
returns explicit `Unavailable` instead of false resume because no successful
continuation exists. A generic continuation cannot be made persistence-free:
after a restart it needs the exact tool/effect arguments and action state, or a
downstream immutable intent handle. The current identifier-only outbox and a
generic agent re-plan cannot supply that safely. Keep approval-required
external-effect capabilities unavailable until all of the following are met:

1. Provision and evidence a non-ZDR persistent posture, envelope-encryption/KMS
   lifecycle, and scoped service identity. Encrypted continuation content is
   still retained content; ZDR callers must be rejected before descriptor
   creation.
2. Define an additive versioned exact-effect descriptor with approval/run/org/
   user/step/action bindings, deterministic effect idempotency key, ciphertext,
   algorithm/version, and KMS key reference. Never store plaintext prompt,
   tool arguments, outputs, or secrets in events, logs, caches, or the outbox.
3. Define separate continuation claim/start/complete RPCs and a durable
   `pending → leased → started → succeeded | retryable | terminal` state
   machine. Do not reinterpret a legacy approval-delivery acknowledgement as
   successful execution.
4. Make continuation idempotent across dispatcher crash before and after effect
   acceptance; one approval produces at most one logical effect. Reuse the same
   effect key after recovery and require a downstream receipt.
5. Capture the exact approved effect before the pause point; never re-plan it
   from a granted generic approval or rely on a process-local DashMap state.
5. Prove generic resume cannot move `AwaitingApproval`, service identity cannot
   grant approval, and completed/cancelled runs cannot reactivate.
6. Add metrics/audit for pending age, attempts, lease expiry, delivered,
   terminal failure, principal, tenant, approval, run, and correlation ID,
   without content.
7. Run grant, deny, timeout, owner mismatch, cache eviction, duplicate retry,
   lease theft, both crash windows, restart, terminal replay, ZDR rejection,
   encrypted-descriptor validation, capability revocation, and tool-specific
   idempotent-receipt E2E tests.

Exit: a verified user decision is accepted by an authenticated continuation
dispatcher and receives one durable success receipt, or reaches an observable
terminal failure. HITL remains non-bypassable.

### Phase 3a — Durable managed-run terminalization

Owner: Session Core + gateway + Execution Core. Priority: P0. This is separate
from approval delivery: a known direct or browser outcome must not become a
durable run terminal state through a one-shot RPC alone.

Status: the additive source implementation is present in the dirty worktree:
`StartManagedRun`, `RecordTerminalOutcome`, and `HeartbeatManagedRun`; migration
`0015`; a metadata-only obligation/receipt; a leased recovery worker; scoped
service calls from Gateway and Execution; and an explicit rejection of legacy
terminal `CompleteStep` for managed runs. Migration `0015` and its tables are
applied locally and scoped Auth issuance works. The exit remains unfulfilled
until crash/recovery, old/new caller, immutable artifact, and rollback evidence
pass.

1. Verify the additive `StartManagedRun` path atomically creates the run and a
   content-free terminalization obligation, while legacy `StartRun` remains
   backward compatible for deployed callers.
2. Verify `RecordTerminalOutcome` uses a scoped service identity, derives
   organization and user from the run, validates a canonical source/terminal
   step, stores fixed metadata only, and returns the same receipt for retries.
3. Prove the keyed opaque request-to-run binding prevents an ambiguous retry
   from creating a second governed run and does not persist caller content.
4. Exercise the Session Core-owned worker's lease, backoff, idempotent receipt,
   cancellation race, and reconciliation against release-shaped Postgres.
   Gateway/Execution remain authenticated producers and never write Session
   Core's database.
5. Prove the receipt gates SSE `done`, replay completion, cost/usage success,
   and `RUN_COMPLETED`; on uncertainty expose `finalization_pending`, never
   success.
6. Preserve the source-level browser truthfulness gate: only explicit success
   completes; denial, timeout, and exhaustion fail; cancellation/abort cancel;
   approval-required browser work remains pre-dispatch rejected until it has a
   durable continuation contract.
7. Test crash before/after outcome persistence, response-loss retry,
   duplicate/conflicting outcomes, cancel race, lease recovery, no browser
   side-effect replay, service/tenant authorization, and ZDR DB/NATS/log
   inspection.

Exit: every governed run has a durable, idempotent terminalization receipt or
an observable non-success reconciliation state; no stream or cost success can
outrun it.

### Phase 4 — Make capability semantics authoritative everywhere

Owner: Capability Core + Execution Core; Frontend owns presentation.
Acceptance criterion: 7.

Local status: an exact execution identity reaches the mandatory policy gate;
`cap.retrieval.query` denies as `health_not_attested`, and cross-organization
reuse denies. All 27 enabled rows remain unavailable because no trusted global
health reporter has attested them.

1. Preserve mandatory policy evaluation before both direct and agentic tool
   dispatch.
2. Deploy the dedicated global-only authenticated health reporter. Newly seeded
   capabilities remain `health_not_attested` until that reporter attests a real
   dependency; tenant reporters must not mutate global catalog health.
3. Prove `allow`, `ask`, `deny`, disabled, stale, unhealthy, quarantined,
   unavailable, and policy-outage outcomes at model offer and execution.
4. Bind dynamic MCP tools to exact registry capabilities and allowlists before
   they can be offered or dispatched. Until then deny.
5. Derive gateway/model catalogs, ordinary selected-tool chat, Browse, Plan,
   Agent Run Console, shipping, and Frontend display from the same versioned
   state/reason contract.
6. Add trusted concrete agent/run/thread/workspace/user subject identifiers
   before enabling those grant scopes; request fields alone grant nothing.
7. Display cost and approval implications. Plain chat may remain intentionally
   tool-free; do not force every turn into expensive agent mode.

Exit: UI, model offers, policy, and dispatch agree for every capability and
health transition.

### Phase 5 — Resolve retention and provider availability

Owner: Identity policy + provider owner + every content boundary. Acceptance
criterion: 9.

Local status: a Data retrieval principal obtained a canonical-issuer token whose
ZDR claim overrode request `zdr=false`; Inference skipped every configured
unattested provider, made zero provider attempts, and returned
`FailedPrecondition`. This proves fail-closed safety, not a usable chat route.

1. Preserve issuer-monotonic retention: request intent can tighten, never
   loosen. Signed ZDR must survive execution ingress and delegated credentials;
   non-ZDR service posture remains deployment-owned per exact audience.
2. Auth Core source now accepts only a strict deployment-owned exact-org
   interactive retention policy; absence means all-ZDR and malformed policy
   fails closed. Provision and evidence one governed usable route:
   - canonical organization retention policy explicitly permits persistence for
     the request; or
   - an independently verified provider deployment satisfies ZDR.
3. Do not infer ZDR from geography, provider brand, environment naming, or an
   unverified confirmation flag.
4. Prove ZDR content does not persist in prompts, messages, events, runs,
   checkpoints, compaction, memory, tool steps, approval outbox, caches,
   traces/logs, cost payloads, Data Plane grounding, MCP, or provider bridges.
5. Ensure ephemeral tool output can support the current reasoning round while
   only content-free metadata crosses durable boundaries.
6. Reject every unattested inference modality before provider I/O, then test
   unary, streaming, embeddings, speech, translation, vision, document,
   language, realtime, video, retry/fallback, circuit breaking, and provider
   outage without surprise paid load.

Exit: a usable governed route exists and end-to-end evidence shows no ZDR
downgrade or retention.

### Phase 6 — Restore memory and finish agentic retrieval safely

Owner: Session Core, letta-bridge, Execution Core, Data Plane. Acceptance
criteria: 4 and 5.

Status: default context assembly now retains bounded Letta outcomes (`empty`,
`results`, or `DEGRADED_LETTA_*`) and emits content-free metrics rather than
collapsing an RPC/timeout failure into an empty result. Compose liveness uses
`/healthz`; live liveness is 200 and `/readyz` is 503 with
`DEGRADED_SEMANTIC_UNVERIFIED`. This proves explicit local degradation, not an
eligible provider, authenticated semantic success, ZDR safety, or Frontend UX.

1. Preserve compaction retry/backoff, poison/ZDR exclusion, deterministic IDs,
   conflict isolation, and metrics. Run the release-Postgres concurrency and
   idempotency cases; no live compaction claim exists on 2026-07-16.
2. Restore Session → letta-bridge semantic memory search or deploy the explicit
   structured degraded state through authenticated readiness, metrics, and UX.
   Preserve the source distinction between liveness and semantic capability.
3. Keep Letta `POST /v1/tools/search` classified separately: it ranks tool
   definitions, is opt-in, makes no request for ZDR/unspecified retention, and
   can only reorder the exact local authorized intersection. It cannot repair
   memory search or grant tool authority.
4. Preserve typed hybrid retrieval states (`ok`, `low_confidence`,
   `no_results`, `degraded`), bounded materially different reformulation,
   exact-repeat suppression, provenance, and round budgets.
5. Add retrieval evaluation for answer support, citation/provenance, query
   decomposition, backtracking, low-confidence stop, cost, latency, and ZDR.
6. Do not expose raw SQL/Cypher or arbitrary MCP retrieval. Future routes need:
   - **tabular/structured:** parameterized allowlisted semantic query DSL,
     schema/column authorization, row/time/cost bounds, provenance;
   - **graph:** typed traversal templates, depth/fanout limits, tenant labels,
     provenance;
   - **MCP retrieval:** registry-bound server/tool allowlist, SSRF/auth/secret
     controls, size/time/cancellation/rate bounds, audit, ZDR handling.
7. Add a server-owned router that selects only configured/healthy routes and
   reports the selected route/reason. Unsupported routes stay machine-readable
   `not_configured`.

Exit: memory is real or honestly degraded, hybrid multi-step retrieval is
live-evaluated, and no unsafe structured/graph/MCP shortcut exists.

### Phase 7 — MCP containment and honest Visma onboarding

Owner: gateway + Capability Core + operations. Acceptance criterion: 6.

1. Keep stdio disabled for deployable MVP unless a fixed executable/argument
   registry and sandboxed workload identity are separately approved.
2. Prove HTTP MCP HTTPS-only validation, redirect denial, DNS re-resolution and
   private-range denial at connect time, proxy bypass prevention, auth,
   tenant scope, exact tool allowlist, managed secret references, timeouts,
   cancellation, rate/size limits, audit, and health/discovery.
3. Reject/quarantine malformed transport hybrids and empty allowlists at write
   and migration time.
4. Report Visma `not_configured`. A real outcome needs a separately deployable
   Verevon Visma MCP server, governed auth/secret reference, exact allowlist,
   discovery/health tests, HITL, and end-to-end invocation. The operator's
   Codex/Claude connector is not a Verevon runtime capability.

Exit: all RCE/SSRF/auth/tenant/secret/allowlist/timeout/HITL negative tests deny,
and no integration is fabricated.

### Phase 8 — Release verification and staged deployment

Owner: release engineering + service and cross-plane owners. Acceptance
criteria: 10–12.

1. Run format, lint, typecheck, unit, integration, race, contract, E2E,
   security, migration, and supply-chain checks from the immutable revision.
2. Measure at least 80% on changed security/business-critical modules; report
   actual gaps and ignored tests.
3. Verify the signed artifact-v3 root and compatibility gates, validate the
   external runtime environment against its allowlist, restore candidate
   archives in a non-production context, and start services in the order in the
   2026-07-16 decision record.
4. Use legitimate seeded identities/data for live curl, gRPC, browser, NATS,
   Postgres, and metrics probes. Include all negative cases and valid access.
5. Classify every downstream as real, sandbox, test server, degraded,
   unavailable, or not configured. Do not use paid/destructive actions merely
   for connectivity.
6. Canary one boundary at a time with declared thresholds and observation
   holds. Rehearse rollback to the separate retained digest.
7. Update the evidence matrix, finding register, status, roadmap, decision
   record, service notes, and cross-plane handoff after the actual deployment.

Exit: every one of the twelve MVP acceptance criteria has source, migration,
live, and rollback evidence. Only then may this be called a production-ready
secure MVP.

## Enterprise-next — explicitly deferred

The following begins only after Secure MVP exit and does not imply enterprise
readiness:

1. **Workload identity and transport:** short-lived per-workload credentials,
   mTLS/SPIFFE or equivalent, automated rotation/revocation and drills.
2. **Fine-grained authorization:** ABAC for principal, tenant, workload,
   capability, data class, purpose, residency, and resource; policy-as-code with
   review, simulation, and rollback.
3. **HA and regional failover:** multi-instance services, database/broker HA,
   region-aware dependency failover, tested circuit breaking.
4. **DR:** declared RPO/RTO, encrypted backups, restore and regional evacuation
   drills, consistency across sessions, approvals, costs, audit, and registry.
5. **SRE:** user-journey SLOs, error budgets, paging, capacity/latency/cost
   dashboards, runbooks, game days, load and regression floors.
6. **Enterprise audit:** tamper-evident retention, export/SIEM, legal hold,
   tenant access review, policy-governed retention and deletion.
7. **Secrets and keys:** managed secret references, envelope/per-tenant
   encryption where required, automated rotation, break-glass, inventory, and
   compromise response.
8. **Supply chain:** reproducible builds, locked dependencies, SBOM,
   provenance, signing/verification, vulnerability policy, admission control.
9. **Capacity and cost:** tenant/workload/model/tool quotas, fair scheduling,
   admission control, anomaly detection, forecasts and reservation economics.
10. **Residency and compliance:** region-aware processing/storage/providers,
    data maps, deletion/subject-request evidence, control mappings, continuous
    compliance evidence.

Every enterprise item needs an owner, measurable exit criteria, threat/failure
tests, deployment evidence, and rollback before it can change the readiness
claim.
