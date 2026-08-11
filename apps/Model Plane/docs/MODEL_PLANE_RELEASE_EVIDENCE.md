# Model Plane release evidence register

Status date: 2026-08-11 (Europe/Oslo)

This is the current release-evidence register for the Model Plane. It is
deliberately separate from historical audits and design proposals. A claim is
not release-ready merely because source code or a unit test exists.

## Evidence classes

| Class | Meaning |
| --- | --- |
| `source` | The implementation exists in the checked-out source. |
| `test` | A repeatable local test covers the claim. |
| `integration` | The claim passed against release-shaped dependencies. |
| `staging` | The claim was observed in a deployed, immutable candidate. |
| `candidate` | A signed artifact and its external runtime policy were verified. |
| `rollback` | The separately signed rollback artifact was restored and exercised. |
| `production` | The approved candidate was observed in production under the declared window. |

Only `candidate` and `rollback` evidence can support a production promotion.

## Current register

| Claim | Source | Test/integration | Candidate | Rollback | Current disposition |
| --- | --- | --- | --- | --- | --- |
| Release artifact v3 captures immutable inputs and partitions secrets | present | `release-contract-test.sh`, `release-runtime-partition-test.sh`, and `release-input-security-test.sh` pass | absent | absent | Gate exists; operator signing and accepted artifacts are still required |
| ZDR is issuer-monotonic and local durable boundaries fail closed | present | Rust gateway/inference/session tests pass | absent | absent | Safe failure is proven; no usable provider route is attested |
| One provider deployment is independently ZDR-attested | configuration gate only | no external provider attestation | absent | absent | Release blocker |
| Approval delivery resumes the exact approved action | worker, encrypted descriptor table, and descriptor protocol present | focused worker tests pass; disposable-Postgres lease-expiry/reclaim test passes with duplicate start receipt suppression | absent | absent | Local crash/recovery is proven; immutable-candidate and deployed effect observation remain required |
| Grounded answer uses authorized Data Plane evidence and citations | source path present | unit coverage present; customer journey E2E absent | absent | absent | Integration proof required |
| Normal chat persists the canonical conversation and resumes after disconnect/restart | message store, identity-scoped SSE buffer, and thread replay query present | gateway Redis restart/cross-device harness and disposable-Postgres thread replay test pass; full deployed chat journey absent | absent | absent | Local durability parity is proven; staging observation and canonical BFF ownership remain required |
| A user can durably erase their own thread and all Session Core thread/run evidence | owner-bound single/bulk Session Core delete RPCs and Model Gateway DELETE routes present | session-core test build passes; real-Postgres owner/scope/cleanup test is available but requires `DATABASE_URL` | absent | absent | Session Core cleanup is local-source complete; Letta semantic-memory erasure is a separate DSAR workflow and must not be implied by this receipt |
| Learning closes the run → feedback → reviewed candidate loop | producer/consumer paths and startup wiring present | durable-layer, composition, focused tests, and real-NATS trigger harness pass; live session-core/inference-core/LLM persistence absent | absent | absent | Integration proof required |
| Capability policy/health state is authoritative before dispatch | source path present | negative policy, health, and decision-correlation tests pass | absent | absent | Valid tools remain unavailable until a trusted reporter is deployed and observed |
| Per-call capability allow decisions are bound to the exact dispatch tuple | Ed25519 JWS signer/verifier and execution dispatch gate present | Go signer tests plus Rust valid/tampered/expired proof tests pass | absent | absent | Deployment keys, rebuilt images, and a live allow/verify observation remain required |
| Org safety policy controls PII redaction before provider dispatch | capability-core safety projection read in unary and SSE gateway paths; unknown policy state redacts fail-closed | `cargo test -p model-gateway --lib moderation::tests --no-fail-fast` (9 passed) | absent | absent | Source/test proof only; staging must observe an enabled and disabled tenant policy against a provider-bound prompt |
| JetStream topology survives a clean broker bootstrap | NATS provisioner owns required Model Plane streams/consumers | provisioner unit tests pass; live provisioner created topology and session-core consumers recovered | absent | absent | Staging must exercise restart/reconnect and bind the provisioner image to a candidate |
| Quality claims separate model capability from the integrated harness and retain bounded telemetry | versioned digest-only eval evidence contract present | contract test rejects missing/changed evidence, non-independent verifiers, and limit-reached false successes | absent | absent | Run both lanes against a private, rotating suite and bind their validated records to the signed candidate |

