# Integration Core v2 Completion Phases

## Goal

Complete `integration-corev2` as Verevon's first-party integration platform: OAuth, tokens, provider lifecycle, safe metadata discovery, webhooks, sync orchestration, provider actions, token leases, audit, and cross-plane handoff. Nango and the old NestJS integration services become references only, not runtime dependencies.

## Completion Definition

`integration-corev2` is complete when Verevon v2, finspo-core, Data Plane v2, Model Plane, and future Application Plane services can use integrations through Verevon-owned APIs without Nango, raw provider-token exposure, or legacy proxy routes.

Required properties:

- Go remains the integration authority for OAuth, tokens, consents, connection state, provider actions, sync jobs, audit, and token leasing.
- Rust owns bounded hot paths where deterministic byte/event processing matters.
- Browser clients never receive provider tokens.
- Every provider action is named, allowlisted, capability-gated, audited, and org-scoped.
- Webhooks are signature-verified, replay-safe, normalized, stored, and published as safe events.
- Sync jobs are durable, resumable, observable, and handed off to the correct plane.

## Phase 1: Runtime Foundation

Status: complete.

Scope:

- Keep Go API service as default `integration-corev2` runtime on `:3026`.
- Keep Postgres as durable state for connections, tokens, sync jobs, webhooks, leases, consents, and audits.
- Keep in-memory repository for local throwaway development only.
- Add Rust `integration-webhook-normalizer-rs` as optional webhook hot-path sidecar.
- Add `INTEGRATION_WEBHOOK_HOTPATH_URL` with Go fallback when Rust is unavailable.
- Sanitize `.env.example` and rotate any leaked credential material.
- Add compose/runtime wiring for Go core + Rust normalizer.

Exit gates:

- `go test ./...`
- `go vet ./...`
- `cargo test --manifest-path services/webhook-normalizer-rs/Cargo.toml`
- `cargo clippy --manifest-path services/webhook-normalizer-rs/Cargo.toml -- -D warnings`
- `/health`, `/ready`, `/api/v1/providers`, and `/api/v1/webhooks/{provider}` work locally.

## Phase 2: Provider OAuth And Readiness

Status: mostly complete; direct OAuth session contract tests cover Microsoft,
Slack, Google, Notion, GitHub, Shopify, and Stripe. Reconnect callback
persistence reuses the active connection and upgrades scopes/capabilities
without changing the connection ID. Remaining work is deeper live-provider
callback/profile validation per provider.

Scope:

- Finish provider config readiness for Microsoft, Google, Slack, GitHub, Notion, Shopify, Stripe, Okta, and SCIM.
- Ensure `/api/v1/providers` exposes clear status, missing config, category, capabilities, and connect mode.
- Finish direct OAuth connect/reconnect/disconnect flows for all OAuth providers.
- Make Okta admin-token and SCIM inbound provisioning explicit non-OAuth adapters.
- Ensure callback URLs and provider scopes are documented and validated at startup.

Exit gates:

- Mock connect session tests for every OAuth provider.
- Provider readiness hides or disables unconfigured providers.
- Reconnect upgrades capabilities/scopes without losing audit history.
- Disconnect tombstones locally and attempts provider revocation when supported.

## Phase 3: Token Broker Hardening

Status: complete for the current rollout; trusted internal auth, required
consumer identity, consumer allowlists, token lease audit records, route rate
limiting, concurrent refresh singleflight tests, and Postgres advisory locks for
cross-replica refresh coordination are implemented.

Scope:

- Harden `/internal/connectors/token` for trusted internal callers only.
- Add consumer identity requirements for token leases.
- Add short lease TTLs, lease audit records, and rate limits.
- Ensure provider refresh flow is safe under concurrent refresh attempts.
- Add token lease capabilities for `finspo-core`, future `conversation-core`, Data Plane workers, and approved Application Plane services.

Exit gates:

- Browser/Bearer callers are denied.
- Cross-org token requests are denied.
- Concurrent refresh tests pass.
- Cross-replica refresh uses a distributed guard when Postgres is active.
- Token material never appears in logs, API responses, GDPR export, or NATS events.

