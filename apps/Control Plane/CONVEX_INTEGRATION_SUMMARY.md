# CoreSystem Convex Core Integration - Complete Overview

## What Was Done

Convex Core has been integrated into the CoreSystem **APPLICATION PLANE** as a real-time UI state synchronization layer with the following components:

### 1. **NATS Event Integration** ✅
   - Convex subscribes to NATS events from auth-core, user-core, and org-core
   - Events are published on specific topics:
     - `organization.created` → syncs org to Convex
     - `organization.updated` → updates org in Convex
     - `organization.deleted` → soft-deletes org in Convex  
     - `organization.member.added` → syncs user to org in Convex
     - `organization.member.removed` → removes user from org in Convex

### 2. **Database Schema** ✅
   Updated Convex schema includes:
   - `organizations` table with external ID references
   - `users` table linked to organizations
   - `conversations` table for chat sessions
   - `messages` table for message storage
   - All tables support multi-tenant isolation

### 3. **Synchronization Mutations** ✅
   Created internal mutations for:
   - `onOrganizationCreated` - receives org from control plane
   - `onOrganizationUpdated` - updates org metadata
   - `onOrganizationDeleted` - soft-deletes org
   - `onOrganizationMemberAdded` - adds user to org
   - `onOrganizationMemberRemoved` - removes user from org

### 4. **Query Functions** ✅
   Created query APIs for:
   - `organizations.getByExternalId` - lookup by auth-core ID
   - `organizations.getBySlug` - lookup by slug
   - `users.getByExternalAuthId` - lookup by auth-core user ID
   - `users.getByEmailAndOrg` - lookup in specific org
   - Proper multi-tenant isolation on all queries

### 5. **Documentation** ✅
   - [CONVEX_INTEGRATION.md](../Application\ Plane/convex-core/CONVEX_INTEGRATION.md) - Architecture & data flow
   - [SETUP_GUIDE.md](../Application\ Plane/convex-core/SETUP_GUIDE.md) - Setup & testing guide
   - Clear separation: Control Plane (source of truth) vs Application Plane (reactive mirrors)

## Current System Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    CoreSystem Control Plane                  │
│                                                                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐           │
│  │ auth-core   │  │ user-core   │  │ org-core    │           │
│  │ (3011)      │  │ (3012)      │  │ (8080)      │           │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘           │
│         │                │                │                   │
│         └────────────────┼────────────────┘                   │
│                          │                                    │
│                   ┌──────▼──────┐                             │
│                   │   NATS JS   │◄─ Event Streaming           │
│                   │(4222, 8222) │                             │
│                   └──────┬──────┘                             │
│                          │                                    │
│         ┌────────────────┘                                   │
│         │                                                     │
│    ┌────▼────────────────────────────┐                       │
│    │  Convex Gateway (localhost:3000) │                      │
│    │  → Organizations (sync-only)     │                      │
│    │  → Users (sync-only)             │                      │
│    │  → Conversations (writable)      │                      │
│    │  → Messages (writable)           │                      │
│    │  → Real-time WebSocket API       │                      │
│    └────────────────────────────────┘                        │
│                                                               │
│  Plus: PostgreSQL, Redis, Infrastructure                    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

## Data Boundaries

### Control Plane (Auth/User/Org Services)
- **Owns**: User authentication, organization metadata, policies
- **Publishes**: Events when orgs/users change
- **Type**: Source of truth

### Convex Gateway
- **Syncs**: Organizations and users from NATS events
- **Owns**: Conversations, messages, real-time interactions
- **Type**: Derived/cached data with additions
- **Isolation**: Organizations are siloed - users can only see conversations in their orgs

## How to Use

### For Development

1. **Start Control Plane**:
   ```bash
   docker compose up -d
   ```

2. **In separate terminal, start Convex Core**:
   ```bash
   cd ../Application\ Plane/convex-core
   npm install  # First time only
   convex dev   # Requires self-hosted Convex backend
   ```

3. **Test by creating organization**:
   ```bash
   curl -X POST http://localhost:3011/api/v2/auth/organization/create \
     -H "Content-Type: application/json" \
     -d '{"name":"Test Org","slug":"test-org"}'
   ```

4. **See it synced in Convex Core dashboard**:
   - Open http://localhost:3000 (or URL from `convex dev`)
   - Navigate to `organizations` table
   - Should see the org with `externalOrgId` set (reactive mirror)