## Evidence captured in this checkout

- `scripts/tests/release-contract-test.sh` (`release contracts: ok`)
- `scripts/tests/release-runtime-partition-test.sh`
- `scripts/tests/release-input-security-test.sh`
- release secret-partition audit now includes the newly surfaced capability,
  application, cost, deep-research, and provider-attestation credentials;
  unreviewed credential-shaped Compose keys remain a hard failure
- `scripts/verify-durable-layer.sh`
- `cargo test -p model-gateway --lib` (781 passed, 0 failed); the focused
  post-change `stream_buffer` suite passed its non-ignored tests
- `scripts/tests/customer-proof-e2e-test.sh` (four scenarios passed using the
  isolated verification target)
- `scripts/tests/approval-recovery-postgres-test.sh` (lease expiry/reclaim and
  idempotent start receipt passed against disposable PostgreSQL)
- `scripts/tests/chat-thread-replay-postgres-test.sh` (thread-owned message
  replay passed against disposable PostgreSQL)
- `scripts/tests/chat-redis-durability-test.sh` (completed stream survived a
  Redis container restart; same-user resume passed and cross-tenant replay was
  rejected)
- `scripts/tests/learning-nats-trigger-test.sh` (RUN_COMPLETED subscription,
  delivery, review, and shutdown passed against a real NATS server; session and
  inference boundaries were fakes)
- `go test ./internal/server ./internal/policy` from
  `go/services/capability-core` (signed decision evidence contract passed)
- `cargo test -p execution-core capability_policy --no-fail-fast` (five policy
  tests passed, including valid, tampered, and expired Ed25519 evidence)
- `cargo test -p model-gateway --lib cancel_registry` (four tests passed,
  including cross-tenant and cross-user cancellation rejection)
- `cargo test -p model-gateway --lib skills::tests` (11 skill-cache and
  reconciliation tests passed)
- `cargo test -p model-gateway --lib retrieval::tests` (17 retrieval/graph/
  authorization tests passed, including packed pinned-fact ordering) plus the
  focused context-pack regression (one test passed)
- `cargo test -p model-gateway --lib confidence::tests` (10 confidence tests
  passed, including Data Plane low-confidence suppression)
- `cargo test -p model-gateway --lib moderation::tests --no-fail-fast` (9
  tests passed, including capability-core policy enforcement and fail-closed
  missing/malformed-policy behaviour)
- `cargo check -p model-gateway --lib` (resolved agentic permission posture is
  forwarded into RunAgentRequest.mode)
- `go test ./...` from `go/services/nats-provisioner` (stream/consumer topology
  reconciliation passed)
- Live NATS provisioner run logged `Model Plane JetStream topology ready`,
  followed by session-core `NATS consumer ready` and `orchestration NATS bridge
  ready` after the broker had initially reported missing streams
- `cargo test --manifest-path rust/Cargo.toml -p session-core approval_delivery
  --no-fail-fast` (18 tests passed)
- `go test ./internal/server ./internal/policy` from
  `go/services/capability-core` (passed)
- `cargo test -p execution-core health_attest` (10 health-attestation tests
  passed)
- `go test ./internal/learning/... ./internal/llmreviewer/... \
  ./internal/skillsink/... ./internal/sessionreview/...` from
  `go/services/capability-core` (passed)
- `scripts/tests/eval-evidence-contract-test.sh` (digest-only, ZDR-safe eval
  evidence contract passed; validates the model-only/integrated-harness lane,
  harness fingerprints, independent verifier identity, and bounded action,
  latency, token, cost, and recovery telemetry)

