# Decision Record: Do Not Rebuild Yet / Safe Model Plane Release

- Date: 2026-07-16 (Europe/Oslo)
- Status: **Accepted production-release block; user-authorized local integration deployment exists**
- Supersedes for current state:
  `grpc-safe-rebuild-decision-2026-07-13.md` (retained as history)
- Scope: all Model Plane application services, with explicit compatibility
  gates for gateway `:9090`, inference `:9092`, execution `:9093`, Session Core,
  Capability Core, Cost Core, NATS, Frontend, and Data Plane callers
- Owners: Model Plane runtime and release engineering; Identity, Frontend, Data,
  and provider owners participate where their contracts are release gates

## Decision

Do not promote the current dirty-worktree stack as the canonical Model Plane
production release. User-authorized local builds and recreates may continue
solely for integration evidence when they preserve data, secrets, and rollback
safety; they do not satisfy immutable artifact or production-cutover gates. Do
not use a flag-day gRPC removal or unauthenticated compatibility listener.

At 2026-07-16 20:05 CEST, a local integration stack has 21 running containers,
19/19 health-checked healthy, with authenticated gateway/inference/execution
gRPC on loopback. Its eleven app images are dirty-tree `working-tree` builds,
unsigned, unattested, runtime-secret-dependent, and not captured as a separately
verified artifact. It is not an accepted rollback target. Approval continuation,
health attestation, usable retention/provider policy, and immutable release
proof remain incomplete.

## Evidence for the block

- Branch `main` at `ae3ee041e13d482b20e5883e79044e714bf3d216` has a large
  uncommitted multi-plane diff.
- The initial read-only inventory found zero Model containers/listeners; that
  historical state was superseded by the later user-authorized local rollout.
- Final source verification at 2026-07-16 15:10 CEST passed the targeted Rust
  suites, configured Rust formatting/Clippy gate, focused Go/Auth suites,
  frontend typecheck/production build, protocol generation, release contracts,
  and diff hygiene. It did not build an image or start a container.
- Required production credentials/configuration are incomplete. Values were
  not printed or copied.
- The initial read-only Compose preflight failed before Docker invocation:
  the mutable local environment lacked ten required scoped NATS-principal
  passwords and would reuse seven existing Model Plane persistent volumes.
  Auth Core was unhealthy and Data shared-network reachability was absent.
  Runtime-only scoped credentials, canonical Auth issuance, and the approved
  Data overlay later cleared those local blockers; Quarry remains absent.
- Gateway, inference, and execution gRPC listeners are additive, live on
  loopback, and reject unauthenticated calls. The production overlay removes
  all host ports; east-west clients use the private networks.
- Artifact-v3 tooling and the release Compose boundary are source-tested. The
  artifact script correctly refuses the dirty tree, so no immutable artifact
  has been produced. Release mode requires signing/verification keys,
  compatibility gates, a signed Auth issuer/JWKS/provider-ZDR public policy,
  and an external allowlisted runtime environment. The copied runner accepts
  only fixed config validation or fixed no-build deployment and rejects an
  unsigned trailing Compose overlay.
- Control Auth's canonical service-token issuance works live, but its registry
  is runtime-only. Multi-tenant principals using `allowAnyOrg` can select an
  organization using only the workload credential in the running development
  stack. Production source now loads a private registry file and rejects that
  dynamic shape outside exact development/test; the secure-MVP path is explicit
  tenant assignments until separately verified delegation exists. That fixed-
  tenant production file has not been provisioned or deployed.
- Approval grant persistence/outbox, durable read-through, and
  claim/lease/retry/terminal primitives exist, but no restartable continuation
  descriptor, authenticated dispatcher, or success receipt can prove the
  suspended execution restarted. Execution correctly returns `Unavailable`
  rather than a false resume.
- Gateway and browser source paths now preserve known terminal meaning: only an
  explicit browser success completes; denial, timeout, and exhaustion fail; and
  cancellation/abort use `CancelRun`. Additive managed-run terminalization now
  exists in source: durable request-to-run binding, metadata-only obligation/
  receipt, scoped producer heartbeat/finalization, and a leased reconciler.
  Legacy terminal `CompleteStep` is rejected for managed runs. Migration
  `0015` and its tables are applied locally and scoped service-token issuance
  works; periodic/browser clock evidence, producer-crash recovery, immutable
  release migration, and rollback proof remain absent.
