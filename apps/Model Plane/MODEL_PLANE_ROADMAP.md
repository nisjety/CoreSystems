# Model Plane — Production-Readiness Roadmap

Evidence date: 2026-07-13. Read `MODEL_PLANE_STATUS.md` and
`docs/core-research/plane-audit-2026-07-13.md` first.

The roadmap is deliberately split into two programs. **MVP work is the active
release gate. Enterprise-next does not begin by relabeling incomplete MVP work
as an enterprise concern.** No phase below is a deployment authorization.

Source progress on 2026-07-13: cost/capability/session/inference/Letta auth,
user-scoped approvals and memory, exact approval/browser delegation, browser
BFF ownership, strict MCP registry validation, structured degraded readiness,
inline MCP denial, monotonic ZDR, additive authenticated gRPC, an additive
capability-availability contract, tenant-bound policy/grants, bounded
user-owned approval cache, persist-before-publish approval creation, and the
ordinary HTTP/SSE caller graph are
implemented and tested. They remain inside the phases below because the
current all-ZDR issuer policy makes durable source flows unavailable, capability
policy is not yet an unavoidable execution gate, approval resume lacks an outbox,
several database tests/migrations lack live proof, provider ZDR eligibility is
unverified, and no safe staged deployment or runtime matrix has occurred.

## Secure MVP

### Phase 0 — Freeze unsafe rebuilds and create recovery evidence

Owner: Model Plane runtime + operations. Gate: P0.

1. Freeze rebuild/recreate of `model-gateway`, `inference-core`, and
   `execution-core`. The source/runtime protocol mismatch is already breaking
   chat and inference.
2. Preserve current containers and images read-only. Inventory exact image IDs,
   config hashes, migration versions, source revisions, and build provenance.
3. Produce immutable candidate and rollback images. The current `revision =
   unverified` images are not acceptable rollback evidence.
4. Recover disk headroom without pruning or deleting data during this audit;
   obtain explicit operator authorization for any cleanup.
5. Implement the side-by-side compatibility sequence, health gates, and
   rollback triggers in
   `docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md`.

Exit: signed/pinned artifacts exist; protocol contract tests pass; old and new
client matrices are known; rollback is rehearsable without rebuilding.

### Phase 1 — Restore the protocol path with authenticated identity

Owner: model-gateway, inference-core, execution-core, session-core, caller
services. Gate: MVP acceptance criteria 1 and 2.

1. Restore additive inference gRPC behavior on `:9092` (or a shadow listener)
   with verified audience and scope enforcement. Update model-gateway,
   execution-core, Data Plane embedding/retrieval, and capability callers before
   cutover.
2. Restore the required model-gateway gRPC APIs on `:9090`, including MCP
   discovery/proxy contracts, with authenticated caller identity.
3. Keep execution-core `:9093` authenticated and update every caller to forward
   or mint the correct credential. Resolve proto drift before deployment.
4. Add readiness probes that execute a bounded dependency check. Liveness must
   not claim readiness when a required listener or downstream is absent.
5. Add old-client/new-server and new-client/old-server contract tests, then an
   authenticated chat → inference → tool smoke test and a Data Plane query-
   embedding smoke test.

Exit: ordinary inference, Browse, Plan, Agent Run Console, shipping-tool, and
MCP-negative paths work through documented compatible contracts; no
caller-supplied identity header grants authority.

### Phase 2 — Close live authorization and HITL bypasses

Owner: session-core, capability-core, model-gateway, cost-core, bridge-core,
browser-broker, sandbox-manager. Gate: criteria 2, 3, 6, and 8.

1. Deploy `cost-core` auth only after pricing and all callers have compatible
   credentials. Prove no auth, malformed bearer, forged identity header, wrong
   tenant, wrong user, wrong scope, valid user, and valid service cases.
2. Deploy session-core gRPC authentication and tenant/user-scoped storage predicates.
   `DecideApproval` must update by approval ID + organization + `requested`
   state, derive the actor from verified identity, and be compare-and-set/idempotent.
   Remove the unauthenticated all-org boot-rehydrate path. Apply and verify the
   user-scoped approval/memory uniqueness migration before traffic cutover.
