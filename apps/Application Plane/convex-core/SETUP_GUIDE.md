# Convex Core Integration Setup Guide

## Quick Start

Convex Core is the real-time UI state synchronization layer in the **APPLICATION PLANE**. It provides reactive mirrors of backend state and real-time WebSocket subscriptions for the frontend, with proper validation and event synchronization.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│         CoreSystem Architecture (Multi-Plane)                │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  CONTROL PLANE (Source of Truth)                             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │  auth-service│    │ user-service │    │  org-core    │  │
│  │ (3011, 50011)│    │ (3012, 50012)│    │ (8080, 9090) │  │
│  └──────┬───────┘    └──────┬───────┘    └──────┬───────┘  │
│         │                   │                    │            │
│         └───────────────────┼────────────────────┘            │
│                             │                                 │
│                      ┌──────▼─────┐                           │
│                      │    NATS    │                           │
│                      │ Event Stream│                           │
│                      └──────┬─────┘                           │
│                             │                                 │
│         ┌───────────────────┼──────────────────────┐         │
│         │                   │                      │         │
│  APPLICATION PLANE          │         DATA PLANE   │         │
│    ┌────▼──────────┐        │       ┌────▼───────┐│         │
│    │ Convex Core   │        │       │ Documents  ││         │
│    │ (Reactive UI) │◄───────┴───────┤ RAG        ││         │
│    │ VALIDATES:    │                │ Vectors    ││         │
│    │ - JWT         │                └────────────┘│         │
│    │ - Org Member  │                              │         │
│    │ - Capabilities│                              │         │
│    │   - Real-time API (HTTP + WebSocket)  │               │
│    │   - Chat, Conversations, Messages     │               │
│    │   Port 3000                           │               │
│    └────────────────────────────────────────┘               │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

## Prerequisites

- Node.js 20+ installed locally
- Convex CLI installed: `npm install -g convex`
- All control-plane services running (auth-core, org-core, user-core)
- NATS running with event streaming

Start the core stack first:
```bash
docker compose up -d
```

## Step 1: Navigate to Convex Gateway

```bash
cd convex-gateway
```

## Step 2: Install Dependencies

```bash
npm install
```

## Step 3: Start Convex Dev Server

There are two options:

### Option A: Using Convex Cloud (Recommended for Production)

```bash
# Login to Convex
convex auth login

# Start dev server connected to your Convex Cloud project
convex dev
```

This will:
1. Authenticate with Convex Cloud
2. Sync your schema and functions
3. Start a local dev environment
4. Print the CONVEX_URL to use in your frontend

### Option B: Self-Hosted Backend (Advanced)