## Phase 4: Webhook Hot Path Completion

Status: mostly complete; Rust hot-path sidecar, Go fallback, Stripe/Slack/
GitHub/Shopify signature checks, replay-safe event IDs, schema versioning,
cross-language shared fixtures, and compose wiring are implemented. Remaining
work is deeper provider-specific webhook extractors for providers that add
webhooks later.

Scope:

- Keep signature verification in Go while provider secrets stay there.
- Extend Rust normalizer for Stripe, Slack, GitHub, Shopify, Google, Microsoft, Notion where applicable.
- Add provider-specific replay keys and event-type extraction.
- Add webhook event schema versioning.
- Publish only safe NATS events with org, provider, event type, webhook event ID, and normalized metadata.
- Add replay window and timestamp checks for providers that support them.

Exit gates:

- Invalid signatures rejected.
- Duplicate deliveries are idempotent.
- Rust and Go fallback produce matching stable IDs for the same inputs.
- Webhook bursts do not block OAuth/token paths.

## Phase 5: Safe Discovery And Onboarding Evidence

Status: mostly complete; all supported OAuth providers expose a
`safe_metadata_only` discovery path. Microsoft and Google tests verify email
fallbacks and private Drive folder names are not exposed in onboarding
previews. Remaining work is broader provider-specific privacy fixtures for
Notion, GitHub, Shopify, and Stripe.

Scope:

- Complete safe discovery snapshots for all supported providers.
- Discovery returns only non-sensitive metadata: names, counts, capability flags, bounded public/allowed samples.
- No message contents, file contents, email contents, issue bodies, private channel names, or provider tokens.
- Feed onboarding/Settings/Knowledge proof surfaces through the same discovery contracts.

Exit gates:

- Provider-specific discovery tests strip sensitive fields.
- Onboarding can render provider proof without irreversible ingestion.
- Discovery failures are non-blocking and clearly represented.

## Phase 6: Provider Actions

Status: mostly complete; Microsoft, Google, Slack, GitHub, Notion, Shopify,
Stripe, and Okta have named action coverage with capability mapping. Write
actions require human approval metadata before token lookup. Remaining work is
fuller provider-specific write families.

Scope:

- Complete named action families for Microsoft, Google, Slack, GitHub, Notion, Shopify, Stripe, Okta, and SCIM.
- Split read actions from write/destructive actions.
- Require capabilities, consent, and human approval metadata for write actions.
- Remove or return `410 Gone` for any raw proxy behavior.
- Add action execution audit and result redaction.

Exit gates:

- Every action has capability mapping, provider implementation, tests, and audit.
- Sensitive writes require explicit capability and approval metadata.
- Old compatibility routes resolve to named actions only.

## Phase 7: Sync Jobs And Cross-Plane Handoff

Status: mostly complete; durable job state, sync event snapshots,
cancellation, retry, Microsoft `finspo-core` waiting handoff, Data Plane
handoff metadata, internal worker claim contracts, and idempotent checkpoint
source refs are implemented. Remaining work is provider-specific worker
execution depth, real Finspo/Data Plane source creation in their owned
services, and broader provider checkpoint fixtures.

Scope:

- Make sync jobs durable and resumable with checkpoints.
- Microsoft SharePoint/OneDrive handoff goes to `finspo-core`.
- Documents, knowledge, and source records hand off to Data Plane v2.
- Inbox/mail sync hands off to future `conversation-core`.
- Sync events support queued, running, waiting_provider, handoff_data_plane, completed, failed, cancelled.
- Add cancellation and retry policy.

Exit gates:

- Sync status is observable through API/SSE.
- Failed syncs are retryable without duplicate source creation.
- Data Plane and finspo handoff contracts are documented and tested.

## Phase 8: Enterprise Identity And Provisioning

Status: mostly complete; Okta admin read/lifecycle actions, durable hashed
per-org SCIM token CRUD, SCIM token last-used tracking, org-scoped SCIM bearer
auth, safe provisioning-event storage, and NATS forwarding are implemented.
Remaining work is downstream lifecycle orchestration into Control
Plane/Application Plane.