3. Authenticate capability-core HTTP/gRPC and scope by-ID repository methods by
   organization. Apply migration `0007`, verify invalid legacy risks and
   ambiguous grants are quarantined/constraint-compatible using a disposable
   production-shaped PostgreSQL fixture, and keep private Capability HTTP
   memory actor-scoped. Policy accepts only `global` and verified-tenant `org`;
   agent/resource scopes and resource-scoped writes remain unavailable until a
   trusted owner can be verified. Apply the same rule to inference routing policy.
4. Prevent direct tools-only MCP dispatch. MCP tools must execute only through
   the governed agentic path and the same durable approval check.
5. Persist approval decision and execution-resume intent transactionally, then
   deliver through an idempotent outbox/reconciler. Decisions must read through
   to the user-scoped durable record after process-cache eviction. Exact retries
   are source-idempotent, but client retry alone is not recovery from process
   failure.
6. Quarantine bridge/browser/sandbox surfaces until they have authenticated,
   tenant-scoped callers and honest readiness states. Replace the Frontend
   browser process-local ownership map with durable/shared ownership before HA;
   for single-instance MVP, make restart loss an explicit health/recovery gate.
7. Validate NATS subject tenant, envelope tenant, and payload tenant equality;
   add durable consumers, bounded deduplication, provenance, and replay tests.
8. Make capability, rollout, and scope mutation plus audit atomic through one
   transaction/outbox; fault-inject audit storage failure and reject an
   unaudited success.

Exit: every sensitive inbound HTTP/gRPC/NATS boundary has a documented
principal, audience, scope, tenant derivation, negative matrix, and audit event.

### Phase 3 — MCP containment and honest Visma onboarding

Owner: model-gateway + capability-core + operations. Gate: criterion 6.

1. Keep stdio registration disabled in the deployable MVP. If it is ever
   reintroduced, use a fixed executable registry and fixed argument templates;
   never accept caller-supplied commands or inherit unrestricted service
   credentials.
2. Deploy HTTP MCP only after exact tool allowlists, HTTPS-only validation,
   redirect denial, DNS re-resolution/private-range denial, proxy bypass
   prevention, auth, tenant scope, timeouts, cancellation, rate limits, size
   limits, health/discovery state, and audit tests pass.
3. Store only encrypted secret references. Reject raw credentials at write time
   and redact all logs/audit payloads.
4. Reject the malformed live `visma mcp` record during migration and report
   `not_configured`. Define an onboarding contract for a separately deployable
   Visma MCP server, transport, OAuth/secret reference, allowlist, discovery,
   health, and rollback.

Exit: RCE, SSRF, redirect, DNS rebinding, malformed transport, empty allowlist,
forged role, wrong tenant, oversized response, timeout, and HITL bypass tests
all deny. No Visma success is claimed without a real Velion runtime server and
end-to-end invocation.

### Phase 4 — Session durability, Letta, and ZDR

Owner: session-core, letta-bridge, model-gateway, provider routing. Gate:
criteria 4, 5, and 9.

1. Preserve the now-healthy compaction path. Source now covers deterministic
   IDs, bounded retry/backoff, poison/ZDR exclusion, conflict isolation, and
   metrics; next bound candidate selection before aggregation, serialize manual
   and automatic ordinal allocation, and run the ignored Postgres concurrency/
   idempotency tests. Do not “fix” the withdrawn 100%-failure claim.
2. Restore Letta semantic search with the intended embedding configuration, or
   expose a structured degraded state through readiness, metrics, API errors,
   and user-facing behavior. Never convert dependency failure to empty relevant
   results.
3. Resolve the product/security policy contradiction: Auth Core currently
   issues every delegated token with `zdr=true`, while session and Letta
   correctly reject durable ZDR work. Then compute effective ZDR once from
   verified issuer policy plus request intent;
   callers may tighten but never loosen it. Use that value for unary and SSE.
4. Prove no ZDR content reaches durable events, session rows, compaction,
   checkpoints, Letta tiers, prompt caches, traces/logs, tool I/O persistence,
   Data Plane grounding, or external MCP/provider bridges.
5. Deploy and live-verify the source-complete provider ZDR eligibility gate:
   Azure confirmation remains false until independent contract evidence exists;
   then prove unverified providers receive no unary, stream, or embedding data.

Exit: compaction and memory behavior are live-proven; ZDR positive/negative
tests span every persistence boundary.

### Phase 5 — One capability contract and cross-plane UX handoff

Owner: Model Plane capability contract; Frontend Plane presentation. Gate:
criterion 7.

