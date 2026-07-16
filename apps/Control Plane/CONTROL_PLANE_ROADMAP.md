# Control Plane — Secure MVP Roadmap

Updated: 2026-07-16. Read `CONTROL_PLANE_STATUS.md` first.

The production-readiness program remains in the MVP phase. Enterprise readiness must not be planned as if it were the next active phase until every MVP acceptance gate below is proven.

## Completed in risk order

1. **Session authentication fail-closed**
   - Removed header-fallback identity.
   - Added explicit audience/scope-bound service credentials.
   - Added regressions for missing, malformed, expired, wrong issuer/audience/signature, and caller-supplied identity headers.
   - Live Session Core and gateway attacks return 401 with no data.

2. **User Core privilege hardening**
   - Removed caller-asserted `X-User-Role` authority.
   - Added scoped caller credentials and verified-claim authorization.
   - Bound gateway/Session self-service delegation to an HMAC-signed caller, audience, timestamp, method, URI, body digest, subject, org, and verified profile claims.
   - Removed caller-controlled email/name/avatar from the bearer profile path.
   - Normalized email lookup at the canonical repository boundary.
   - Live legacy-key plus asserted admin returns 401.

3. **Auth/Org durable projection foundation**
   - Added revisioned transactional organization and membership outboxes.
   - Fixed update re-enqueue, RLS GUC mismatch, monotonic CAS application, deletion tombstones, and reconciliation retries.
   - Fixed Postgres `BIGINT` string serialization to Org Core's numeric JSON contract using a tested fail-closed normalizer.
   - Added a five-second reconciliation deadline and gated acknowledgements/follow-on notifications on exact-revision compare-and-swap success.
   - Live worker recovered three organizations and all three membership rows automatically.

4. **Canonical membership routes**
   - Velion gateway invite/accept/remove/role-change routes now target Auth Core's Better Auth organization contract.
   - Org Core is a projection/domain authority, not a competing canonical membership writer.
   - Removed six legacy Org membership/role mutation registrations; live requests now return 404 while Auth's internal reconcile route remains available.

5. **Deletion and billing ordering**
   - Corrected Billing Core port to 3014.
   - Decoupled Billing and Org completion markers so either side effect can resume independently.
   - Added Billing tombstones/revision protection against delayed resurrection.

6. **Audit correctness and connectivity**
   - Fixed usage-summary SQL and added database coverage.
   - Recreated the live service on the inter-plane network.
   - Added readiness and Prometheus connectivity/event-age visibility.
   - Moved both buses to a file-backed JetStream stream with named durable audit/usage consumers, explicit ACK, bounded NAK, and durable DLQ+TERM handling.
   - Added migration-ledgered `(source_bus, source_stream_sequence)` inbox uniqueness so ACK-loss redelivery is a no-op, and made TERM conditional on confirmed durable DLQ publication.
   - Proved success, transient retry, malformed-event DLQ, five-delivery exhaustion, and stream update against embedded JetStream; proved inbox dedupe against disposable Postgres 16.

7. **Deployment safety**
   - Added migration ledgers/checksums/advisory locks and fixed Auth's constant-folded checksum failure.
   - Removed Auth image's recursive runtime `chown` bottleneck.
   - Rehearsed additive migration rollback paths in isolated transactional schemas.
   - All six Control containers and the gateway are healthy.

