# Application Plane — Current Status

Last verified: 2026-07-13 CEST. This status separates checked source from the running workload. See `docs/core-research/plane-audit-2026-07-13.md` for commands, evidence grades, and limitations.

## Release decision

The Application Plane is **not a production-ready secure MVP** and is not enterprise-ready. Critical source remediations are tested, but none of the changed images, SQL migrations, or Convex functions were deployed in this pass. The current runtime cannot be correlated reliably with the working tree. An earlier 3.6 GiB root-capacity alarm cleared without action from this audit; the latest 19:13 CEST read-only check showed 32 GiB free on root/Data and 63 GiB on the workspace volume, with Docker retaining 35.67 GB of images and 4.17 GB of build cache. Capacity is no longer the immediate blocker, but each targeted build still needs an operator-defined margin; immutable rollback artifacts, migrations, secrets, approval/ZDR controls, and live gates remain unresolved.

The running stack was left untouched. No prune, daemon restart, database mutation, Convex apply, customer message, social publication, or paid provider call was performed.

## Corrected runtime facts

- `application-postgres` is Docker-healthy. The July 11 storage-corruption conclusion is stale for this runtime.
- `leads-core` is `linux/arm64`; the exact running container completed DNS/TLS and returned HTTP 200 from real Brreg. The old Rosetta/x86 diagnosis is stale.
- Conversation send-first/502 behavior and fail-closed ingest exist in source. Missing sender/thread mapping/unsupported provider now return 503 without storing a false sent message, and AI actions cannot be re-reviewed after leaving `suggested`. The running image revision is not provable.
- Conversation HITL is real. Social HITL is enforced by the worker at execution time.
- Social metrics and catalog gateway routes exist.
- Notification intake is `POST /api/v1/notification-requests`; callers were wrong.
- Information providers are real. Fabricated traffic volume/speed was a local data-honesty defect layered on real Atlas metadata.
- Convex's `change-me` fallback remains removed.
- Auth Core now owns an exact `(user, organization)` membership decision endpoint. It rejects ambiguous duplicate rows, and migration 016 refuses to add the unique index until an operator remediates any duplicates. User Core and the gateway fail closed on authority degradation in changed source; the migration and workloads are not live.

## Runtime and source state

| Surface | Running state | Changed source state | Release state |
|---|---|---|---|
| Control membership / Verevon gateway | Running revisions are older than this audit | exact Better Auth authority, dedicated credential, duplicate fail-closed/unique-index migration, User Core exact resolver plus denial-driven stale-projection removal, gateway-wide live membership result and signed downstream delegation | not deployed; migration duplicate preflight, real projection write, and cross-domain live negatives remain operator-gated |
| conversation-core / ingest | Healthy old containers; revisions absent/unverified and required new attestation variables absent by name | signed tenant/user/role delegation, per-org Auth Core service bearer, durable effect-bound Ed25519 write attestation, content-free intent/receipt ledgers with pre-provider retry phases, forward-only applied-0008 constraint audit/upgrade, send-first errors, stable Inbox retry key, fail-closed ingest, HITL tests green | credentials/migrations/source not deployed; callback/reconciliation, ZDR, and sandbox E2E remain blockers |
| social-core | Healthy | Worker-enforced HITL and metrics/catalog routes regression-tested | live authenticated UI/provider proof still pending |
| convex-core | Healthy backend with old bundle/subscriber | removal projection, tombstones, revoked sessions, import projection, public authz, durable consumer, dry-run reconciliation implemented; 28 tests green | not deployed or reconciled; authoritative signed/revisioned Control events missing |
| notification-core | Old `/health` 200, `/ready` 404; no mode/provider key | org-scoped typed recipient, signed delegation, membership gating, workflow allowlists, ZDR metadata-only ledger, explicit disabled/Novu mode | stale false-submission runtime still live; no authoritative membership writer/backfill, delivery/feed outbox, callback, or HA replay store |
| support-worker | Old container still running; source profile says opt-in | canonical typed/ZDR notification caller, timeout, strict response validation, failure propagation; 5 tests green | default-disabled; no current workflow supplies authoritative organization/user mapping |
| information-core | Healthy old traffic contract | nullable provenance-bearing observations; no invented measurements; traffic package 92.0% | service, Model formatter, and UI not deployed; legacy v2 incompatible |
| leads-core | Healthy arm64; real Brreg HTTP 200 | no architecture repair required | criterion met for current provider probe; reproducible multi-arch rollout evidence remains |
| insight-core | Healthy | no material change in this pass | daily-brief trigger/delivery path not proven |

## Release-blocking findings