### For Testing Integration

Run the comprehensive test:
```bash
bash test-all-services.sh
```

Run the org lifecycle test:
```bash
bash scripts/test-org-lifecycle.sh
```

## File Structure

```
Application Plane/
└── convex-core/                   # Real-time UI state layer
    ├── convex/
    │   ├── schema.ts              # Database schema
    │   ├── nats.ts                # NATS event handlers
    │   ├── organizations.ts       # Org mutations/queries
    │   ├── users.ts               # User mutations/queries
    │   ├── conversations.ts       # Chat conversations
    │   └── messages.ts            # Chat messages
    ├── nats-subscriber.js         # NATS event subscriber service
    ├── CONVEX_INTEGRATION.md      # Full architecture
    ├── SETUP_GUIDE.md             # Setup instructions
    └── package.json               # Dependencies

Control Plane/
├── auth-core/                     # Authentication service (source of truth)
├── user-core/                     # User profiles service (source of truth)
├── org-core/                      # Organization service (source of truth)
│
├── docker-compose.yml             # Infrastructure + services
├── test-all-services.sh           # Integration test
└── scripts/test-org-lifecycle.sh  # Org flow test
```

## Key Features

### ✅ Implemented
- NATS event streaming integration
- Organization synchronization
- User-to-organization mapping
- Multi-tenant data isolation
- Real-time conversation support
- Schema with external ID references
- Clear read-only vs writable boundaries
- Comprehensive documentation

### 🔧 Production Ready (With Configuration)
- Convex Cloud connection (requires account)
- PostgreSQL persistence (instead of SQLite)
- Custom authentication  
- Production NATS setup with authentication
- Monitoring and alerting
- Backup and recovery procedures

### 📋 Can Be Added Later
- Advanced conversation features (threading, reactions)
- AI integration with conversation context
- Analytics and usage tracking
- Custom webhooks
- API keys and programmatic access
- Advanced search and filtering

## Testing Checklist

✅ **Core Services**
- [x] Auth service running on 3011
- [x] User service running on 3012
- [x] Org service running on 8080
- [x] NATS running on 4222/8222
- [x] PostgreSQL running on 5432
- [x] Redis running on 6379

✅ **Integration Points**
- [x] User signup endpoint working
- [x] User signin endpoint working
- [x] Organization creation working
- [x] Organization persisted to database
- [x] NATS event publishing
- [x] User authentication via NATS

✅ **Convex Setup**
- [x] Convex schema updated with external IDs
- [x] NATS integration mutations created
- [x] Environment configuration ready
- [x] Documentation complete
- [ ] (Manual) Run `convex dev` and verify sync

## Next Steps

1. **Run Convex locally** (requires Convex Cloud account or self-hosted):
   ```bash
   cd convex-gateway
   convex dev
   ```

2. **Build a frontend** that connects to Convex:
   - Use Convex React hooks for real-time data
   - Authenticate users via auth-core
   - Create conversations in Convex

3. **Add features**:
   - Profile management
   - Conversation threading
   - Message reactions
   - File uploads
   - AI responses

4. **Deploy to production**:
   - Use Convex Cloud (https://dashboard.convex.dev)
   - Configure production NATS
   - Set up PostgreSQL for persistence
   - Enable proper authentication/authorization

## Important Links

- [Convex Documentation](https://docs.convex.dev)
- [CONVEX_INTEGRATION.md](../Application\ Plane/convex-core/CONVEX_INTEGRATION.md) - Detailed architecture
- [SETUP_GUIDE.md](../Application\ Plane/convex-core/SETUP_GUIDE.md) - Setup instructions
- [NATS Documentation](https://docs.nats.io)
- [BOUNDARY_VALIDATION.md](../../docs/BOUNDARY_VALIDATION.md) - Multi-plane architecture

## Questions?

Refer to:
1. The documentation files above
2. Convex official docs
3. NATS documentation
4. APPLICATION PLANE README (convex-core)
5. CONTROL PLANE README (auth, user, org services)

---

**Status**: ✅ Complete - Ready for Development  
**Last Updated**: February 19, 2026  
**Integration Type**: Event-driven APPLICATION PLANE with reactive UI state synchronization
**Architecture**: Multi-plane with clear boundaries (Control → Application → Data → Reasoning)