8. **Velion v3 Control contract repair**
   - Added no-redirect OAuth/OIDC/SAML callback proxies that preserve only required callback query/form data, redirects, cookies, and cache/content headers.
   - Added canonical Auth invitation acceptance with same-origin invitation links and a validated `returnTo` across login, verification, 2FA, OAuth, and SSO. The Auth-owned wrapper uses the canonical trusted-origin/rate-limited Better Auth router and real adapter transactions for member creation plus active-org selection; it is idempotent for committed/lost responses and recovers only from invitation/user/email/membership-bound evidence. Better Auth's preceding invitation-status transition remains outside that transaction and is tracked below.
   - Replaced fabricated workspace state with Auth-owned organization list/switch and membership list/invite/remove/role-change flows pinned to the live active membership.
   - Fixed default Nexi checkout selection and `charged`/`reserved` activation handling without exercising a live payment.
   - Normalized Audit Core's live row schema in the shared SPA client.
   - Restored fail-closed mandatory ZDR for Model and delegated audience tokens; the full Auth suite now passes.
   - Scoped locally sticky onboarding completion to the same user and organization; invitation acceptance now selects the accepted org and bypasses stale Better Auth cookie-cache state.
   - Made gateway/release-nginx access logs path-only, normalized invitation IDs embedded in paths, and scrubbed reset tokens from browser history.
   - Required canonical HTTPS public origins in production, derived checkout return URLs server-side, pinned executable/redirect origins, disabled unsupported Hyperswitch for the MVP, and bounded/sanitized Nexi confirmation.
   - Bound invitation acceptance rate limiting to the verified session actor: every Better Auth IP-precedence header is overwritten with a 120-bit HMAC-derived address, the Dragonfly increment/expiry is atomic, and cache failures fail operationally. A short-lived invitation-bound HMAC marker makes the Auth wrapper the only route to the canonical Better Auth mutation.
   - Replaced raw upstream transport, translation, SSE, and WebSocket failures with bounded gateway envelopes/events so internal URLs and upstream response bodies do not cross the browser boundary.
   - Rebuilt Auth Core, Billing Core, the gateway, and the SPA; all six Control services, the gateway, and the SPA are healthy.

9. **Scoped Org/Billing/Audit service principals — source and isolated verification complete**
   - Replaced generic inbound-key authority with required principal registries bound to audience, scope, plane, subject, request path/body, nonce, and short expiry.
   - Made Gateway, Auth projection/deletion, Session, and Integration callers pairwise distinct and fail startup on placeholder, legacy-key, or token reuse.
   - Split Control/Model/Application NATS into plane-runtime, Audit-consumer, and deployment-provisioner users with least-privilege publish/subscribe/JetStream ACLs.

10. **Better Auth invitation repair — isolated Postgres complete**
    - Added migration 017 and a bounded repair worker that records observed acceptance intent and reconciles both partial transaction shapes.
    - Preserved cancellation/deletion/removal tombstones and existing roles; rejected blind historical inference; reported repaired, superseded, and not-repairable rows separately.
    - Measured the repair module at 89.47% statements, 81.35% branches, 100% functions, and 90% lines with real Postgres tests.

11. **Lifecycle, ordering, deletion, and billing tombstone E2E — isolated complete**
   - Seven database-backed phases pass for invitation compensation, deletion resume, Org projection/plan ordering, outbox retry, scoped HTTP, Billing revision/tombstone, and durable retry/DLQ.
   - An expanded four-phase fresh-image Docker stack passes Auth repair/outbox -> Org convergence, canonical membership retry/role ordering/removal/audit cardinality, Billing-down deletion checkpointing, Billing restart/resume, and delayed-resurrection rejection.
   - Billing migration 0007 atomically binds a stable usage event ID, the aggregate row, and a durable Lago job. Exact replay is idempotent, conflicting ID reuse fails closed, worker leases recover after crashes, and account writes reject stale or same-revision-conflicting state.
   - Fixtures use unique disposable databases/containers with no host ports or persistent volumes; no existing tenant was mutated.

12. **Extra-plane Audit consumers — isolated current-image complete**
    - Provisioned fixed v2 durable consumers on Control, Model, and Application brokers and exposed per-bus readiness/lag.
    - Isolated live proof reported all three buses ready, persisted two audit and two usage events, denied Audit-principal producer publication, and returned 401 for unauthenticated HTTP.

13. **Release posture and static gates — source complete**
    - Production overlay renders 22 services with zero host-published ports, zero host-network services, and no development `env_file` on the six Control services, including the scoped shared broker and one-shot topology provisioner.
    - Broker-only environment wrapping preserves arbitrary generated NATS passwords while clients receive the raw value.
    - The five Go Control services pass full test/vet; Auth build, 372 active tests, full lint ratchet, and lint-contract tests pass; Gateway format/check/strict-Clippy and 288 tests pass; SPA lint/typecheck/358 tests/build pass.

