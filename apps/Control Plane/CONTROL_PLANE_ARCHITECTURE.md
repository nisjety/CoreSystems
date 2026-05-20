# Control Plane Architecture

**Last Updated:** April 8, 2026

## Pyramid Placement

The Control Plane is the **authority root** of the CoreSystem pyramid.

Every other plane may use Control Plane services for identity, user, org, billing, entitlement, quota, and session relations, but no other plane may become the durable owner of those domains.

### Authority Rules

- `auth-core` is the canonical authentication authority.
- `user-core` is the canonical user-profile authority.
- `org-core` is the canonical organization, entitlement, and quota authority.
- `billing-core` is the canonical billing authority.
- `session-core` is the canonical session authority where that domain is active.
- Control Plane does **not** own product documents, retrieval state, embeddings, crawl output, or agent memory.
- Upper planes may consume Control Plane through internal APIs and events, but must not write directly into Control Plane databases.

## 🏗️ Service Structure

The Control Plane consists of **4 active core services** and one session authority domain:

```
┌─────────────────────────────────────────────────────────────┐
│                    CONTROL PLANE                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  auth-core   │  │  user-core   │  │   org-core   │      │
│  │   :3011      │  │    :3012     │  │    :8080     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                              │
│  ┌──────────────┐  ┌──────────────────────────────────────┐ │
│  │ billing-core │  │ session-core (canonical session      │ │
│  │   :3014      │  │ authority where used)                │ │
│  └──────────────┘  └──────────────────────────────────────┘ │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 📦 Service Responsibilities

### 1. **auth-core** (Port 3011)
**Domain**: Authentication & Authorization  
**Technology**: NestJS + TypeScript  
**Database**: PostgreSQL `auth_service`

**Owns**:
- ✅ **Auth** - All authentication methods
  - Email/password login
  - OAuth providers (Google, GitHub, Microsoft)
  - SSO (SAML, OIDC)
  - Passkeys/WebAuthn
  - Two-factor authentication (2FA)
- ✅ Session management
  - JWT/Bearer tokens
  - Session persistence
  - Device tracking
- ✅ Organization creation (initial)
  - Creates org records on signup
  - Manages organization memberships
  - Handles invitations
- ✅ User profile basics
  - Email, name, phone
  - Verification status
  - Global roles (admin/user)

**Events Published**:
- `user.created`
- `user.updated`
- `organization.created`
- `organization.member.added`
- `organization.member.removed`

---

### 2. **user-core** (Port 3012)
**Domain**: User Profiles & Management  
**Technology**: Go  
**Database**: PostgreSQL `user_service`

**Owns**:
- ✅ **User** - Extended user data
  - User profiles
  - User preferences
  - User settings
  - Avatar/photo management
- ✅ User metadata
  - Onboarding status
  - Activity tracking
  - Feature usage analytics

**Events Published**:
- `user.profile.updated`
- `user.preferences.changed`

**Events Subscribed**:
- `user.created` (from auth-core) → Create user profile
- `organization.member.added` → Link user to org

---

### 3. **org-core** (Port 8080)
**Domain**: Organization Management, Entitlements, Quotas, Feature Flags  
**Technology**: Go  
**Database**: PostgreSQL `org_core`

**Owns**:
- ✅ **Org** - Organization data
  - Organization ID, name, slug
  - Plan (free, pro, enterprise)
  - Status (active, suspended, deleted)
  - Metadata and settings
- ✅ **Feature Flags** - Capabilities & entitlements
  - Feature toggles (chat, SSO, API keys, audit logs)
  - Plan-based feature access
  - Custom feature overrides
- ✅ Quotas & limits
  - API call quotas
  - User seat limits
  - Storage limits
  - Usage tracking
- ✅ Role mappings
  - Organization roles (owner, admin, member, viewer)
  - Custom roles
  - Permission sets
- ✅ Compliance settings
  - GDPR, HIPAA, SOC2 flags
  - Data residency
  - MFA requirements
  - IP allowlists

**Events Published**:
- `organization.updated`
- `organization.plan.changed`
- `organization.deleted`
- `organization.quota.exceeded`
- `organization.feature.enabled`

**Events Subscribed**:
- `organization.created` (from auth-core) → Initialize org data
- `organization.member.added` → Update quota usage

**Does NOT Own**:
- Product documents or crawl/import output
- Retrieval or grounding state
- Embeddings or vector indexes
- Durable billing ledger state

---

### 4. **billing-core** (Port 3014)
**Domain**: Billing & Usage Authority  
**Technology**: Go  
**Database**: Billing-specific relational state

**Owns**:
- ✅ Subscription lifecycle and billing status
- ✅ Plan billing state and invoice/account metadata
- ✅ Usage accounting inputs and quota enforcement signals
- ✅ Billing-domain events consumed by other planes

**Events Published**:
- `billing.account_updated`
- `billing.quota_exceeded`
- `billing.invoice_created`
- `billing.plan_changed`

**Does NOT Own**:
- Authentication
- User profile metadata
- Canonical organization metadata outside billing scope
- Product documents or knowledge state

---

### 5. **session-core**
**Domain**: Canonical Session Authority  
**Status**: Active authority boundary where session-domain durability is required

**Owns**:
- ✅ Durable session lineage and canonical session state
- ✅ Session-domain authority for systems that need explicit session persistence outside auth token issuance

**Does NOT Own**:
- User profile authority
- Billing authority
- Product data authority

---

## 🎯 Domain Mapping

| **Domain**       | **Service**  | **Database Table(s)**                              |
|------------------|--------------|---------------------------------------------------|
| **Auth**         | auth-core    | `user`, `session`, `account`, `two_factor`, `passkey` |
| **User**         | user-core    | `user_profiles`, `user_preferences`               |
| **Org**          | org-core     | `organizations`, `org_entitlements`               |
| **Billing**      | billing-core | billing-domain tables and usage ledgers           |
| **Feature Flags**| org-core     | `org_entitlements`, `org_role_mappings`, `org_quotas` |
| **Session**      | auth-core / session-core | session issuance and durable session lineage |

---

## 🔄 Data Flow Examples

### Example 1: User Signup → Org Creation

```
1. User signs up → auth-core
   ↓