- Capability policy is mandatory at dispatch. A live exact execution identity
  was denied with `health_not_attested`, and wrong-org reuse was denied. All
  seeded tool capabilities
  begin unavailable with `health_not_attested`. A dedicated global-only health
  attestation path exists, but no reporter is configured or has attested a live
  dependency.
- NATS has distinct named principals and no generic `model-runtime` principal,
  but does not yet configure TLS, mTLS, or workload identity. The formerly
  ambient gateway `SendMessage` publisher is quarantined before publication and
  its broad ACL grants are removed. The broker and scoped principals are live;
  hostile-principal subject and transport-identity proof remain absent.
- Interactive identity remains all-ZDR. A live Data retrieval token proved
  signed ZDR overrides request `zdr=false`; both configured providers were
  skipped with zero attempts and the call failed before provider I/O. Signed ZDR is preserved through
  execution/delegation and every unattested inference modality fails before
  provider I/O. No provider deployment is independently verified as
  ZDR-capable, so secure inference is intentionally unavailable.
- Letta tool-definition ranking is optional and source-tested. It is not memory
  search. Live Letta liveness is 200 while semantic readiness is 503
  `DEGRADED_SEMANTIC_UNVERIFIED`; authenticated semantic success is not proven.
- Source unit/security suites pass substantially, but the current all-target
  Rust coverage result is only 59.52% lines. Changed lifecycle paths remain
  below the 80% target (gateway browser/session/SSE 62.49%/64.53%/56.43%,
  execution browser/runtime/tool bridge 77.90%/65.43%/70.67%, and Session
  approval delivery 35.32%); six Session Core database tests are ignored,
  selected live auth probes pass but full E2E remains absent, and legacy Buf
  lint and breaking-baseline gates do not pass.
- A Data Plane internal credential appeared in operator output during live
  validation. Its value is omitted; coordinated secret-manager rotation across
  Data and Model is required before production.

## Release artifacts

The repository now defines immutable artifact format v3. It covers all 21
Compose services and 20 unique images, records content IDs, and enforces
`pull_policy: never`:

1. `scripts/release-artifact.sh build` builds application images once from an
   isolated source revision and requires release-mode managed signing and
   verification keys plus a non-secret compatibility-gates file.
2. Each image is exported to a Docker archive with a checksum;
   `images.lock.env` contains content-addressed `sha256:` image IDs only.
3. The artifact snapshots the release Compose/NATS/OTEL/seccomp inputs,
   migrations, cross-plane revision/keyset records, a complete
   public/secret/artifact configuration partition, independent Auth issuer/JWKS
   policy, any exact provider-ZDR route, and a complete signed root manifest.
4. `verify` checks the signature, complete snapshot, locks, migrations, and
   gates without rebuilding; `restore` imports exact archives without
   registry/tag resolution.
5. Artifact-contained `compose`/`deploy` verifies before Docker invocation,
   accepts only fixed `config --quiet` or `up -d --wait --no-build` operations,
   and
   requires a separately managed secret-only runtime file; unsafe input types,
   modes, sizes, symlinks, races, and placeholder evidence fail closed. It also
   requires an externally located, independently signed rollback artifact
   (`MODEL_PLANE_ROLLBACK_ARTIFACT_DIR`) whose manifest digest, distinct source
   revision/image payload, secret schema, supplied keyset, and opaque secret-
   version reference are candidate-bound. The current verifier consumes private
   signed-member snapshots through a fixed legacy-v3 adapter. This proves only
   `rollback-config-renderable`; credentials, data, health, and cutover need a
   live drill. Runtime credentials are never copied into the artifact. Direct
   mutable-workspace production Compose is refused.

This is an **artifact contract**, not an artifact. The current tree fails the
clean-source precondition and no prior digest exists. Do not weaken the refusal
or substitute a mutable tag, uncommitted directory hash, emergency rebuild, or
unsigned/external-unverified runtime input.

## Required safe-build order

### Gate 0 — Isolate and authorize the release source

1. Preserve all current user changes. Do not reset, clean, checkout, or revert
   them.
2. Obtain explicit authority to commit or otherwise isolate an exact reviewed
   source revision. Record the revision and complete diff.
3. Complete security/correctness review of that diff. Resolve critical/high
   findings or record an operator-gated blocker.
