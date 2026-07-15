# velionv3 Endpoint Map

Complete catalog of backend endpoints across the five reachable planes, annotated for velionv3 consumption.
Generated 2026-06-10 from `docs/core-research/*` (per plane) plus route registrations verified in source
(velion-gateway-rs `src/`, model-gateway `src/http_routes.rs`, quarry-edge `src/routes.rs`, integration-corev2
`internal/api/server.go`, each Control Plane core's `internal/http/server.go`, etc.).

**Architecture rule (fixed):** velionv3 (SolidJS SPA, no BFF) talks ONLY to `velion-gateway-rs` (Application
Plane, port 3185). Every endpoint below tagged with a relevance other than `none` is consumed *through* the
gateway, never directly from the browser. See `gateway-integration-plan.md` for the gateway route surface.

## Legend

| Field | Values |
|---|---|
| `v3` (relevance) | `core` = needed by a live v3 workspace surface · `onboarding` = needed by the onboarding/auth flow · `later` = plausible future use, **do not wire yet** · `none` = internal/inter-plane/dev only, **never wire for the SPA** |
| Transport | `HTTP` (JSON), `SSE` (server-sent events), `WS` (WebSocket), `gRPC`, `NATS` |
| Auth | What the *caller of that service* must present. For everything the SPA uses, the gateway is the caller and holds the secret. |

> ⚠️ Rows marked `later` or `none` are flagged so nobody wires them prematurely. `none` rows are listed for
> completeness/trust-boundary awareness only.

---

## 1. Control Plane

### 1.1 auth-core (NestJS + Better Auth) — HTTP :3011, gRPC :50011, NATS (controlplane-nats :4223)

The only user-credential authority. Better Auth session cookie (or bearer) on user routes; `X-Internal-Api-Key` on `/internal/*`. Most "enhanced" routes are POST-RPC style.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| ANY | `/api/auth/*` | Better Auth native catch-all (sign-up/in/out, session, OAuth flows, callbacks) | session cookie | HTTP | onboarding |
| POST | `/api/v2/auth/signUp` | Enhanced email/password signup | none (public) | HTTP | onboarding |
| POST | `/api/v2/auth/signIn` | Enhanced sign-in | none (public) | HTTP | onboarding |
| POST | `/api/v2/auth/signOut` | End session | session | HTTP | core |
| POST | `/api/v2/auth/getSession` | Resolve current session (primary session check) | session | HTTP | core |
| POST | `/api/v2/auth/sendEmailVerification` | Send verification mail (mock Resend in dev) | session | HTTP | onboarding |
| POST | `/api/v2/auth/verifyEmail` | Verify email token | token | HTTP | onboarding |
| POST | `/api/v2/auth/oauth/initiate` | Start OAuth/social login (mock URLs in dev) | none (public) | HTTP | onboarding |
| POST | `/api/v2/auth/password/check-strength` | Password strength check (HIBP TODO) | none (public) | HTTP | onboarding |
| POST | `/api/v2/auth/sendPasswordReset` | Send password-reset email | none (public) | HTTP | core |
| POST | `/api/v2/auth/resetPassword` | Complete password reset | token | HTTP | core |
| POST | `/api/v2/auth/profile/getProfile` | Get auth-side profile | session | HTTP | core |
| POST | `/api/v2/auth/profile/updateProfile` | Update auth-side profile | session | HTTP | core |
| GET | `/api/v2/auth/user/profile` | Profile fetch (NatsAuthController convenience) | session | HTTP | core |
| POST | `/api/v2/auth/consent/get` | Read consent state (persistence TODOs) | session | HTTP | onboarding |
| POST | `/api/v2/auth/consent/update` | Update consent | session | HTTP | onboarding |
| POST | `/api/v2/auth/consent/withdraw` | Withdraw consent | session | HTTP | **later** |
| POST | `/api/v2/auth/organization/create` | Create org via auth surface (bridges org-core events) | session | HTTP | onboarding |
| POST | `/api/v2/auth/organization/list` | List caller's orgs | session | HTTP | core |
| POST | `/api/v2/auth/organization/invite-member` | Invite member | session | HTTP | core |
| POST | `/api/v2/auth/organization/switch-active` | Switch active org on session | session | HTTP | core |
| POST | `/api/v2/auth/2fa/enable` / `disable` / `verify` | 2FA (placeholder logic) | session | HTTP | **later** |
| POST | `/api/v2/auth/otp/email/send` / `verify` | Email OTP (placeholder/mock) | session | HTTP | **later** |
| POST | `/api/v2/auth/otp/sms/send` / `verify` | SMS OTP (mock Twilio; dev auto-succeeds) | session | HTTP | **later** |
| POST | `/api/v2/auth/passkey/create` | Create passkey (placeholder) | session | HTTP | **later** |
| POST | `/api/v2/auth/api-keys/create` / `list` / `delete` / `rotate` | API key management (placeholder-backed) | session | HTTP | **later** |
| POST | `/api/v2/auth/api-keys/validate` | Validate API key (service-facing) | internal | HTTP | none |
| POST | `/api/v2/auth/bearer/validate` / `create` / `revoke` / `list` | Bearer token ops (list unimplemented) | internal | HTTP | none |
| POST | `/api/v2/auth/admin/users/*` (`list`,`get`,`create`,`suspend`,`set-role`,`ban`,`sessions`,`remove`) | Admin user ops (placeholder paths) | admin session | HTTP | **later** |
| POST | `/api/v2/auth/admin/organizations/list`, `/api/v2/auth/admin/system/stats` | Admin org list / system stats | admin session | HTTP | **later** |
| GET | `/api/v2/auth/debug-router` | Debug route listing | — | HTTP | none |
| GET | `/api/v2/organizations` | List orgs (enhanced oRPC controller) | session | HTTP | core |
| GET | `/api/:audience/token` | Mint short-lived plane token (model-plane, quarry, data-plane, application-plane) from session | session + internal key | HTTP | core (gateway-internal) |
| POST | `/api/:audience/internal-token` | Plane token service-to-service | internal key | HTTP | none |
| GET | `/api/model-plane/token` | Model Plane token mint (duplicates `:audience/token`) | session + internal key | HTTP | core (gateway-internal) |
| POST | `/api/model-plane/internal-token` | Model Plane internal token | internal key | HTTP | none |
| GET | `/api/convex-auth/jwks` | JWKS for Convex bridge | none | HTTP | none |
| GET | `/api/convex-auth/token` | Convex auth token (only if v3 keeps Convex realtime) | session | HTTP | **later** |
| GET | `/users/me` | Demo/aux current user (duplicates user-core) | session | HTTP | none |
| GET | `/users/public`, `/users/optional`, `/users/protected` | Auth-guard demo routes | varies | HTTP | none |
| POST | `/users/set-password` | Set password for OAuth-created accounts | session | HTTP | **later** |
| POST | `/internal/agent-signup`, `/internal/oauth/token`, `/internal/oauth/refresh` | Internal signup/OAuth exchange | internal key | HTTP | none |
| GET | `/docs/hub`, `/docs/better-auth`, `/orpc/openapi.json`, … | OpenAPI/Swagger surfaces | none | HTTP | none |
| gRPC | `TokenValidationService.ValidateToken` (auth.v1, dataplane.auth.v1) | Token/session validation — recommended gateway validation path | mTLS/plane | gRPC :50011 | none (gateway-internal) |
| gRPC | `AuthService.SignUp/SignIn/SignOut/GetCurrentUser/HealthCheck` | Auth ops for inter-plane callers | plane | gRPC | none |
| NATS | `session.validate`, `service.authenticate`, `health.check` | Req-reply session/service auth | NATS creds | NATS | none |
| NATS | `aqencia.controlplane.*`, `velion.audit.v1.control.*` | Event fan-out (audit only with org_id) | NATS creds | NATS | none |

### 1.2 user-core (Go) — HTTP :3012, gRPC :50012

Internal-key gated; trusts identity forwarded by the gateway (Better Auth session validated upstream).

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health` | Health | none | HTTP | none |
| GET | `/api/v1/users/me` | Current user profile (canonical) | internal key + identity hdrs | HTTP | core |
| GET | `/api/v1/users/current` | Alias of `/users/me` | internal key | HTTP | **later** |
| PATCH | `/api/v1/users/me` | Update profile (onboarding profile step + settings) | internal key | HTTP | onboarding |
| DELETE | `/api/v1/users/me` | Delete account | internal key | HTTP | **later** |
| POST | `/api/v1/users/onboarding/complete` | Authoritative onboarding-complete flag | internal key | HTTP | onboarding |
| GET | `/api/v1/users/me/onboarding-state` | Read wizard step + state blob (multi-device resume) | internal key | HTTP | onboarding |
| PUT | `/api/v1/users/me/onboarding-state` | Upsert wizard step + state blob | internal key | HTTP | onboarding |
| GET | `/api/v1/users/by-email/:email` | Lookup by email (internal helper) | internal key | HTTP | none |
| GET | `/api/v1/users/:id` | Get user by ID (authz hardening TODO) | internal key | HTTP | **later** |
| GET | `/api/v1/me/session-context` | Post-login routing context (active org id) — org resolution source | internal key | HTTP | onboarding |
| POST/GET | `/api/v1/api-keys` | Create / list user API keys | internal key | HTTP | core |
| DELETE | `/api/v1/api-keys/:id` | Revoke API key | internal key | HTTP | core |
| GET/PATCH | `/api/v1/preferences` | User preferences | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/appearance` | Appearance settings (used by onboarding theme) | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/language` | Language settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/privacy` | Privacy settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/notifications` | Notification settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/security` | Security settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/accessibility` | Accessibility settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/ai` | AI/Copilot settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/storage` | Storage & sync settings | internal key | HTTP | core |
| GET/PUT | `/api/v1/settings/voice` | Voice mode, dictionary, snippets, local/private retention policy | internal key | HTTP | core |
| GET/POST | `/api/v1/calendar/events` | Navbar calendar state / lightweight events | internal key | HTTP | core |
| POST | `/api/v1/calendar/notes` | Calendar note | internal key | HTTP | core |
| POST | `/api/v1/support/requests` | Support request from navbar | internal key | HTTP | core |
| GET | `/api/v1/providers` | Linked OAuth provider identities | internal key | HTTP | core |
| POST | `/api/v1/providers` | Link provider (internal/NATS pipeline) | internal key | HTTP | none |
| POST | `/api/v1/internal/memberships/ensure`, `/api/v1/internal/users/enrich-from-provider` | Auth-pipeline hooks | internal key | HTTP | none |
| gRPC | `UserService`, `DocumentAccessService` (:50012) | Inter-plane profile/ACL ops (gaps: device mgmt, ACL events) | plane | gRPC | none |

### 1.3 org-core (Go) — HTTP container :8080 / host :18080; gRPC :19090-91 health-only

Internal-key style service auth; identity forwarded from gateway. RBAC/member management exists ONLY on the compat `/orgs/*` family.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health` | Health | none | HTTP | none |
| POST | `/api/v1/auth/login` | Login compat bridge to auth-core — do not use | internal | HTTP | none |
| GET | `/api/v1/users/me` | Current-user compat (duplicates user-core) | internal | HTTP | none |
| GET | `/api/v1/organizations` | List caller's orgs (org-existence check post-login) | internal key | HTTP | onboarding |
| POST | `/api/v1/organizations` | Create organization | internal key | HTTP | onboarding |
| GET | `/api/v1/organizations/:id` | Org details | internal key | HTTP | core |
| GET | `/api/v1/organizations/:id/entitlements` | Org entitlements (feature gating/paywall) | internal key | HTTP | core |
| GET | `/api/v1/organizations/:id/members/search` | Search members | internal key | HTTP | core |
| POST | `/api/v1/organizations/:id/plan` | Update org plan | internal key | HTTP | onboarding |
| PATCH | `/api/v1/organizations/:id/brreg` | Verify/attach BRREG registry data | internal key | HTTP | onboarding |
| GET | `/api/v1/brreg/search` | Search BRREG companies by name | internal key | HTTP | onboarding |
| GET | `/api/v1/brreg/:orgnr` | BRREG lookup by org number | internal key | HTTP | onboarding |
| GET | `/orgs`, `/orgs/me`, `/orgs/:id`, `/orgs/:id/entitlements` | Compat duplicates (prefer `/api/v1`) | internal key | HTTP | **later** |
| POST | `/orgs`, `/orgs/:id/plan` | Compat duplicates of create/plan (gateway currently uses `POST /orgs`) | internal key | HTTP | **later** |
| PATCH | `/orgs/:id/capabilities` | Update org capabilities (compat family only) | internal key | HTTP | core |
| GET | `/orgs/:id/members` | List members (compat family only) | internal key | HTTP | core |
| POST | `/orgs/:id/members/invite` | Invite member | internal key | HTTP | core |
| DELETE | `/orgs/:id/members/:userId` | Remove member | internal key | HTTP | core |
| GET | `/orgs/:id/members/search` | Search members (compat duplicate) | internal key | HTTP | **later** |
| GET | `/orgs/:id/roles/catalog` | RBAC capability catalog | internal key | HTTP | core |
| GET/POST | `/orgs/:id/roles` | List / create roles | internal key | HTTP | core |
| PATCH/DELETE | `/orgs/:id/roles/:roleName` | Update / delete role | internal key | HTTP | core |
| PATCH | `/orgs/:id/members/:userId/role` | Assign member role | internal key | HTTP | core |
| GET/POST | `/internal/orgs/by-tenant`, `/internal/orgs/ensure-from-tenant`, `/internal/orgs/:orgId/onboarding/state` | Internal tenant/onboarding hooks | internal key | HTTP | none |
| gRPC | health/reflection only | No business services registered — do not integrate | — | gRPC | none |
| NATS | auth/org/user/session bridge + shared org events | Event plumbing | NATS creds | NATS | none |

### 1.4 billing-core (Go) — HTTP :3014, gRPC :50013 health-only; Lago :3016 / Stripe adapters

Internal-key gated (`X-Internal-Api-Key`); all routes org-scoped by `:orgId`. **Gateway must enforce org membership before proxying.**

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health` | Health | none | HTTP | none |
| GET | `/api/v1/billing/orgs/:orgId/account` | Billing account (plan, trial status) | internal key | HTTP | onboarding |
| PUT | `/api/v1/billing/orgs/:orgId/account` | Upsert billing account (trial start on org creation) | internal key | HTTP | onboarding |
| POST | `/api/v1/billing/orgs/:orgId/usage` | Record usage (normally NATS-fed) | internal key | HTTP | none |
| GET | `/api/v1/billing/orgs/:orgId/entitlements/:feature` | Single feature entitlement (paywall gate) | internal key | HTTP | core |
| GET | `/api/v1/billing/orgs/:orgId/quotas/:metric` | Quota status | internal key | HTTP | core |
| POST | `/api/v1/billing/orgs/:orgId/invoices` | Create invoice (Lago/Stripe facade) | internal key | HTTP | **later** |
| POST | `/api/v1/billing/orgs/:orgId/checkout-session` | Stripe checkout session for paid upgrade | internal key | HTTP | onboarding |
| gRPC | health/reflection only (:50013) | — | — | gRPC | none |
| NATS | sub `usage.>` + org lifecycle; pub `aqencia.controlplane.billing.*` | Usage ingest, trial-expiry sweep | NATS creds | NATS | none |

### 1.5 session-core (Go) — HTTP :3017, gRPC :50017 (listener, zero services)

Internal-key startup gate; Redis read-through cache on snapshots.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health` | Health | none | HTTP | none |
| GET | `/api/v1/sessions/current` | Control Session snapshot (user+org+billing+entitlements) — natural shell bootstrap | internal key | HTTP | core |
| POST | `/api/v1/sessions/refresh` | Force snapshot refresh after plan/org changes | internal key | HTTP | core |
| POST | `/v1/sessions` | Legacy: create model session via command bridge | internal key | HTTP | **later** |
| GET | `/v1/sessions/:id/state` | Legacy: session state | internal key | HTTP | **later** |
| GET | `/v1/sessions/:id/events` | Legacy: SSE session events | internal key | SSE | **later** |
| POST | `/v1/sessions/:id/messages` | Legacy: send message | internal key | HTTP | **later** |
| POST | `/v1/sessions/:id/approvals/:approval_id` | Legacy: resolve approval | internal key | HTTP | **later** |
| POST | `/v1/sessions/:id/resume` | Legacy: resume session | internal key | HTTP | **later** |
| NATS | session commands/events, `app.session.entitlements_changed` | Command routing to Model Plane; Convex mirror | NATS creds | NATS | none |

### 1.6 audit-core (Go) — HTTP :8187

Internal API key (`X-Internal-Api-Key` or `X-Api-Key`); reads require `org_id` query param; **trusts caller-supplied org_id** — gateway must inject the validated org.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/healthz`, `/readyz` | Liveness / readiness | none | HTTP | none |
| GET | `/v1/audit` | Org-scoped audit events (since/until/event/user_id/limit) | internal key | HTTP | **later** (dashboard) |
| POST | `/v1/audit` | Direct audit ingest (normally NATS) | internal key | HTTP | none |
| GET | `/v1/usage` | Org-scoped usage records | internal key | HTTP | **later** |
| GET | `/v1/usage/summary` | Summarised usage (usage dashboard) | internal key | HTTP | **later** |
| NATS | sub `velion.audit.v1.>`, `velion.usage.v1.>` | Best-effort ingest (no retry/NAK) | NATS creds | NATS | none |

---

## 2. Data Plane v2

**Secure-MVP boundary (verified in source/tests 2026-07-15):** every interactive
Data route below requires an RS256 `aud=data-plane` bearer minted from the
verified Velion session. The gateway derives the tenant from canonical session
membership, mints the bearer through Auth Core, forwards the authoritative
tenant only, and returns 503 when delegation is unavailable. It never falls
back to the fleet-shared internal key for user reads. Data services independently
verify issuer, audience, signature, time claims, identity, scopes where
applicable, and tenant equality; caller-selected headers/body/path values cannot
widen authority.

### 2.1 documents-api-go — HTTP :8010 (container dpv2-documents-api)

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/readyz` | Probes | none | HTTP | none |
| GET | `/v1/documents` | List canonical documents (knowledge library) | Bearer; verified user visibility or `documents:read` service | HTTP | core |
| POST | `/v1/documents` | Create document (Ingestion Plane write path — not SPA) | Bearer; `documents:write` service scope | HTTP | none |
| POST | `/v1/documents/bulk` | Bulk ingest (Ingestion batch) | Bearer; `documents:write` service scope | HTTP | none |
| GET | `/v1/documents/{documentID}` | Single document metadata | Bearer; tenant + viewer visibility pinned | HTTP | core |
| DELETE | `/v1/documents/{documentID}` | Delete document (user-initiated removal) | Bearer; tenant + actor pinned | HTTP | **later** |
| GET | `/v1/sources` | Source facet data | Bearer; tenant + visibility pinned | HTTP | none (gateway source-list intentionally uses `/v1/documents`) |
| POST | `/v1/source-objects`, `/v1/source-objects/delete` | Source-object lifecycle (ingestion-internal) | Scoped service bearer | HTTP | none |
| GET | `/v1/source-objects/duplicates` | Duplicate inspection | Bearer; `documents:read` service scope | HTTP | **later** |
| NATS | document lifecycle outbox → JetStream | Consumed by index-engine-rs | — | NATS | none |

### 2.2 retrieval-engine-rs — HTTP host :8014 → container :8004; gRPC host :50062 → :50052

Verified Bearer JWT only. Fleet-shared API keys and header-only tenant identity
are rejected. User membership is resolved through the versioned Control decision
contract; calls fail closed when token issuance or policy authority is unavailable.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/readyz` | Probes | none | HTTP | none |
| POST | `/v1/retrieve` (alias `/v1/retrieve/hybrid`) | Main hybrid retrieval (dense+sparse+rerank) | Verified bearer | HTTP | core |
| GET | `/v1/retrieval/{trace_id}` | Stored retrieval trace (citation provenance) | Verified bearer; tenant + actor pinned | HTTP | core |
| POST | `/v1/retrieve/graph` | Graph-aware retrieval | Verified bearer | HTTP | core |
| POST | `/v1/retrieve/wiki` | Wiki ANN retrieval | Verified bearer | HTTP | core |
| POST | `/v1/retrieve/contradictions` | Contradicting claims | Verified bearer | HTTP | **later** |
| POST | `/v1/retrieve/timeline` | Timeline retrieval | Verified bearer | HTTP | **later** |
| POST | `/v1/retrieve/pack` | Context packing (Model Plane consumer — not SPA) | Scoped service bearer | HTTP | none |
| POST | `/v1/retrieve/sources` | Resolve sources behind results (citation lists) | Verified bearer | HTTP | core |
| POST | `/v1/retrieve/freshness` | Freshness/staleness signal | Verified bearer | HTTP | **later** |
| POST | `/v1/retrieve/chunks` | Fetch chunks (expand citation to text) | Verified bearer | HTTP | core |
| POST | `/v1/retrieve/compare` | Compare retrieval configs (debug/eval) | Verified bearer | HTTP | **later** |
| GET | `/v1/index/versions` | List index versions | Verified bearer | HTTP | **later** |
| POST | `/v1/admin/cleanup/orphans` | Admin cleanup | Dedicated scoped bearer | HTTP | none |
| POST | `/v1/knowledge/search` | Tool-style search — knowledge page and navbar search | Verified bearer | HTTP | core |
| gRPC | `RetrievalService` (incl. server-stream `RetrieveStream`), `KnowledgeService`, `DocumentService` (reads only) | Inter-plane retrieval facade | plane | gRPC | none |

### 2.3 wiki-store-go — HTTP :8011; gRPC internal-only (port unpublished)

Verified `aud=data-plane` bearer on HTTP and gRPC; wiki scopes and tenant are
derived from verified claims. `X-Org-ID` is only an equality assertion.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/readyz`, `/metrics` | Probes/metrics | none | HTTP | none |
| POST | `/v1/wiki/pages` | Create wiki page | `wiki.write` bearer | HTTP | **later** |
| GET | `/v1/wiki/pages` | Paginated page list (built for the velion sidebar) | `wiki.read` bearer | HTTP | core |
| GET | `/v1/wiki/pages/by-path` | Page by path | `wiki.read` bearer | HTTP | core |
| GET | `/v1/wiki/pages/{pageID}` | Page by ID | `wiki.read` bearer | HTTP | core |
| POST | `/v1/wiki/pages/{pageID}/versions` | Publish new version | `wiki.write` bearer | HTTP | **later** |
| GET | `/v1/wiki/pages/{pageID}/versions` | Version history | `wiki.read` bearer | HTTP | core |
| GET | `/v1/wiki/pages/{pageID}/diff` | Diff two versions | `wiki.read` bearer | HTTP | core |
| GET | `/v1/wiki/pages/{pageID}/backlinks` | Backlinks | `wiki.read` bearer | HTTP | core |
| POST | `/v1/wiki/pages/{pageID}/proposals` | Submit edit proposal | `wiki.write` bearer | HTTP | **later** |
| POST | `/v1/wiki/proposals/review` | Approve/reject proposal | `wiki.approve` bearer | HTTP | **later** |
| POST/GET | `/v1/wiki/pages/{pageID}/source-logs` | Provenance write (internal) / read | scoped bearer | HTTP | none / **later** |
| POST/GET | `/v1/wiki/pages/{pageID}/maintenance-logs` | Maintenance log write (internal) / read | scoped bearer | HTTP | none / **later** |
| POST | `/v1/wiki/maintenance/sweep` | Batch lint ingest (operator/automation) | `wiki.maintenance.write` bearer | HTTP | none |
| gRPC | `wiki.v1.WikiService` (full CRUD mirror) | Inter-plane consumers | plane | gRPC | none |
| NATS | pub `dataplane.wiki.version.published` | Drives wiki embedding (silent if NATS_URL unset!) | — | NATS | none |

### 2.4 graph-index-rs — HTTP :9203; gRPC internal-only

Verified RS256/JWKS bearer on HTTP and gRPC. User reads are tenant-bound; service
principals additionally require `graph:read`. Every path/query/body tenant is
checked against the signed claim.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/readyz` | Probes | none | HTTP | none |
| GET | `/v1/graphs/{org_id}` | Aggregate graph snapshot per org (Knowledge Graph + onboarding preview) | Verified bearer | HTTP | core |
| GET | `/v1/graph/entities`, `/v1/graph/entities/{entity_id}` | Entities | Verified bearer | HTTP | core |
| GET | `/v1/graph/relationships/{entity_id}` | Relationships | Verified bearer | HTTP | core |
| GET | `/v1/graph/claims`, `/v1/graph/contradictions` | Claims / contradictions | Verified bearer | HTTP | core |
| POST | `/v1/graph/expand` | N-hop expansion from seeds | Verified bearer | HTTP | core |
| POST | `/v1/graph/exports` | Export graph (json/graphml/markdown) | Verified bearer | HTTP | core |
| gRPC | `graph.v1.GraphService` | Consumed by retrieval-engine | plane | gRPC | none |
| NATS | JetStream consumer (graph extraction, orphan cleanup) | — | — | NATS | none |

### 2.5 Headless / operator services (no SPA relevance)

| Service | Surface | v3 |
|---|---|---|
| index-engine-rs (:9201) | health/readyz only; NATS chunking pipeline | none |
| embedding-engine-rs (:9202) | health/readyz only; NATS embedding + Qdrant writes | none |
| quickwit-adapter-rs (:9204) | health + scoped, audited `POST /admin/rebuild`; destructive clear remains disabled | none |
| data-orchestrator-go (:8012) | JWT/scoped `/v1/orchestrator/{jobs,reindex,stale-embeddings}` — operator-facing; production mutations contained | none |
| data-quality-go (:8013) | JWT + `data:quality:admin` evals/quality/cost APIs; trust/lint/cost are **later** UI candidates | later/none |
| retrieval-eval-py | empty directory, no endpoints — do not build against | none |

---

## 3. Ingestion Plane

### 3.1 quarry-edge (Rust/axum) — HTTP :8082, SSE, GraphQL; 1MB body cap

The only Ingestion service designed for end-user JWTs: verifies `Authorization: Bearer` against auth-core JWKS itself (verified org_id overrides client input). Gateway can pass a minted plane token through.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/ready` | Probes | none | HTTP | none |
| GET | `/graphql/schema`, `/graphql/playground` | Schema introspection / dev playground | none | HTTP | none |
| POST | `/v1/scrape` | Single-page scrape (cache policy, ZDR flag, render hints) | Bearer JWT (JWKS) | HTTP | core |
| POST | `/v1/scrape/stream` | Streaming scrape | Bearer JWT | SSE | core |
| POST | `/v1/crawl` | Durable multi-page crawl job (→ quarry-control) — backs `knowledge.recrawl_source` | Bearer JWT | HTTP | core |
| POST | `/v1/batch` | Batch scrape job | Bearer JWT | HTTP | **later** |
| POST | `/v1/internal/run_page` | Orchestrator-internal page execution | internal | HTTP | none |
| GET/POST/DELETE | `/v1/profiles[...:id]`, `POST /v1/profiles/:id/restore_probe` | Browser profile CRUD/probe | Bearer JWT | HTTP | **later** |
| POST | `/v1/audio` | Audio evidence ingestion; not the primary Velion dictation path | Bearer JWT | HTTP | **later** |
| POST | `/v1/search`, `/v1/search/images` | Web/image search | Bearer JWT | HTTP | **later** |
| POST | `/v1/map`, `/v1/extract` | Site URL discovery / structured extraction | Bearer JWT | HTTP | **later** |
| POST | `/v1/answer`, `/v1/answer/stream` | Answer with web evidence (downstream model-gateway stream noted stubbed) | Bearer JWT | HTTP / SSE | **later** |
| GET | `/v1/artifacts` | List artifacts | Bearer JWT | HTTP | **later** |
| GET | `/v1/sources`, `/v1/snapshots` | Sources/snapshots (forwards to quarry-control) | Bearer JWT | HTTP | **later** |
| GET | `/v1/:kind/jobs` | Jobs by kind (crawl/batch status) | Bearer JWT | HTTP | core |
| GET | `/v1/runs/:id/events` | Durable run event history (progress display) | Bearer JWT | HTTP | core |
| GET | `/v1/request-queues`, `/v1/benchmarks` | Queues / benchmarks (stub-level data) | Bearer JWT | HTTP | later / none |
| GET | `/v1/team/{credit-usage,token-usage,concurrency,queue-status,activity}` | Team usage/observability | Bearer JWT | HTTP | **later** |
| POST/GET | `/v1/change/{check,latest,history}` | Change tracking | Bearer JWT | HTTP | **later** |
| POST | `/graphql` | Auth-gated GraphQL | Bearer JWT | HTTP | **later** |
| GET/POST/DELETE | `/v1/schedules[...]` (+ pause/unpause/trigger/backfill) | Schedules (trigger/backfill pending Temporal wiring) | Bearer JWT | HTTP | **later** |
| POST/DELETE | `/v1/agent/runs[...]` | Agentic browser runs (Model-Plane-proposed; feature-gated) | Bearer JWT | HTTP | none |

### 3.2 quarry-control (Go/chi) — HTTP :8081 — **inter-plane only**

HMAC-signed cross-plane requests (`X-Quarry-Sig*`). **Never expose via gateway**; frontend-relevant reads are proxied through quarry-edge. (The gateway's existing onboarding crawl-preview/website-ingest handlers call it server-side — acceptable because the HMAC secret stays in the gateway.) All rows: v3 = none. Families: `/v1/jobs` CRUD+events+history, `/v1/stores`, `/v1/snapshots`, `/v1/artifacts`, `/v1/profiles`, `/v1/schedules` (+lifecycle, trigger/backfill stubs), `/v1/webhooks` (+delivery retry), `/v1/blocklists`, `/v1/presets`, `/v1/restore`, `/v1/runs/{id}/events` (GET paginated, POST bearer-key), `/v1/sources|benchmarks|request-queues`, `/v1/team/*`, `/v1/{kind}/jobs`.

### 3.3 imports-core (Python FastAPI) — HTTP :3025

Shared internal key (`X-Internal-Api-Key` / `x-service-auth`), `X-Org-Id` REQUIRED — gateway is the trust boundary.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/` | Probes/info | none | HTTP | none |
| POST | `/api/v1/import/jobs/upload` | File-upload import job (knowledge file intake) | internal key + X-Org-Id | HTTP | core |
| POST | `/api/v1/import/jobs/source` | Source-import job | internal key + X-Org-Id | HTTP | core |
| GET | `/api/v1/import/jobs/{job_id}` | Job detail/status | internal key + X-Org-Id | HTTP | core |
| GET | `/api/v1/import/jobs/{job_id}/events` | Job progress events | internal key + X-Org-Id | HTTP | core |
| NATS | import events pub; Control Plane subject subs | Cross-plane signaling | NATS creds | NATS | none |

### 3.4 integration-corev2 (Go/Fiber) — HTTP :3026; envelope `{success, data|error}` (gateway must normalize)

Tiers: internal key OR Control Plane bearer JWT on most `/api/v1`; pro-plan gate on connect-session routes; public `GET /api/v1/providers` and `GET /oauth/callback/:provider`.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/ready`, `/health/detailed`, `/metrics` | Probes/metrics | none / internal | HTTP | none |
| GET | `/api/v1/providers` | Provider catalog with readiness flags | none (public) | HTTP | core |
| POST | `/api/v1/providers/:provider/connect-session` (alias `/connect`) | Start OAuth connect session | internal-or-bearer + pro plan | HTTP | core |
| POST | `/api/v1/providers/:provider/reconnect-session` | Re-authorize connection | internal-or-bearer + pro plan | HTTP | core |
| GET | `/oauth/callback/:provider` | OAuth redirect callback — browser hits this directly; must be routable | none (public) | HTTP | core |
| GET | `/api/v1/connect-sessions/:id/status` | Poll connect-session status | internal-or-bearer | HTTP | core |
| GET | `/api/v1/connections` | List org's connections | internal-or-bearer | HTTP | core |
| GET | `/api/v1/connections/:id` / `/status` / `/capabilities` | Connection detail / health / capability toggles | internal-or-bearer | HTTP | core |
| PATCH | `/api/v1/connections/:id/capabilities` | Update capability toggles | internal-or-bearer | HTTP | core |
| GET/POST | `/api/v1/connections/:id/consents` | Consent list/record | internal-or-bearer | HTTP | **later** |
| GET | `/api/v1/connections/:id/discovery` | Discover provider resources (SharePoint sites/drives); not_implemented for some providers | internal-or-bearer | HTTP | core |
| POST | `/api/v1/connections/:id/actions`, `/api/v1/actions/execute` | Whitelisted provider actions | internal-or-bearer | HTTP | **later** |
| DELETE | `/api/v1/connections/:id` | Disconnect | internal-or-bearer | HTTP | core |
| POST | `/api/v1/connections/:id/sync` | Trigger sync | internal-or-bearer | HTTP | core |
| GET/POST | `/api/v1/sync-jobs` | List / create sync jobs | internal-or-bearer | HTTP | core |
| GET | `/api/v1/sync-jobs/:id`, `/api/v1/sync-jobs/:id/events` | Job detail / progress feed | internal-or-bearer | HTTP | core |
| POST | `/api/v1/sync-jobs/:id/cancel`, `/retry` | Cancel / retry | internal-or-bearer | HTTP | core |
| POST/PATCH | `/internal/sync-jobs/claim`, `/internal/sync-jobs/:id/progress` | Worker claim/progress (finspo-worker) | internal | HTTP | none |
| POST | `/api/v1/webhooks/:provider` | Provider webhook ingress | provider verification | HTTP | none |
| GET/POST/DELETE | `/api/v1/scim/tokens[...:id]` | SCIM token management | internal-or-bearer | HTTP | **later** |
| GET | `/api/v1/projections/integration-profile` | Aggregated integration profile (dashboard/settings overview) | internal-or-bearer | HTTP | core |
| POST | `/internal/connectors/token`, `/internal/gdpr/*` | Internal token mint, GDPR ops | internal | HTTP | none |
| GET/POST | `/integrations/{slack,github,notion,shopify,stripe}/...` legacy + proxy routes (proxy = 410 Gone) | Legacy compat | internal | HTTP | none |

### 3.5 finspo-core (Go) — HTTP :3130; envelope `{success, data, error}`

`X-API-Key` (or `x-internal-api-key`) + REQUIRED `X-Org-ID`; optional `X-User-ID` as actor. Gateway injects key + org.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/ready` | Probes | none | HTTP | none |
| GET | `/api/v1/sharepoint/sites` | SharePoint sites visible to org's Graph connection | API key + X-Org-ID | HTTP | core |
| GET | `/api/v1/sharepoint/sites/:siteID/items?path=/` | Ad-hoc drive browse | API key + X-Org-ID | HTTP | core |
| POST | `/api/v1/sources` | Register/upsert drive to sync | API key + X-Org-ID | HTTP | core |
| GET | `/api/v1/sources`, `/api/v1/sources/:id`, `/api/v1/sources/:id/status` | Source list / detail / delta-cursor status | API key + X-Org-ID | HTTP | core |
| POST | `/api/v1/sources/:id/sync` | Trigger delta sync (synchronous) | API key + X-Org-ID | HTTP | core |
| GET | `/api/v1/analytics/{largest,inactive,by-site,duplicates}` | Storage analytics | API key + X-Org-ID | HTTP | **later** |
| GET | `/api/v1/recommendations` | Cleanup proposal drafts | API key + X-Org-ID | HTTP | **later** |
| POST/GET | `/api/v1/proposals[...:id]` (+ approve/reject/execute) | Delete/archive proposals (execute 503 unless `FINSPO_ALLOW_EXECUTION=true`) | API key + X-Org-ID | HTTP | **later** |

### 3.6 Other Ingestion services

| Service | Surface | v3 |
|---|---|---|
| quarry-orchestrator | Temporal worker, no callable API | none |
| autocomplete-core (:3219 default; **absent from compose — deployment unverified**) | `GET /v1/suggestions?q=&scope=&limit=&org_id=` is **core** (search typeahead) once deployed behind gateway with injected X-Org-ID; `POST /v1/internal/push` later; auth only if `AUTOCOMPLETE_INTERNAL_TOKEN` set | core (deployment-gated) |
| support-worker | NATS→Temporal bridge (`velion.support.>`), no API | none |
| integration-webhook-normalizer (:3036), integration-engine-go-api (:3126), connector-runtime-engine (:3003/:3009) | **Not catalogued** — needs a separate research pass if touched | none (uncatalogued) |

---

## 4. Application Plane

### 4.1 velion-gateway-rs — HTTP :3185 (THE v3 backend)

Today: 20 routes (health + onboarding domain). No inbound token validation — actor from trusted proxy headers (`x-session-user-id/email/name`), dev headers behind `ALLOW_DEV_ACTOR_HEADERS`, silent fallback actor `velion-v3-local-user`. Outbound: stamps `x-internal-api-key` + identity headers on every upstream (12s reqwest timeout). CORS allowlist defaults to Vite dev origins. Several handlers mask upstream failures as 200 + `success:false`.

| Method | Path | Upstream | Transport | v3 |
|---|---|---|---|---|
| GET | `/health` | — (local) | HTTP | later |
| GET | `/api/v1/session/bootstrap` | — (header echo + flags) | HTTP | onboarding |
| GET | `/api/v1/onboarding/status` | — (actor echo) | HTTP | onboarding |
| GET | `/api/v1/onboarding/brreg/search` | org-core `GET /api/v1/brreg/search` | HTTP | onboarding |
| GET | `/api/v1/onboarding/graph-preview` | graph-index `GET /v1/graphs/{orgId}` (empty graph on failure) | HTTP | onboarding |
| POST | `/api/v1/onboarding/crawl-preview` | quarry-control `POST /v1/jobs/` + quarry-edge `POST /v1/scrape/stream` + poll `GET /v1/jobs/{id}/events` (~18s budget) | SSE (POST!) | onboarding |
| POST | `/api/v1/onboarding/recommend-plan` | model-gateway `POST /v1/recommend/plan` (deterministic local fallback) | HTTP | onboarding |
| GET/PUT | `/api/v1/onboarding/state` | user-core `GET/PUT /api/v1/users/me/onboarding-state` (errors masked as 200) | HTTP | onboarding |
| PUT | `/api/v1/onboarding/theme` | user-core `GET`+`PUT /api/v1/settings/appearance` | HTTP | onboarding |
| POST | `/api/v1/onboarding/complete` | user-core `POST /api/v1/users/onboarding/complete` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/create-organization` | org-core `POST /orgs` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/set-plan` | org-core `POST /orgs/{orgId}/plan` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/start-checkout` | org-core `POST /orgs/{orgId}/checkout-session` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/start-website-ingest` | quarry-control `POST /v1/jobs/` (SSRF-guarded, max_pages 1-20) | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/start-connect-session` | integration-api `POST /api/v1/providers/{provider}/connect-session` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/discover-source` | integration-api `POST /api/v1/onboarding/discover-source` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/cleanup-source` | integration-api `POST /api/v1/onboarding/cleanup-source` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/warm-sharepoint-discovery` | integration-api `POST /api/v1/providers/microsoft/sharepoint/discovery/warm` | HTTP | onboarding |
| POST | `/api/v1/onboarding/actions/start-integration-sync` | integration-api `POST /api/v1/providers/{provider}/sync` | HTTP | onboarding |
| POST | `/api/v1/security/url-reputation-checks` | gateway security adapter: local policy/cache + Google Web Risk lookup; vendor key stays server-side | HTTP | core |
| POST | `/api/v1/security/url-investigations` | gateway security adapter: approved urlscan.io submission/search with explicit visibility and audit controls | HTTP | core |

### 4.2 conversation-core-go — HTTP :3160

Shared internal API key (per-route granularity undocumented; endpoints from research docs, not re-verified in this pass).

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/health`, `/ready` | Probes | none | HTTP | none |
| GET | `/api/v1/inboxes` | List support inboxes | internal key | HTTP | core |
| GET | `/api/v1/inboxes/:id/queue` | Inbox conversation queue | internal key | HTTP | core |
| GET | `/api/v1/conversations`, `/api/v1/conversations/:id` | List / get conversations | internal key | HTTP | core |
| POST | `/api/v1/conversations/search` | Search conversations | internal key | HTTP | core |
| POST | `/api/v1/conversations/:id/messages` | Append message | internal key | HTTP | core |
| POST | `/api/v1/conversations/:id/notes` | Internal note | internal key | HTTP | core |
| PATCH | `/api/v1/conversations/:id/status` / `/assignment` | Status / assignment | internal key | HTTP | core |
| POST/DELETE | `/api/v1/conversations/:id/tags[/:tag]` | Tag add/remove | internal key | HTTP | core |
| POST | `/api/v1/ai-actions/:id/{review,approve,reject}` | Human review of proposed AI actions | internal key | HTTP | core |
| POST/GET | `/internal/conversation-events`, `/internal/conversations/:id/projection` | Internal event intake / projection | internal | HTTP | none |

### 4.3 conversation-ingest-rs — HTTP :3161

`/internal/ingest/email`, `/internal/ingest/normalized-email` → forwards into conversation-core. All v3 = none.

### 4.4 convex-core — backend :3210 (Convex protocol/WS), HTTP actions :3211, dashboard :6791

| Surface | Purpose | v3 |
|---|---|---|
| WS `convex-backend:3210` | Reactive projections (orgs, users, control sessions, agent runs, conversations, planner docs, knowledge Q&A) | core **if** v3 adopts Convex realtime — requires a WS-proxy/exposure decision; defer until then |
| removed legacy job webhooks | Dead `api.jobs.*` callers were removed in 2026-07-13 source; no no-op jobs module was added | none |
| `:3211/webhooks/ai/stream`, `/webhooks/health`, `/ingest/*`, `/api/webhook/nats/*`, `/api/operator/reconcile-memberships` | Inter-plane mirrors plus a dedicated HMAC/timestamp/nonce-gated, removal-only operator reconciliation endpoint; changed bundle not deployed | none |

### 4.5 information-core — HTTP :3190 (Gin, in-memory cache)

Internal-key gated.

| Method | Path | Purpose | Transport | v3 |
|---|---|---|---|---|
| GET | `/health`, `/ready` | Probes | HTTP | none |
| GET | `/api/v1/weather` (+ `/api/v1/weather/oslo`) | Weather for dashboard cards | HTTP | core |
| GET | `/api/v1/traffic` | Traffic metadata plus nullable provenance-bearing observations; UI must label measured/estimated/synthetic/stale/unavailable | HTTP | core |
| GET | `/api/v1/news` | News feed for dashboard cards | HTTP | core |

### 4.6 notification-core — HTTP :3140

Internal API key required (even healthcheck); user identity via forwarded headers.

| Method | Path | Purpose | Transport | v3 |
|---|---|---|---|---|
| GET | `/health` | Probe (internal-key-gated) | HTTP | none |
| POST | `/api/v1/notification-requests` | Producer-side intake (services, not SPA) | HTTP | none |
| GET | `/notifications` | User's notification feed | HTTP | core |
| GET | `/notifications/unread/count`, `/notifications/unseen/count` | Badge counts | HTTP | core |
| POST | `/notifications/:id/read`, `/notifications/:id/seen` | Mark one read/seen | HTTP | core |
| POST | `/notifications/mark-all-read`, `/notifications/mark-all-seen` | Mark all | HTTP | core |
| DELETE | `/notifications/:id` | Delete from feed | HTTP | core |
| GET | `/preferences` | Notification preferences | HTTP | core |
| PUT | `/preferences/:eventType/:channel` | Update one preference | HTTP | core |
| GET/PATCH | `/channels/config[...]` | Channel config (admin-ish) | HTTP | **later** |
| POST | `/internal/recipients/upsert` | Identity sync (internal) | HTTP | none |

2026-07-13 release note: the navbar support caller uses canonical notification intake and a non-PII deterministic idempotency key in source, but notification organization scope and typed destination resolution are unresolved. Do not treat that support action as production-ready or enable it for arbitrary recipients.

### 4.7 zammad-foundation

Separate support stack (own compose); no first-party API in-repo. v3 = none. (v2's inbox used Zammad-backed plumbing — v3 inbox targets conversation-core instead; Zammad stays behind support-worker/conversation-ingest.)

---

## 5. Model Plane

### 5.1 model-gateway — HTTP :8080 (sole public boundary), gRPC :9090

JWT Bearer via JWKS from auth-core on all non-health routes; rate-limited; dev bypass `MODEL_GATEWAY_AUTH_DEV_BYPASS=1`. velion-gateway-rs mints the user's `model-plane` audience token and forwards it.

| Method | Path | Purpose | Auth | Transport | v3 |
|---|---|---|---|---|---|
| GET | `/healthz`, `/readyz`, `/metrics` | Probes/metrics | none | HTTP | none |
| POST | `/v1/invoke` | Single-shot model invoke for a thread | JWT | HTTP | core |
| POST | `/v1/invoke/stream` | **Primary v3 chat token stream** (`{content, model, session_key, thread_id, profile:"chat"}`) | JWT | SSE | core |
| GET | `/v1/invoke/resume/:request_id` | Re-attach to in-flight stream (cross-device resume) | JWT | SSE | core |
| POST | `/v1/invoke/:request_id/cancel` | Cooperative stop | JWT | HTTP | core |
| GET | `/v1/threads/:thread_id/messages` | Reload thread history | JWT | HTTP | core |
| GET | `/v1/models` | Model picker list + feature families | JWT | HTTP | core |
| POST | `/v1/chat/documents` | Upload doc from chat into Data Plane for RAG | JWT | HTTP | core |
| POST | `/v1/feedback` | Operator feedback (skill-promotion signal) | JWT | HTTP | core |
| GET | `/v1/runs/:run_id/events` | Run/orchestration event stream (live agent-run UI; backs v3 action runs) | JWT | SSE | core |
| GET | `/v1/orchestration/runs/:run_id/plans`, `/v1/orchestration/plans/:plan_id` | Plans for a run / single plan | JWT | HTTP | core |
| GET | `/v1/orchestration/threads/:thread_id/todos`, `/v1/orchestration/todos/:todo_id` | Todos | JWT | HTTP | core |
| GET | `/v1/orchestration/runs/:run_id/approvals`, `/v1/orchestration/approvals/:approval_id` | Approvals | JWT | HTTP | core |
| GET | `/v1/orchestration/threads/:thread_id/lineage` | Subagent lineage tree | JWT | HTTP | core |
| POST | `/v1/orchestration/plans/:plan_id/{approve,reject}` | Plan decision | JWT | HTTP | core |
| POST | `/v1/orchestration/todos/:todo_id/status` | Todo status | JWT | HTTP | core |
| POST | `/v1/orchestration/approvals/:approval_id/decide` | Human-in-the-loop approval gate | JWT | HTTP | core |
| POST | `/v1/orchestration/runs/:run_id/{cancel,resume}` | Run cancel / resume | JWT | HTTP | core |
| GET | `/v1/capabilities`, `/v1/capabilities/:id` | Capability list/detail (capability-core proxy) | JWT | HTTP | core |
| GET/POST | `/v1/tasks`; GET/PATCH `/v1/tasks/:id`; POST `/v1/tasks/:id/cancel` | Background agent tasks (proxy) | JWT | HTTP | core |
| GET/POST | `/v1/cron`; GET/PATCH/DELETE `/v1/cron/:id` | Cron schedules (proxy) | JWT | HTTP | core |
| GET/POST | `/v1/memory`; GET/PATCH/DELETE `/v1/memory/:id` | Agent memory (proxy) | JWT | HTTP | core |
| GET/POST | `/v1/skills`; GET/PATCH/DELETE `/v1/skills/:id` | Agent skills (proxy) | JWT | HTTP | core |
| POST | `/v1/recommend/plan` | Plan recommendation (onboarding dependency; already proxied by gateway) | JWT | HTTP | onboarding |
| POST | `/v1/ai/chat`, `/v1/ai/embeddings`; GET `/v1/ai/models` | Direct chat (use `/v1/invoke` instead) / embeddings / model list | JWT | HTTP | **later** |
| POST/GET | `/v1/ai/speech*` | Speech STT/TTS; v3 voice routes proxy here for MVP and must propagate ZDR | JWT | HTTP | core |
| POST/GET | `/v1/ai/images*`, `/v1/ai/translate*`, `/v1/ai/documents*`, `/v1/ai/language*`, `/v1/ai/video*` | Multimodal suites other than speech | JWT | HTTP | **later** |
| POST/GET | `/v1/ai/realtime*` | Realtime session — **placeholder per openapi.yaml** | JWT | HTTP | **later** |
| GET/POST | `/v1/documents`; `/v1/documents/bulk`; GET/DELETE `/v1/documents/:id`; GET `/v1/documents/:id/index-status` | Data Plane document relays | JWT | HTTP | core (bulk **later**) |
| POST | `/v1/retrieval`; GET `/v1/retrieval/traces/:trace_id`; POST `/v1/retrieval/{sources,chunks,pack}` | Data Plane retrieval relays | JWT | HTTP | core / later (traces, chunks, pack) |
| GET | `/v1/knowledge/:document_id/units`, `/v1/knowledge/:document_id/permissions` | Knowledge units / permissions | JWT | HTTP | core / **later** |
| GET/POST | `/v1/graph/*` (entities, relationships, claims, expand, contradictions) | Graph relays | JWT | HTTP | **later** |
| POST/GET | `/v1/wiki/*` (pages, by-path, versions, sources, backlinks, maintenance, proposals) | Wiki relays | JWT | HTTP | core (pages/by-path/backlinks) / **later** (rest) |
| POST | `/v1/toon/encode` | TOON context encoding utility | JWT | HTTP | none |
| POST/GET/DELETE | `/v1/finetune/jobs[...]` | Fine-tune job lifecycle (create admin-gated) | JWT | HTTP | **later** |
| gRPC | `ModelGateway.*` (Invoke, InvokeStream, Fetch, plan-mode, teams, approvals, trajectories, skills, MCP, plugins, permissions, …) | Inter-plane/CLI twins; several gRPC-only families flagged later for agent settings | plane | gRPC | none / later |

### 5.2 Internal Model Plane cores (never called by the gateway — fronted by model-gateway)

| Service | Surface | v3 |
|---|---|---|
| session-core (gRPC :9091) | Thread/run/checkpoint authority; orchestration plans/todos/approvals; `StreamRunEvents` | none |
| inference-core (gRPC :9092) | Provider routing for all modalities | none |
| execution-core (gRPC :9093) | Agent runtime loop (in-memory state) | none |
| orchestrator-core (:8084 health only) | Temporal workflow shell | none |
| capability-core (:8085 HTTP, :9097 gRPC) | Workplane APIs behind gateway `/v1/{capabilities,tasks,cron,memory,skills}` proxies | none |
| sandbox-manager / browser-broker / letta-bridge | Sandbox leases / browser grants / memory bridge (all in-memory) | none |
| bridge-core (:8091) | CLI/IDE/channel ingress, not for the SPA | none |
| cost-core (:8089) | `GET /api/v1/usage`, `POST /api/v1/budget/check` — **later** candidate for dashboard spend, but in-memory ledger + placeholder feed and NOT proxied by model-gateway (would need direct gateway wiring) | later |

Defined-but-unhosted protos (`RunService`, `EventLog`, `BrowserAgentService`) — **do not build against**.

---

## 6. v3 Consumption View (by surface)

Endpoints listed in call order where order matters. Everything routes through velion-gateway-rs.
⏳ = upstream tagged `later` — listed for planning only, **do not wire in the current program**.

### 6.1 Onboarding + Auth (`/auth`, `/login`, `/onboarding`)

Signup→workspace sequence (gateway-mediated):

1. auth-core `POST /api/v2/auth/signUp` (or `/api/auth/*` native; social via `POST /api/v2/auth/oauth/initiate`) — with `POST /api/v2/auth/password/check-strength` inline
2. auth-core `POST /api/v2/auth/sendEmailVerification` → `POST /api/v2/auth/verifyEmail`
3. auth-core `POST /api/v2/auth/getSession` (session established; gateway validates from here on)
4. user-core `GET /api/v1/me/session-context` (has org? onboarding done? → routing)
5. user-core `GET /api/v1/users/me/onboarding-state` → hydrate wizard; `PUT` on user-driven changes
6. user-core `PATCH /api/v1/users/me` (profile step)
7. org-core `GET /api/v1/brreg/search` + `GET /api/v1/brreg/:orgnr` (company verification)
8. org-core `POST /api/v1/organizations` (gateway today uses compat `POST /orgs`) → `PATCH /api/v1/organizations/:id/brreg`
9. Website step: gateway `crawl-preview` SSE (quarry-control job + quarry-edge scrape stream) → commit via `start-website-ingest`
10. Connect step: integration-api `POST /api/v1/providers/:provider/connect-session` → browser completes `GET /oauth/callback/:provider` → poll `GET /api/v1/connect-sessions/:id/status` → `discover-source` (integration-api discovery + finspo `GET /api/v1/sharepoint/sites`) → `start-integration-sync`
11. Plan step: graph-index `GET /v1/graphs/{orgId}` (preview) + model-gateway `POST /v1/recommend/plan`
12. org-core `POST /api/v1/organizations/:id/plan`; trial → billing-core `PUT /api/v1/billing/orgs/:orgId/account`; paid → billing-core/org-core checkout-session
13. user-core `PUT /api/v1/settings/appearance` (brand theme)
14. user-core `POST /api/v1/users/onboarding/complete` → session-core `POST /api/v1/sessions/refresh`

Deferred for this surface: ⏳ 2FA/OTP/passkey/api-keys (placeholder-backed in auth-core), ⏳ consent withdraw.

### 6.2 Chat (`/chat`)

1. model-gateway `GET /v1/models` (picker)
2. model-gateway `POST /v1/invoke/stream` (SSE: today `connected/message/done/error`; target taxonomy `delta`, `reasoning_delta`, `tool_call/result`, `citation`, `artifact`, `usage`, `step_update` is additive)
3. model-gateway `POST /v1/invoke/:request_id/cancel` (stop button) / `GET /v1/invoke/resume/:request_id` (resume, `Last-Event-ID` shape)
4. model-gateway `GET /v1/threads/:thread_id/messages` (history reload)
5. model-gateway `POST /v1/chat/documents` (upload-for-RAG), `POST /v1/feedback`
6. Citations: chat grounding is emitted by model-gateway itself (owner-correct). Citation *expansion* uses retrieval-engine `GET /v1/retrieval/{trace_id}`, `POST /v1/retrieve/sources`, `POST /v1/retrieve/chunks` via the gateway.

Deferred: ⏳ `/v1/ai/*` multimodal, ⏳ realtime (placeholder), ⏳ session-core legacy `/v1/sessions/*`.

### 6.3 Knowledge (`/knowledge`)

- Library: documents-api `GET /v1/documents`, `GET /v1/documents/{id}`. The
  source-list gateway route deliberately derives source cards from visible
  documents; the Data `/v1/sources` response is a different facet contract.
- Search: retrieval-engine `POST /v1/knowledge/search`; citation resolve uses
  the real `/v1/retrieve/sources` and `/v1/retrieve/chunks` routes. The gateway
  normalizes SPA `limit`/`kinds` into Data `top_k`/filters and maps
  candidates/sources back into the UI result contract.
- Wiki: wiki-store `GET /v1/wiki/pages` (sidebar), `GET /v1/wiki/pages/by-path`, `GET /v1/wiki/pages/{id}`, `/versions`, `/diff`, `/backlinks`
- Graph: graph-index `GET /v1/graphs/{org_id}` plus entities, relationships,
  claims, contradictions, expansion, and exports. The org is session-derived;
  the historical onboarding query-parameter IDOR is closed.
- Imports: imports-core `POST /api/v1/import/jobs/upload` | `POST /api/v1/import/jobs/source` → `GET /api/v1/import/jobs/{id}` → `GET /api/v1/import/jobs/{id}/events`
- Recrawl action (`knowledge.recrawl_source`): quarry-edge `POST /v1/crawl` → `GET /v1/:kind/jobs` → `GET /v1/runs/:id/events`
- SharePoint sources: finspo `GET/POST /api/v1/sources`, `GET /api/v1/sources/:id/status`, `POST /api/v1/sources/:id/sync`
- Deferred: ⏳ wiki authoring (create/version/proposals), ⏳ document delete,
  ⏳ trust/lint badges (data-quality), ⏳ autocomplete suggestions (deployment
  unverified), ⏳ freshness/timeline/contradictions retrieval arms.
- Every interactive Data request uses a session-minted Data bearer and fails
  closed with 503 when delegation is unavailable; there is no shared-key user
  fallback.

### 6.4 Dashboard (`/`, `/dashboard`)

- Live runs: model-gateway `GET /v1/runs/:run_id/events` (SSE) + orchestration reads (`runs/:id/plans`, `threads/:id/todos`, `runs/:id/approvals`)
- Source health: documents-api `GET /v1/sources` + integration-api `GET /api/v1/projections/integration-profile` + sync-job status
- Info cards: information-core `GET /api/v1/weather`, `/api/v1/traffic`, `/api/v1/news`
- Usage/cost posture: ⏳ audit-core `GET /v1/usage/summary` + `GET /v1/audit`; ⏳ cost-core `GET /api/v1/usage` (in-memory/placeholder today — numbers not durable); ⏳ data-quality `GET /v1/cost/summary`; ⏳ quarry-edge `/v1/team/*`
- Entitlement/paywall posture: org-core `GET /api/v1/organizations/:id/entitlements`, billing-core entitlement/quota checks

### 6.5 Settings / Org / Billing (`/settings`)

- User: user-core `GET/PUT /api/v1/settings/{appearance,language,privacy,notifications,security,accessibility,ai,storage}`, `GET/PATCH /api/v1/preferences`, `GET /api/v1/providers`, API keys (`/api/v1/api-keys`)
- Org: org-core org get/entitlements; members + roles via compat `/orgs/:id/members*`, `/orgs/:id/roles*`; auth-core `POST /api/v2/auth/organization/switch-active`
- Billing: billing-core account, `GET .../entitlements/:feature`, `GET .../quotas/:metric`, checkout-session; ⏳ invoices
- Integrations: integration-api providers/connections/capabilities/sync-jobs/disconnect; finspo sources
- Session snapshot: session-core `GET /api/v1/sessions/current` + `POST /api/v1/sessions/refresh` after changes
- Deferred: ⏳ 2FA/passkey/OTP (placeholders), ⏳ SCIM tokens, ⏳ admin surfaces, ⏳ account delete

### 6.6 Inbox + Notifications (`/inbox`)

- Inbox: conversation-core `GET /api/v1/inboxes` → `GET /api/v1/inboxes/:id/queue` → `GET /api/v1/conversations/:id`; mutations: messages, notes, status, assignment, tags; AI actions `POST /api/v1/ai-actions/:id/{review,approve,reject}` (backs `inbox.draft_reply` approval loop)
- Notifications (topbar): notification-core feed + unread/unseen counts + read/seen/mark-all/delete + preferences
- Deferred: ⏳ channel config; Zammad stays behind the Application Plane (not a v3 target)

### 6.7 Agents (`/agents`)

- model-gateway: `GET /v1/capabilities`, tasks CRUD+cancel, cron CRUD, memory CRUD, skills CRUD
- Runs: `GET /v1/runs/:run_id/events` (SSE), orchestration plans/todos/approvals + decide/cancel/resume, lineage
- `agents.deploy_channel` action: no runtime backend exists today (Channel Plane is docs-only) — keep mock-degraded or map to capability rollout; flag in UI
- Deferred: ⏳ MCP/plugin/permission management (gRPC-only on model-gateway), ⏳ fine-tune jobs, ⏳ trajectories, ⏳ teams
- **Open gap:** no backend owner for v3 agent-role *configuration* (the `agentRoles` mock); nearest fit is model-gateway `/v1/skills` + `/v1/capabilities` — decision needed in Phase 5.

### 6.8 Explicitly NOT consumed by v3 (do not wire)

- quarry-control direct (HMAC inter-plane; gateway-internal only for onboarding crawl handlers)
- documents-api writes (`POST /v1/documents`, `/bulk`) — Ingestion Plane ownership
- All Data Plane headless services (index/embedding/quickwit/orchestrator/quality internals)
- All Model Plane sibling cores (session/inference/execution/capability/sandbox/browser/letta/bridge/cost direct)
- conversation-ingest, convex `:3211` webhooks (stale/broken), zammad-foundation, support-worker
- auth-core internal/admin/bearer/api-key-validate routes, all NATS/gRPC surfaces
- Channel Plane (docs-only — no runtime exists)
- affine-core (compose entry whose build context is missing on disk)