2. auth-core creates user record in auth_service DB
   ↓
3. auth-core publishes: user.created
   ↓
4. user-core receives event → creates user profile
   ↓
5. User creates organization → auth-core
   ↓
6. auth-core creates org record + membership
   ↓
7. auth-core publishes: organization.created
   ↓
8. org-core receives event → initializes:
   - Default quotas (based on free plan)
   - Default billing record (trial/free)
   - Default feature flags (chat enabled, SSO disabled)
   - Default role mappings (owner, admin, member, viewer)
   - Default compliance settings
```

### Example 2: Plan Upgrade

```
1. Admin upgrades plan (free → pro) → org-core API
   ↓
2. org-core updates organizations.plan = 'pro'
   ↓
3. org-core updates quotas (10x API calls, 50 users)
   ↓
4. org-core updates feature flags (enable SSO)
   ↓
5. org-core records plan change in org_plan_history
   ↓
6. org-core publishes: organization.plan.changed
   ↓
7. billing-core receives plan change context and applies billing lifecycle updates
   ↓
8. Convex receives event → updates org projection
```

---

## Cross-Plane Contract Rules

### Allowed Consumers Of Control Plane

- Data Plane may call Control Plane for org, user, entitlement, quota, and billing checks.
- Ingestion Plane may call Control Plane for auth, org membership, entitlement, quota, and provider/session validation.
- Model Plane v2 may call Control Plane for auth, org, quota, budget, and session validation.
- Application Plane and Frontend Plane may use Control Plane for identity and authorization context.

### Forbidden Patterns

- No other plane may write directly into Control Plane databases.
- No other plane may become the source of truth for auth, user, org, billing, entitlement, quota, or session relations.
- No service may depend on private database tables from another plane as an integration surface.

### Example 3: Feature Flag Check

```
1. Frontend requests: "Can user access SSO?"
   ↓
2. Request → org-core: GET /organizations/:id/entitlements
   ↓
3. org-core queries: org_entitlements WHERE key='feature.sso'
   ↓