4. Supply required credentials from managed references/files; never from
   process arguments or copied operator connector credentials.
5. Supply a private Control service-principal registry containing explicit
   per-principal tenant allowlists. Production must reject `allowAnyOrg`; keep
   dynamic workers unavailable until signed user/work delegation is complete.
6. Rotate/revoke the connector credentials previously observed in argv after
   moving them to the approved mechanism. Verify clean process listings without
   printing values.

### Gate 1 — Finish release semantics before building

1. Keep approval-required external-effect capabilities unavailable until a
   verified non-ZDR retention policy, envelope-encryption/KMS lifecycle,
   scoped worker identity, and tool-specific downstream idempotency/receipt
   proof exist. Then build on the existing claim/lease primitives with an
   encrypted exact-effect descriptor, separate dispatcher lifecycle, and
   durable success receipt; never re-plan from a granted generic approval.
2. Migrate and prove the source-implemented Session Core managed-run contract:
   additive `StartManagedRun`, service-authenticated `RecordTerminalOutcome`,
   heartbeat, metadata-only receipt, and leased reconciliation. Gateway waits
   for the receipt before SSE `done`, usage/cost success, or replay completion.
   Run release-Postgres and crash/response-loss/cancel-race/no-side-effect-
   replay tests.
3. Define and deploy the dedicated global-only capability health reporter and
   attest every tool dependency. Unknown, stale, and unhealthy stay
   unavailable; a tenant reporter must not mutate global catalog health.
4. Resolve the interactive all-ZDR/provider contradiction through a canonical
   organization retention policy or independently verified ZDR provider. Keep
   signed ZDR monotonic through execution/delegation and reject every
   unattested inference modality before provider I/O; do not enable a
   confirmation flag without evidence.
5. Prove Letta memory semantic success or deploy the source-implemented bounded
   degraded state through authenticated readiness, metrics, and Frontend UX.
   Do not substitute `/v1/tools/search`, which ranks tools.
6. Keep structured/tabular/graph/MCP retrieval unavailable until typed,
   parameterized, tenant-scoped owner-plane contracts pass security tests.
7. Apply and test required migrations against disposable production-shaped
   Postgres, including Session Core approval delivery and concurrency tests.
8. Resolve or explicitly waive legacy Buf lint with a staged compatibility
   plan, and establish a non-empty breaking-change baseline.

### Gate 2 — Build candidate and rollback artifacts

1. Confirm disk headroom for images, archives, logs, and database safety. No
   prune or deletion is implicit.
2. Run formatting, lint, typecheck, unit, integration, race, contract, E2E,
   migration, and security suites from the isolated revision.
3. Measure changed security/business-critical coverage; close or justify every
   gap below 80%.
4. Build the signed immutable artifact-v3 once. Record source revision,
   toolchain, resolved dependencies, image IDs, checksums, Compose/config/migration
   snapshots, cross-plane records, root-manifest signature, gates, and creation
   time. Do not copy runtime credentials into it.
5. Verify the archive with the trusted verification key, validate the separately
   managed runtime-env file against the captured key policy, restore into a
   non-production Docker context, and confirm restored image IDs equal
   `images.lock.env`.
6. Retain one independently verified rollback artifact. Because there is no
   current predecessor, the candidate cannot be its own rollback; the rollback
   must be a separately accepted known-good release or the deployment remains
   blocked.

### Gate 3 — Start dependencies and protected services in order

Use distinct candidate service names/addresses. Do not replace canonical
discovery during validation.

1. Start Postgres/Redis/NATS/Temporal/observability dependencies with distinct
   scoped credentials and health gates. NATS currently has no TLS/mTLS/workload
   identity, so do not characterize this source state as a finished transport
   identity control.
2. Run migrations and verify schema/data invariants before application traffic.
3. Start Auth-dependent control services: Cost, Capability, Session, and Letta
   bridge/degraded boundary. Verify their exact audiences/scopes first.
4. Start inference candidate on a shadow listener and verify gRPC readiness,
   auth, ZDR provider filtering, embedding, fallback, usage, and cancellation.
5. Start execution candidate and verify capability allow/ask/deny/outage,
   approval pause/grant/deny/delivery, read-only tools, cancellation, and direct
   MCP denial.