The current checkout also contains pre-existing uncommitted gateway changes.
Those files are intentionally listed as working-tree state, not candidate
evidence. A candidate must be built from a clean, isolated revision and must
bind its image and configuration digests to the signed root manifest.

## Required promotion evidence

Before a release claim can move to `candidate`:

1. Build from a clean revision with managed signing and verification keys.
2. Verify the candidate root, external runtime environment, migrations, and
   image lock without rebuilding.
3. Restore the images in a non-production environment and run the authenticated
   negative and positive boundary matrix.
4. Rehearse the separate rollback artifact with a distinct source revision and
   image payload.
5. Attach the four customer-proof scenarios: grounded chat, durable chat,
   effectful approval recovery, and learning review.
6. Record observation windows, thresholds, operator sign-off, and the exact
   artifact/rollback digests.

Until those rows are populated with the required evidence classes, the Model
Plane remains source-verified or locally integrated, not production-ready.

## External quality-grounding implications

The external references supplied for this review sharpen what the remaining
quality gates must measure:

- [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent) demonstrates
  that durable goals, child-agent state, schedules, compaction, bounded
  continuation, and recovery belong to the runtime owner—not to a UI cache.
  Its host-permission execution model is not an acceptable security boundary
  for Model Plane; Quarry-v2 and capability policy must remain authoritative.