1. Changed gateway source replaces caller-controlled scope across protected domains with an exact canonical membership result and signed tenant/user/role delegation, but it is not deployed. Conversation now uses a per-org Auth Core `ingestion` service bearer plus a durable Ed25519 attestation bound to tenant, intent/action, connection, provider operation, actor, exact payload digest, idempotency key, and short validity. Integration rejects opaque `approvalId`/legacy writes and atomically consumes the authorization in source. The service principal, private/public keys, migrations, and workloads remain unprovisioned/undeployed.
2. ZDR/retention is not proven across message bodies, attachments, AI drafts, social and notification payloads/providers, caches, logs, analytics, or events. Notification suppresses local ZDR persistence, but the full payload still reaches Novu without proven provider retention controls.
3. Conversation and integration now have content-free durable intent/receipt ledgers in source. Provably pre-provider Auth/OAuth failures use exact `retryable` or `pending` phases; Integration atomically moves `pending` to `executing` immediately before the provider call. `sending`/`executing`/`unknown` rows can still strand permanently, provider callbacks/delivered state are absent, and no dry-run reconciliation worker or operator compensation API exists.
4. Auth Core has a transactional, revisioned DB outbox to Org Core in changed source, but the subsequent cross-plane NATS membership fan-out remains unsigned, lacks a durable source event ID/revision in the consumed envelope, and is still best-effort. Convex consumer hardening cannot establish issuer authority by itself.
5. Notification source now has an organization boundary and typed user recipient, but no signed/revisioned Control-authority writer or safe backfill exists; the secure source therefore denies legitimate access. Provider/feed outbox, callback reconciliation, and durable replay protection remain open.
6. Conversation's primary message/event persistence still lacks authoritative tenant retention/ZDR policy; content-free ledgers do not make the message body or lifecycle event ZDR-safe.
7. The running notification, Convex, information, Model, Control, gateway, integration, and conversation callers are stale relative to source; immutable revision/rollback evidence is missing.
8. Full Inbox AI/HITL E2E is blocked by Model inference/retrieval health and the absence of authorized WhatsApp/Messenger/Novu sandbox recipients.

## Verification summary

- Convex: 28/28 tests, typecheck, lint.
- Auth Core: 15 suites / 145 tests and build pass; membership authority 100% statements/lines/functions and 91.11% branches. Migration 016 is contract-tested but not applied.
- User Core: full `go test -race ./... -count=1` and vet pass; exact denial reconciliation helper 100%, membership removal service 85.7%, and the new required gateway-registry validator 100%. Production-like startup now rejects an absent/malformed registry or missing/duplicate gateway policy. The PostgreSQL removal statement and deployment are not live-verified.
- Notification: `go test -race -cover ./...` passes; delegation 81.4%, notification 59.1%, HTTP 32.6%, database 41.0%.
- Information: `go test -race ./...`; traffic package coverage 92.0%, `Latest` 86.7%.
- Support-worker: build/typecheck and 5/5 contract tests.
- Verevon v3: typecheck and 5/5 targeted provenance tests.
- Conversation and social: full Go race suites pass. Conversation now has 293 tests; attestation 93.7%, integration client 81.6%, config 87.7%, and changed critical functions 83.3–100%. Integration Core's 18 tested packages pass under race; attestation is 87.2% and its changed API/store/config functions are 80–100%. Applied-0008 upgrade, fresh-install ordering, audit-failure rollback, and lock-before-check tests cover forward migration 0009 and concurrent migrators; `applyMigrations` is 88.1% and its lock/check helpers are 100%. Broader legacy packages remain below 80%; social domain remains 41.7%.
- Inbox UI: 6/6 targeted idempotency/honesty tests, full lint/typecheck/build. Coverage was not measurable because `@vitest/coverage-v8` is not installed.
- Verevon gateway: 242/242 all-target Rust tests, format, diff check, and scoped strict Clippy pass. Protected routers share the live exact membership decision; missing membership is 403, authority/malformed/mismatch is 503, Inbox performs one lookup, organization path IDs must match the verified tenant, and tenant-bearing onboarding routes reject forged scope before upstream. Changed-line coverage across the six scoped files is 96.83% and middleware is 98.65%; repository-wide line coverage is 41.80%. Repository-wide strict Clippy remains blocked by three unrelated pre-existing diagnostics.
- Model traffic formatter: four targeted Rust tests pass.

These are source/test claims, not deployment claims. Changed ledger functions meet the local 80% target, but several broader conversation/notification packages and the Inbox UI coverage gate do not; real-provider E2E, authenticated cross-tenant live negatives, and plane-wide ZDR proof are not complete.

## Operator references

- `docs/core-research/plane-audit-2026-07-13.md`
- `docs/runbooks/application-plane-safe-deployment-2026-07-13.md`
- `docs/runbooks/convex-membership-reconciliation-2026-07-13.md`
- `docs/runbooks/leads-core-native-build-deploy-2026-07-13.md`
- `APPLICATION_PLANE_ROADMAP.md`