1. Adopt the additive capability availability schema as the authoritative
   contract for built-ins, provider actions, Browse, MCP, and future tools.
   Keep version-CAS, server-time freshness, safe public reasons, and audit.
2. Make policy evaluation, model tool offers, and execution dispatch deny
   disabled/unhealthy/stale/unavailable capabilities. Provision scoped health
   reporters, including an explicit global-capability path, and reconcile
   durable List/Get/model behavior.
3. Extend policy invocation with trusted concrete identifiers for agent, run,
   thread, workspace, and user grants before re-enabling those currently
   rejected scopes; never treat a caller-supplied scope kind/value alone as
   subject authorization. Keep missing/unknown scope and risk values denied.
4. Return machine-readable state and reason: `available`, `disabled`,
   `unhealthy`, `approval_required`, `unavailable`, and `not_configured`.
5. Reconcile static frontend catalogs, gateway discovery, capability-core,
   execution-core registrations, and model tool offers with contract tests.
6. Frontend v3 must let users intentionally select business tools or Plan mode,
   display cost/approval implications, and render degraded/unavailable reasons.
   Plain chat may remain intentionally tool-free; do not silently force all
   messages into expensive agentic execution.
7. Test default chat, explicitly selected tools, Browse, Plan, Agent Run Console,
   shipping, unhealthy MCP, and approval-required outcomes end to end.

Exit: every mode derives from the same authoritative semantics and the UI never
advertises an unavailable capability as usable.

### Phase 6 — MVP release verification and staged deployment

Owner: release engineering + service owners. Gate: criteria 10–12.

1. Run format, lint, typecheck, unit, integration, contract, E2E, security, and
   migration suites. Measure changed security/business-critical packages and
   report actual coverage; target at least 80% without hiding module-level gaps.
2. Run safe authenticated live probes with seeded test tenants: no auth,
   malformed bearer, wrong audience, wrong tenant, forged headers, internal
   credential misuse, allowed user/service, ZDR, HITL, budget, and dependency
   degradation.
3. Classify every downstream/provider result as real, sandbox, stub, degraded,
   or unavailable. Do not create paid/destructive actions for connectivity
   proof.
4. Deploy in the safe build order with health gates and rollback triggers. Hold
   each stage long enough to observe errors, latency, cost attribution, and
   audit completeness.
5. Reconcile dated status, per-service notes, evidence matrix, finding register,
   operations runbooks, and cross-plane ownership docs.

Exit: all twelve MVP acceptance criteria are evidenced. Only then may the
release be called a production-ready secure MVP.

## Enterprise-next — explicitly not current readiness

These items start after MVP acceptance and do not imply enterprise readiness:

1. **Workload identity and transport:** per-workload identities, short-lived
   credentials, mTLS/SPIFFE or an equivalent authenticated mesh, automated
   rotation, and revocation drills.
2. **Authorization:** fine-grained ABAC for organization, user, workload,
   capability, data classification, residency, and purpose; policy-as-code with
   review and simulation.
3. **Availability:** multi-instance stateless services, database and broker HA,
   regional failover, tested dependency circuit breaking, and removal of
   single-host assumptions.
4. **DR:** declared RPO/RTO, encrypted backups, restore and regional evacuation
   drills, and evidence that sessions, costs, approvals, audit, and registry
   state recover consistently.
5. **SRE:** user-journey SLOs, error budgets, latency/cost/capacity dashboards,
   paging, runbooks, game days, and benchmark/load/regression floors.
6. **Enterprise audit:** tamper-evident retention, tenant export, legal hold,
   access review, SIEM integration, and policy-governed retention/deletion.
7. **Secrets and keys:** managed secret references, envelope encryption,
   per-tenant keys where required, automated rotation, break-glass, compromise
   response, and cryptographic inventory.
8. **Supply chain:** reproducible builds, locked dependencies, SBOMs, provenance,
   signing/verification, vulnerability policy, and admission controls.
9. **Capacity and cost:** quotas by tenant/workload/model/tool, admission
   control, fair scheduling, spend anomaly detection, capacity forecasts, and
   provider reservation/fallback economics.
10. **Residency and compliance:** region-aware scheduling/storage/provider
    selection, data maps, deletion/subject-request evidence, control mappings,
    and continuous compliance evidence collection.

Each enterprise item needs an owner, measurable exit criteria, threat/failure
tests, deployment evidence, and a rollback path before it can change the
readiness claim.