14. **Historical ownerless-organization preflight — isolated Postgres complete**
    - Migration 018 reports and stops on unmapped, ambiguous, missing-member, changed-role, and stale-organization evidence.
    - Repair requires one explicit reviewed mapping to an existing canonical Auth member, locks and rechecks the evidence, updates projection/membership outboxes atomically, and records append-only operator audit evidence.
    - The workflow never invents an owner; valid multi-owner organizations remain unchanged.

15. **Canonical membership idempotency and audit ordering — source and isolated Postgres complete**
    - Migration 019 commits append-only invite/member audit intent with the canonical Auth mutation and orders member add, role change, and removal by the same per-subject revision as the Org projection.
    - Normalized duplicate invitations, same-role updates, and already-absent removals return stable no-op results without a second canonical mutation or audit event.
    - The container lifecycle runner includes invitation replay, role replay, removal replay, exact audit cardinality, and Auth-to-Org convergence.

16. **Residual producer durability and scoped shared broker — source/isolated complete**
    - Migration 020 transactionally captures user registration and provider-link events. The worker validates bounded payloads, orders provider-link after registration, claims with `SKIP LOCKED`, publishes with a stable message ID, acknowledges with an exact compare-and-swap, and exposes bounded retry/dead-letter state.
   - Org plan and Billing plan producers require PubAck before marking durable state delivered and reuse stable organization/revision message IDs under retry.
   - Billing usage ingress requires caller-stable identity and time, persists its de-duplication binding and Lago delivery job transactionally, and never derives replay identity from wall-clock time.
    - Runtime services use distinct scoped shared-broker users with token fallback disabled and no stream-administration rights. A one-shot provisioner owns topology; the temporary legacy bridge preserves/derives stable message IDs and acknowledges only after target confirmation.

17. **Auth lint and gateway Rust coverage gates — complete**
    - Auth's full lint command now ratchets an exact reviewed legacy baseline: every changed TypeScript file is checked with an empty suppression file, new debt and baseline drift fail, and no bulk fix is performed. The current run covers 135 files and 84 changed files; 212 violations remain baselined in seven untouched legacy files.
    - Gateway security coverage is enforced at 80% per selected module: audience tokens 98.82%, config 90.63%, membership boundary 89.62%, middleware 87.32%, and upstream 81.55%.

18. **Credential rollout and rotation controls — source complete**
    - A non-printing preflight rejects missing, short, placeholder, or reused values across 58 scoped broker/HTTP/gRPC credentials, validates 10 bounded registry/key/TLS files, and supports an explicit bridge-retired mode. Three valid profiles and 20 negative cases pass. Production file-backed secrets use a root-only handoff into app-owned `0600` files before the services drop to `appuser`.
    - The ordered rollout provisions topology, consumers, and producers before legacy revocation; rollback restores a prior scoped principal or pauses producers and retains durable outboxes. Release-mode producer token fallback is prohibited.
    - Exact legacy-token inventory is limited to the root environment template and the compatibility bridge mapping.

19. **Durable multi-org GDPR fanout and scoped Data consumer — source/isolated complete**
    - User Core migration 016 transactionally snapshots all active organization recipients before Auth/local cleanup and creates one deterministic child per org.
    - Parent completion is gated on a valid `AQENCIA_CONTROLPLANE` PubAck for every child; per-child retries, terminal state, lag, health degradation, and evidence-preserving bounded requeue are durable.
    - Documents API binds one fixed explicit-ACK durable with a dedicated principal, NAKs transient failures, and requires a DLQ PubAck before ACKing poison/exhausted source messages.
    - Embedded JetStream ACL proof denies Documents request forgery and topology administration. GDPR subjects are never copied to the legacy token broker.
    - Changed User GDPR files measure 80.8% and Documents GDPR measures 84.2%; full tests/vet and targeted race gates pass. Coordinated credential injection and live rollout remain operational.