- [PokeAgent (arXiv:2603.15563)](https://arxiv.org/abs/2603.15563) separates
  model capability from the harness and reports milestones, action count,
  wall-clock latency, tokens, and cost. Model Plane eval manifests should
  therefore fingerprint model/provider, prompt, tools, memory/retrieval,
  feedback/verifier, policy, and budgets, with model-only and integrated-harness
  controls.
- [ARC-AGI-3](https://arcprize.org/leaderboard) reinforces private rotating
  evaluation tasks, replayable action traces, independent completion checks,
  and contamination/ZDR controls. A local regression score or a successful
  harness run is not evidence of generalization or release readiness.

These are evaluation requirements, not a license to copy benchmark-specific
tools or to train on customer traces. The local quality milestone is now
implemented as `scripts/eval-evidence.sh`: it accepts only digest-only suite
and harness identities, requires a distinct independent-verifier identity, and
checks per-attempt action, latency, token, cost, and recovery bounds. It does
not turn a local fixture into a quality result. The remaining milestone is to
run both lanes against a private rotating suite and attach the validated
evidence records to a signed candidate.

## Recommended next-step order

1. **Create the external candidate gate.** Build from a clean revision, sign
   the root/image/config manifest, attach an independently verified provider
   ZDR route, and retain a distinct signed rollback artifact. The dirty
   working tree and local fixture tests cannot satisfy this gate.
2. **Prove grounded chat in staging.** The direct gateway and tool retrieval
   paths now request Data Plane's token-budgeted `context_pack` (including
   pinned facts) and retain only local citation/injection framing. The live
   positive proof is still blocked: the running Data Plane rejected the
   verified bearer with `not_member` despite the Control Plane databases
   containing an owner membership. Resolve that authority/projection mismatch,
   then observe non-empty authorized evidence plus citations in a real turn.
3. **Exercise effectful approval recovery.** Run the encrypted descriptor,
   lease-reclaim, receipt-before-effect protocol with the real execution
   worker, then kill the worker after the receipt and verify exactly one
   external effect and a durable audit trail.
4. **Make the conversation owner canonical.** Transcript reads now go through
   the durable Model Gateway/Session Core message owner; the frontend BFF no
   longer stores or serves a Redis transcript cache. Pin/title/preview remain
   owner-backed projections. Staging still needs the authenticated customer
   journey, DSAR/erasure observation, and activity/run projection evidence.
5. **Close the learning loop live.** The real NATS trigger is proven and the
   consumer starts, but the live reviewer currently fails closed because no
   provider deployment has verified ZDR support. Register and independently
   attest a provider route, then observe session-core/inference-core review,
   candidate persistence, and next-turn injection.
6. **Add signed per-call capability evidence.** The policy contract now carries
   a short-lived Ed25519 JWS binding capability/version, tenant, run, agent,
   scope, decision, reason, and budget; execution-core verifies it before an
   allow reaches dispatch. Production still requires key provisioning, image
   rebuild, and a live allow/verify trace.
7. **Attach the two eval lanes.** Run `scripts/eval-evidence.sh validate` for
   the fixed-policy `model-only` control and `integrated-harness` lane, then
   use `compare` to prove they use the same suite revision and fixture IDs.
   Each evidence directory carries only task/config/trace fingerprints and
   bounded result telemetry; the private task corpus and any customer material
   stay out of the candidate. Bind both validated records to the signed
   candidate before reporting an improvement claim.

## Six-sequence execution status (2026-08-11)

The current execution sequence is intentionally evidence-first:

1. **Release evidence before features — complete locally.** The release
   contract, runtime partition, input-security, durable-layer, and gateway
   focused test gates are repeatable. A signed candidate and rollback rehearsal
   are still operator-controlled gates, so this item is not a production
   promotion by itself.
2. **Release blockers — local recovery proof complete; external attestations pending.**
   ZDR downgrade prevention, durable mutation scoping, browser risk
   backstops, encrypted continuation descriptors, and approval lease recovery
   are covered in source/tests plus disposable-dependency harnesses. The
   remaining blockers are an independently verified provider ZDR route, a
   deployed crash/recovery observation, and signed candidate/rollback artifacts.
3. **Customer-proof E2E — local boundary harness complete.** Run
   `scripts/tests/customer-proof-e2e-test.sh` for grounded context, disconnect
   persistence, monotonic ZDR, and cross-tenant approval isolation. These are
   release-shaped integration tests; staging must still add the effectful
   approval crash/recovery and learning-review journeys against an immutable
   artifact and real cross-plane dependencies.
4. **Canonical chat durability — local implementation and restart proof complete.**
   Thread replay now includes direct thread-owned creation/message events, and
   stream buffers are identity-scoped with terminal-cursor semantics. Redis
   restart and PostgreSQL thread replay are executable local proofs; the
   frontend BFF transcript cache has been removed in favor of the durable
   Model Gateway/Session Core owner. The remaining parity gate is an
   authenticated deployed customer journey plus activity/DSAR projection
   observation.
5. **Governed capability intelligence — health reporter and signed evidence path present.**
   Policy responses carry a stable decision correlation ID, exact capability
   version, and a short-lived Ed25519 decision proof; execution-core verifies
   the proof before allowing dispatch and retains the measured heartbeat
   backstop. Key provisioning/rebuild and live ranking/allow observations
   remain open, while the NATS learning trigger has a real-bus harness but
   still needs real cross-service/LLM observation.
6. **Large bets — deferred behind gates.** GraphPlan, RL/continual learning,
   and broad capability expansion remain research or milestone work. They may
   not be promoted by a local score or an unverified external benchmark.

The authoritative “done” rule is the evidence class table above: local source
and tests can close engineering work, but only staging/candidate/rollback
evidence can close release work.

## Follow-up pass — capability-registry durability (2026-08-11)

The next local parity pass closed the MCP cache/share gap identified by CR-01,
CR-02, CR-03, and AZI-1: authenticated list/chat paths now hydrate a tenant's
MCP projection from capability-core, replace stale local entries fail-closed,
restore owner/share metadata, and persist share changes through the durable MCP
catalog with rollback on rejection. The reconcile consumer remains a low-
latency removal optimization; a JetStream/durable-consumer revocation proof,
restart across multiple replicas, and Control-Plane `resource_grants`
consolidation remain release/staging work. Hydration never imports credentials;
OAuth dispatch continues through capability-core's encrypted token resolver.