4. Response: { "feature.sso": { "enabled": true } }
```

---

## 🗄️ Database Schema Overview

### auth-core Database (`auth_service`)

**Core Tables**:
- `user` - User identity and credentials
- `session` - Active sessions
- `account` - OAuth connections
- `organization` - Organizations (basic)
- `member` - User-org relationships
- `invitation` - Pending invites
- `two_factor` - 2FA secrets
- `passkey` - WebAuthn credentials
- `sso_provider` - SSO configurations

### user-core Database (`user_service`)

**Core Tables**:
- `user_profiles` - Extended user data
- `user_preferences` - User settings
- `user_activity` - Activity logs

### org-core Database (`org_core`)

**Core Tables**:
- `organizations` - Org master data
- `org_quotas` - Resource limits
- `org_billing` - Subscription & payment
- `org_compliance` - Compliance settings
- `org_role_mappings` - Roles & permissions
- `org_plan_history` - Plan change audit trail
- `org_entitlements` - Feature flags

---

## 📡 Event Bus (NATS JetStream)

All services communicate via **NATS** for:
- Event-driven architecture
- Decoupled services
- Async processing
- Fan-out to multiple consumers

**NATS Streams**:
- `USER_EVENTS` - User lifecycle events
- `ORGANIZATION_EVENTS` - Org lifecycle events
- `AUTH_EVENTS` - Legacy auth events (being phased out)

---

## 🚀 Why This Structure?

### Separated Services
- **auth-core**: Fast auth operations, critical path
- **user-core**: User data can scale independently
- **org-core**: Complex org/billing logic isolated

### Billing in billing-core (separate authority)
✅ **Reasons**:
- Billing is now a first-class Control Plane authority domain
- Subscription lifecycle and usage accounting need an explicit owner
- Keeps org metadata/entitlements separate from billing execution concerns
- Preserves the pyramid rule that billing relations stay in Control Plane but outside Data ownership

### Feature Flags in org-core (not separate)
✅ **Reasons**:
- Features are enabled/disabled per organization
- Tied to subscription plans
- Quotas and features work together
- Simpler permission checks (one DB query)

---

## 🔮 Future Considerations

If services grow too large, consider splitting:

### billing-core evolution path
**Continue expanding when**:
- Payment gateway integrations deepen
- Invoice generation and reconciliation grow
- Tax and compliance logic expands
- Usage enforcement and billing events require richer workflows

### Potential: feature-flag service
**When**: If feature management becomes sophisticated
- A/B testing
- Gradual rollouts
- User segment targeting
- Real-time feature toggles

### Current Decision
- Billing authority belongs to `billing-core`
- Organization metadata, entitlements, quotas, and feature flags belong to `org-core`
- The split is intentional so Control Plane stays authoritative without collapsing domains back into one service
- Better performance (fewer network calls)
- Easier transactions (no distributed data)

---

## 📊 Service Comparison

| Metric              | auth-core | user-core | org-core |
|---------------------|-----------|-----------|----------|
| **Language**        | TypeScript| Go        | Go       |
| **Port**            | 3011      | 3012      | 8080     |
| **Database**        | PostgreSQL| PostgreSQL| PostgreSQL|
| **Framework**       | NestJS    | Native    | Native   |
| **Primary Domain**  | Auth      | Users     | Orgs     |
| **Secondary Domain**| Org Create| Profiles  | Billing, Features |
| **Event Producer**  | ✅ High   | ✅ Medium | ✅ High  |
| **Event Consumer**  | ❌ Low    | ✅ High   | ✅ Medium|

---

## ✅ Summary

**Control Plane = 3 Services handling 5 Domains**

1. **Auth** → auth-core
2. **User** → user-core  
3. **Org** → org-core
4. **Billing** → org-core (same service as Org)
5. **Feature Flags** → org-core (same service as Org)

This keeps related domains together while allowing independent scaling and deployment where needed.

---

## 2026-05-20 — Velion Build Runtime Audit

Source: `apps/Control Plane/docker-compose.yml` + `build-velion-services.sh` audit.

### Observed services & ports

| Service | Container name | Host port → Container | Lang | Notes |
|---|---|---|---|---|
| auth-core | `auth-core` | 3011 → 3011 | Better Auth (TS) | canonical authn |
| user-core | `user-core` | 3012 → 3012 | Go | user profile authority |
| org-core | `org-core` | 8080 → 8080 | — | org, entitlements, quotas, billing facade, feature flags |
| billing-core | `billing-core` | 3014 → 3014 | — | (also wired into org-core per summary above) |
| session-core | `session-core` | TBD | — | session authority where used |
| lago-api | `lago-api` | 3016 → 3000 | Ruby | billing engine (hidden behind billing-core) |
| lago-front | — | TBD | — | UI |
| lago-worker / lago-clock / lago-redis (6381) / lago-pdf / lago-db | — | — | — | infra for Lago |
| lago-migrate | one-shot | — | — | DB migration; gated via `condition: service_completed_successfully` |

### Bootstrap one-shots tracked by `build-velion-services.sh`
- `lago-migrate` — auto-removed post-exit-0.

### Network
- Compose uses `controlplane-net` (private) — **NOT joined to `inter-plane-bus`**, so velion server-side fetches to `auth-core:3011`, `user-core:3012`, `org-core:8080`, `billing-core:3014`, `session-core:3017` will DNS-fail from the velion container.

### Known cross-stack consumer drift (still present)
- Application Plane `convex-backend` compose declares `AUTH_SERVER_URL=http://auth-service:3011` and `ORG_CORE_URL=http://org-core-service:8080`. **Both names are stale**: the live containers are `auth-core` and `org-core`. Fix the env defaults in `apps/Application Plane/docker-compose.yml`.
- Velion `apps/Frontend Plane/velion/.env` uses the same stale names (`auth-service`, `user-service`, `org-core-service`, `billing-core-service`, `session-core-service`) — all fail when velion runs in compose because:
  1. Names don't match live containers.
  2. Even with correct names, Control Plane isn't on `inter-plane-bus`.