If you want to run Convex completely locally without Convex Cloud, use the self-hosted backend option. See [Convex Self-Hosted Documentation](https://docs.convex.dev/self-hosted).

## Step 4: Verify NATS Integration

The Convex gateway is configured to subscribe to NATS events once the dev server starts.

Check the logs for these messages:
```
[Convex] NATS Subscriber initialized
[Convex] Subscribed to: organization.created
[Convex] Subscribed to: organization.member.added
...
```

To manually test NATS integration:

```bash
# Publish a test organization event
nats pub organization.created --json '{
  "id": "test-org-1",
  "name": "Test Organization",
  "slug": "test-org",
  "createdAt": '$(date +%s)'000
}'
```

Then check the Convex dashboard (usually at http://localhost:3000) to see the organization synced.

## Integration Testing

### Test 1: Sync Organization from Auth Service

```bash
# Create a new organization (via auth-core API)
curl -X POST http://localhost:3011/api/v2/auth/organization/create \
  -H "Content-Type: application/json" \
  -d '{
    "name": "My Organization",
    "slug": "my-org"
  }' | jq .

# ORG_ID from response, then verify in Convex dashboard:
# Dashboard → organizations table → should see the org with externalOrgId
```

### Test 2: Sync User to Organization

```bash
# Invite a user to organization (via org-core)
curl -X POST http://localhost:3011/api/v2/auth/organization/invite-member \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "ORG_ID_FROM_ABOVE",
    "email": "user@example.com",
    "role": "member"
  }' | jq .

# Verify in Convex dashboard:
# Dashboard → users table → should see user with externalAuthId
```

### Test 3: Create Conversation

Once users are synced, create a conversation:

```typescript
// From your frontend or Convex mutation
const conversationId = await client.mutation(api.conversations.create, {
  orgId: convexOrgId,
  title: "My Conversation",
  userId: convexUserId,
});
```

## Data Flow Example

When a user creates an organization:

1. **User Action**: Calls `POST /api/v2/auth/organization/create` on auth-core
2. **Auth Service**: Creates org, publishes `organization.created` event to NATS
3. **NATS Broker**: Distributes event to subscribers
4. **Convex Subscriber**: Receives event, calls `onOrganizationCreated` mutation
5. **Convex DB**: Stores org with `externalOrgId` reference
6. **Frontend**: Queries Convex for org, receives real-time updates

## Important: Data Boundaries

### Read-Only from Control Plane
Organizations and Users are **synchronized only**:
- Cannot be created/edited directly in Convex  
- Always reference external IDs (`externalOrgId`, `externalAuthId`)
- Deleted via NATS events

### Writable in Convex
Conversations and Messages are **created and managed** in Convex:
- Created by authenticated users
- Support real-time subscriptions
- Can reference external user/org IDs for audit trails

## Environment Variables

Key variables in `.env.local`:

```
# NATS Configuration
NATS_URL=nats://localhost:4222  # Or nats://coresystem-nats-local:4222 if in Docker
NATS_TOKEN=nats
NATS_SERVICE_NAME=convex-gateway

# Auth/Org Service URLs
AUTH_SERVER_URL=http://localhost:3011
ORG_CORE_URL=http://localhost:8080

# Convex Instance (for prod)
CONVEX_DEPLOYMENT=your-convex-deployment-name
```

## Troubleshooting

### "Cannot connect to NATS"
```bash
# Check NATS is running
docker compose ps | grep nats

# Test NATS connection
nats server info
```

### "Organization not syncing"
1. Check Convex logs for NATS subscription messages
2. Verify the event was published: `nats ls`
3. Manually publish a test event to confirm
4. Check Convex dashboard for tables and data

### "Users can't create conversations"
1. Verify user was synced (check `users` table in Convex dashboard)
2. Check user has correct `orgId` and `syncStatus: "synced"`
3. Verify conversation creation mutation is accessible

### "Port 3000 already in use"
```bash
# Find process using port 3000
lsof -i :3000

# Kill the process
kill -9 <PID>
```

### Convex Cloud Login Issues
```bash
# Clear Convex authentication
rm ~/.convexrc

# Re-login (requires browser)
convex auth login
```

## Production Considerations

### Security
- Don't expose Convex directly to the internet without authentication
- Use JWT verification for user context
- Implement row-level security for conversations
- Validate organization membership on all queries

### Performance
- Use React Convex hooks for real-time subscriptions
- Implement pagination for large result sets
- Index queries appropriately
- Monitor database size and clean up old conversations

### Monitoring
- Set up alerts for sync failures
- Monitor NATS event latency
- Track Convex function execution times
- Log all organization/user sync events

### Backup & Recovery
- Regularly backup Convex database
- Keep NATS event stream for audit trail
- Document data retention policies
- Test recovery procedures

## Running Both Convex and Control Plane

### Terminal 1 - Control Plane:
```bash
docker compose up -d
docker compose logs -f
```

### Terminal 2 - Convex Dev:
```bash
cd convex-gateway
convex dev
# Open the URL it provides in your browser
```

### Terminal 3 - Frontend (Optional):
```bash
# Your frontend app
npm run dev
```

## File Structure

```
convex-gateway/
├── convex/
│   ├── schema.ts                  # Database schema definition
│   ├── organizations.ts           # Org queries/mutations
│   ├── users.ts                   # User queries/mutations
│   ├── conversations.ts           # Chat conversation logic
│   ├── messages.ts                # Message storage and queries
│   ├── nats.ts                    # NATS event handlers
│   ├── http.ts                    # HTTP webhooks
│   └── ai.ts                      # AI integration
├── nats-subscriber.js             # NATS event subscriber service
├── convex.config.ts               # Convex configuration
├── package.json                   # Dependencies
├── .env.local                      # Local configuration
└── CONVEX_INTEGRATION.md          # Architecture documentation
```

## Next Steps

1. ✅ **Done**: Set up Convex with NATS integration
2. **Next**: Create a frontend app that connects to Convex
3. **Next**: Build chat interface using Convex real-time subscriptions
4. **Next**: Implement organization/user context in frontend
5. **Next**: Add AI features for conversations

## Documentation

- [CONVEX_INTEGRATION.md](./CONVEX_INTEGRATION.md) - Full architecture details
- [Convex Official Docs](https://docs.convex.dev) - API reference
- [NATS Documentation](../../docs/NATS_INTEGRATION.md) - Event streaming details

## Support

For issues:
1. Check [Convex Discord](https://discord.gg/convex)
2. Review [Convex Docs](https://docs.convex.dev)
3. Check [CoreSystem README](../README.md)
4. Open an issue in the repository

---

**Last Updated**: February 2026  
**Status**: ✅ Integration Complete - Ready for Development