Scope:

- Complete Okta admin/API-token adapter.
- Complete SCIM inbound provisioning endpoints.
- Add org-scoped SCIM bearer token model instead of one global token.
- Store provisioning events as safe integration events.
- Forward identity lifecycle events to Control Plane/Application Plane where needed.

Exit gates:

- SCIM bearer auth is org-scoped.
- SCIM tokens are stored hashed and can be revoked.
- Replay and duplicate SCIM calls are safe.
- Okta lifecycle actions are admin-consent gated and audited.

## Phase 9: Verevon V2 Integration

Scope:

- Verevon v2 uses only BFF routes backed by `integration-corev2`.
- Settings shows provider readiness, connect, reconnect, disconnect, capabilities, consents, sync health.
- Onboarding uses safe discovery and sync progress from `integration-corev2`.
- Knowledge uses Data Plane records plus integration source state.
- Navbar/dashboard projections use integration profile endpoint or BFF-composed projection.

Exit gates:

- No active Nango browser/client logic.
- No frontend calls internal token broker.
- Playwright flows cover connect, reconnect, disconnect, sync, degraded provider, and onboarding proof.

## Phase 10: Observability, Security, And Compliance

Status: in progress; rate limits, GDPR export/delete basics, audit-core
forwarding, request ID propagation, and internal request metrics exist.
Tracing, dashboards, richer provider metrics, and runbooks remain.

Scope:

- Add structured logs, metrics, tracing, and request IDs.
- Add audit-core forwarding when available.
- Add rate limits for connect, token lease, action, webhook, and SCIM endpoints.
- Add GDPR export/delete orchestration coverage.
- Add operational dashboards for provider failures, token refresh failures, webhook replay, sync backlog, and action failure rates.

Exit gates:

- Security tests pass for cross-org denial, token exposure, webhook replay, SCIM auth, and write-action approval.
- GDPR export excludes token material.
- GDPR delete clears token material and tombstones connections.
- Runbook exists for provider outage, bad OAuth config, webhook replay spike, and token refresh failure.

## Phase 11: Legacy Cutover

Scope:

- Disable Nango and old integration-core v1 behind explicit legacy profiles only.
- Migrate all active callers to `integration-corev2`.
- Keep compatibility routes only where required, backed by named actions.
- Remove Nango-specific UI assumptions from onboarding/settings.
- Lock old raw proxy routes to `410 Gone`.

Exit gates:

- No active runtime dependency on Nango.
- No active Verevon v2 caller points at old integration-core v1.
- Compose/docs point to `integration-corev2` on `:3026`.
- Legacy profiles are off by default.

## Phase 12: Production Readiness

Scope:

- Load-test token broker, webhook intake, sync queue, and action execution.
- Verify HA behavior with multiple Go replicas and Rust normalizer replicas.
- Ensure migrations are forward-only and deployment safe.
- Add disaster recovery procedures for token vault, connection state, and replay events.
- Freeze public API contracts for Verevon v2 and internal planes.

Exit gates:

- `go test ./...`
- `go test -race ./...`
- `go vet ./...`
- Rust tests and clippy pass.
- Provider mock suite passes.
- Frontend onboarding/settings/knowledge/dashboard flows pass.
- Production runbook and rollback plan are complete.

## Not In Scope For Integration Core

- It does not store source content.
- It does not own Knowledge indexing.
- It does not own conversation truth.
- It does not make AI decisions.
- It does not expose raw provider proxy endpoints.
- It does not expose provider tokens to browsers.

## Next Immediate Tasks

1. Wire the `finspo-worker` runtime into the cutover compose stack and ensure
   the Microsoft source-discovery path supplies SharePoint `site_id` and
   `drive_id` into sync-job checkpoint/metadata.
2. Add downstream SCIM lifecycle orchestration into Control Plane/Application
   Plane.
3. Add tracing, dashboards, richer provider metrics, and production runbooks.
4. Run Verevon v2 BFF/UI cutover tests for onboarding, settings, Knowledge,
   dashboard, reconnect, disconnect, sync progress, and degraded providers.