### Remediation
1. ~~Rename the env defaults across `apps/Application Plane/docker-compose.yml` and `apps/Frontend Plane/velion/.env` to match the live container names~~ — **Not needed.** The Control Plane compose uses `container_name:` overrides (`auth-service`, `user-service`, `org-core-service`, `billing-core-service`, `session-core-service`). These names DO match what velion's `.env` already calls. The names were inferred wrong in earlier audits because the docs use the canonical core names. Velion calls resolve correctly via `inter-plane-bus`.
2. Add Control Plane services to `inter-plane-bus` — **already done** in compose (`networks: [controlplane-net, inter-plane-bus]`).
3. R15 (2026-05-20) brought the stack up cleanly. See §Verified all-green below.

## 2026-05-20 — Verified all-green (R15)

Final run brought the Control Plane stack to **15/15 running** under the compose project name `control-plane`. Fixes landed:

- `apps/Control Plane/docker-compose.yml`
  - Remapped `org-core` host ports to avoid cross-stack collisions with Model Plane's `model-gateway` (8080) and `session-core` (9091): `8080→18080`, `9090→19090`, `9091→19091`. Container ports unchanged so the cross-plane bus name `http://org-core-service:8080` from velion still works.

### Container roll-up (final)
| Container | Status |
|---|---|
| auth-service (3011) | Up healthy |
| user-service (3012) | Up healthy |
| org-core-service (18080→8080) | Up healthy |
| billing-core-service (3014) | Up healthy |
| session-core-service (3017) | Up healthy |
| controlplane-postgres (5433→5432) | Up healthy |
| controlplane-redis | Up healthy |
| controlplane-nats (4223→4222) | Up healthy |
| lago-api (3016→3000) | Up healthy |
| lago-db | Up healthy |
| lago-redis (6381→6379) | Up healthy |
| lago-worker | Up |
| lago-clock | Up |
| lago-pdf | Up |
| lago-front | Up |
| lago-migrate (one-shot) | Exited 0, removed |

`lago-migrate` ran to exit-0 and was cleanly removed by the build script. Velion server-side calls to `auth-service:3011`, `user-service:3012`, `org-core-service:8080`, `billing-core-service:3014`, `session-core-service:3017` all resolve over `inter-plane-bus`.