6. Start gateway candidate last. Verify its gRPC listener and full HTTP/SSE
   dependency readiness before any Frontend traffic.
7. Exercise Data Plane query embedding and Frontend default chat, selected
   tools, Browse, Plan, Agent Run Console, shipping, degraded memory, cost,
   approval, and MCP-not-configured paths.

### Gate 4 — Required positive and negative probes

Record command, expected/actual result, timestamp, artifact digest, principal,
real/sandbox classification, and limitation for every probe.

| Gate | Required pass condition |
|---|---|
| Listener/readiness | `:9090`, `:9092`, and `:9093` accept gRPC; readiness fails when their listener or required dependency is absent. |
| Authentication | No token, malformed token, wrong issuer/audience/scope/tenant/user, forged headers, and internal-credential misuse deny; exact valid identities pass. |
| Contract | Generated descriptors and old/new caller matrices pass; no required RPC or field disappears. |
| Cost | Cross-tenant/user reads deny; valid scoped reads work; usage is once-only and budget outage fails closed. |
| Capability | Unattested/stale/unhealthy/disabled deny; approval-required returns `ask`; only the dedicated global reporter can attest a global row; authenticated healthy allow reaches dispatch exactly once. |
| HITL | Approval-required work pauses; direct/generic resume cannot bypass; grant/deny are owner-bound; a durable managed-run receipt/reconciler prevents false success or stranded terminal state. |
| ZDR | Signed posture cannot be downgraded; no prompt/session/tool/memory/cache/trace/event/bridge content persists or reaches an ineligible provider; every unattested modality rejects before provider I/O. |
| Retrieval | Hybrid results distinguish ok/low-confidence/no-results/degraded; reformulation is bounded; unsupported routes are unavailable. |
| Letta | Tool ranking remains non-authoritative; memory search is real or explicitly degraded in readiness/API/UI. |
| MCP | RCE, SSRF, redirect, DNS/private range, raw secret, empty allowlist, auth, tenant, size, timeout, and HITL bypass cases deny; Visma says `not_configured`. |
| Observability | Logs/metrics/traces/audit/cost agree with outcomes and contain no secrets or ZDR content. |

### Gate 5 — Canary and canonical cutover

1. Declare numeric error, latency, cost, audit, and observation thresholds in
   the release ticket. Missing thresholds fail the gate.
2. Route only seeded test tenants to the candidate. Hold and observe each
   dependency boundary separately.
3. Shift Data Plane embedding, then execution, then gateway/Frontend traffic.
4. Keep compatibility code and both artifacts throughout the rollback window.
5. Remove old protocol behavior only in a later release after caller telemetry
   proves zero use.

## Rollback triggers

Rollback the most recent boundary immediately for any absent listener; false
readiness; auth/tenant bypass; approval execution before durable authorization;
false terminal success, stranded or duplicate continuation, or missing terminal
receipt; ZDR content leakage; ineligible provider I/O; hidden memory/retrieval
degradation; unexplained paid load; duplicate or missing usage; migration
inconsistency; missing audit identity; or a breached declared
error/latency/cost threshold.

## Rollback procedure

1. Stop traffic shifting and preserve evidence without secrets.
2. Switch only the last discovery/config change back to the retained rollback
   digest and configuration. Never build during rollback.
3. Leave additive backward-compatible migrations in place. For any approved
   destructive migration, use the separately rehearsed restore procedure.
4. Re-run listener, auth, chat/inference, embedding, capability, HITL, cost,
   retrieval, and ZDR smoke gates before resuming traffic.
5. Isolate the failed candidate. Do not delete evidence or fix forward under
   production traffic without a new gate review.

This procedure is not executable today because no rollback artifact exists.

## Exit criteria

This decision may be superseded only by a later dated record containing:

- exact candidate and separate rollback digests, checksums, source revisions,
  Compose hash, and migration set;
- complete RPC/caller/audience/scope/retention inventory;
- passing release suites and measured coverage;
- release-Postgres migration and restore rehearsal results;
- live positive/negative evidence for every Gate 4 row;
- exact canary thresholds, observation windows, and rollback rehearsal; and
- operator and affected plane-owner sign-off.

Until then: **do not promote or rebuild a production candidate**. The current
loopback-bound dirty-tree stack is integration evidence only and must not be
mistaken for an immutable production deployment or rollback target.