20. **Auth readiness and reciprocal scoped authority — source/current-image complete**
    - Auth rejects missing, unreadable, mismatched, non-RSA, and sub-2048-bit signing keys before startup. Production readiness requires exactly one structural RSA/RS256 signing JWK; the current image reached it before lifecycle work began.
    - Auth gRPC, Auth internal HTTP/NATS, User gRPC, Auth→User, and User→Auth use file-backed exact ID/principal/audience/token/scope-or-method tuples. The real Auth transport matrix passes 9/9 denial/overlap cases.
    - Auth→User gRPC is CA-pinned TLS in production; User requires a TLS 1.3 certificate/key pair. The fresh real-authority integration passes and rejects a plaintext channel. File-backed Compose secrets are copied by a root-only handoff into app-owned `0600` files before both services drop to `appuser`.
    - Production User consumes the same Auth public-key secret and requires an explicit issuer; its developer host key mount and localhost issuer default are reset in the release overlay.
    - The fresh-image 4/4 lifecycle runner permits safe asynchronous delivery/retry while requiring durable state, bounded attempts, stable IDs, exact logical cardinality, and automatic cleanup.

21. **Per-core environment contracts — source complete**
    - Auth, User, Org, Audit, Billing, and Session each have an independent `.env` plus a tracked `.env.example`; the three previously missing local files were added without production credentials.
    - Development Compose layers each core's `.env` before its Docker-hostname override and no longer relies on a single root `.env` as a service `env_file`. The production override resets all six service env files and uses external secret/file inputs.
    - `scripts/control-service-env-contract-test.sh` verifies all six files, required Audit/Billing/Session keys, Compose wiring, ignored local files, and no root service env-file reuse.
    - The Control Plane root `.env` and `.env.example` are removed. `scripts/run-control-plane.sh` is the only supported local Compose entry point: it supplies service-local interpolation inputs and a temporary `0600` development fallback file, deletes that file on exit, and refuses the production overlay.

## Remaining dependency-ordered MVP work

### 0. Docker recovery — current-image isolation complete; local cross-plane dependency pending 2026-07-16

The prior containerd/BuildKit storage incident did not recur. The service-local runner built all eight current Control images and started Auth, User, Org, Billing, Session, and Audit Docker-healthy without resetting volumes or tenant data. Audit `/healthz` checks only the local Control DB/NATS dependencies; full `/readyz` remains 503 until the external Model/Application NATS endpoints (`model-nats` and `application-nats`) are attached, while the supervisor retries them. Auth JWKS, Org, Billing, and Session local probes returned 200.

The runner is local-development only: it creates disposable interpolation values in a temporary `0600` file, reuses the service-local database credential, and refuses the production overlay. The deployment authority still owns production secret-manager injection and cross-plane rollout.

**Safety retained:** never factory-reset, prune volumes, or initialize replacement databases as a routine recovery step. Preserve volumes and verify Postgres/NATS consistency after any future engine incident.

### 1. Coordinated integration credential rollout and rotation — external operator gate

The secure-MVP source, static, embedded-broker, disposable-Postgres, current-image Control, and real-authority Data/Velion contracts are green. Production execution is intentionally not claimed: an operator with integration secret-manager and deployment authority must generate pairwise-distinct values and registry files, run the non-printing preflight, deploy registries/servers before clients and consumers before producers, prove health/auth denial/PubAck/lag/outbox convergence from reviewed image digests, and only then revoke old credentials and the legacy bridge token.

**Rollback/safety:** restore the prior scoped principal/configuration as a coordinated rollout or pause producers and retain durable outboxes. Never re-enable generic producer token fallback, print credential material, or mutate real tenant lifecycle state for a probe.

### 2. Production integration acceptance review — external operator gate

MVP is accepted only when:

- Auth and Org converge exactly with zero unresolved canonical rows.
- All privileged service routes use scoped credentials.
- Audit ingestion is durable and observable.
- Isolated lifecycle/reordering/deletion E2E passes (currently green).
- All six services and gateway pass tests/build/static gates.
- Changed critical modules meet the coverage threshold.
- Docker health and auth matrices pass from the images built from the reviewed worktree.
- Docs contain no unverified production claim.

Source/isolation completion is not a production release certificate. The integration review must be run from images built from the reviewed worktree with the rotated credentials and must distinguish deployed, isolated, and historical evidence.

## Enterprise phase

**Deferred and not yet defined.** Create the separate enterprise-readiness plan only after the operator-owned production MVP review above is green. Do not mix workload identity/PKI, HA/DR, multi-region, SLOs/alerting, compliance evidence, zero-downtime migrations, chaos/recovery, enterprise SSO/SCIM, policy administration, or capacity/cost work into the MVP record unless required to close an active security gate.
