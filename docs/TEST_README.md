# Organization Testing Suite

Test scripts for verifying the organization lifecycle and event flow in the CoreSystem.

##📁 Test Scripts

### 1. `test-org-lifecycle.sh` - Full Lifecycle Test
Comprehensive test covering the complete organization lifecycle.

**What it tests:**
- ✅ User registration (owner)
- ✅ Organization creation
- ✅ Database persistence (auth_service + org-core)
- ✅ NATS event flow
- ✅ Member registration
- ✅ Member invitation
- ✅ Organization member listing

**Usage:**
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/backend
./test-org-lifecycle.sh
```

**Expected Output:**
- Organization created in auth_service database
- Organization synced to org-core database with capabilities, quotas, and usage tracking
- NATS events published and consumed
- Invitation sent (if endpoint available)

---

### 2. `test-org-invite.sh` - Invitation Test
Quick test for inviting members to existing organizations.

**Usage:**
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/backend

# List available organizations first
docker exec coresystem-postgres-local psql -U coresystem -d auth_service -t -c \
  "SELECT id, name FROM organization ORDER BY created_at DESC LIMIT 5;"

# Then run the invitation test
./test-org-invite.sh <organization-id> [member-email]
```

**Examples:**
```bash
# Invite test member
./test-org-invite.sh vjLfEn7443Mp6QKonxqZ4OhRDDuCC882

# Invite specific email
./test-org-invite.sh vjLfEn7443Mp6QKonxqZ4OhRDDuCC882 john@example.com
```

---

### 3. `test-org-events.go` - Event Publisher
Go script for manually publishing organization events to NATS for testing.

**Usage:**
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/backend/Org-core

# Test organization creation event
go run test/test-org-events.go org

# Test member added event
go run test/test-org-events.go member

# Test member removed event
go run test/test-org-events.go remove

# Test all events in sequence
go run test/test-org-events.go all
```

**Event Types:**
- `org` - Organization created
- `member` - Member added
- `remove` - Member removed
- `all` - All events (sequential with 2s delays)

---

## 🔍 Verification Commands

### Check Organizations in Databases

**Auth Service (Better Auth):**
```bash
docker exec coresystem-postgres-local psql -U coresystem -d auth_service -c \
  "SELECT id, name, slug, created_at FROM organization ORDER BY created_at DESC LIMIT 5;"
```

**Org-Core (Synchronized):**
```bash
docker exec coresystem-postgres-local psql -U coresystem -d org_core -c \
  "SELECT org_id, org_name, status, plan_level, created_at FROM organizations ORDER BY created_at DESC LIMIT 5;"
```

### Check Event Processing

**Org-Core Logs:**
```bash
docker logs org-core-service --tail 50 | grep -E 'organization|member'
```

**NATS Events:**
```bash
docker logs org-core-service 2>&1 | grep "Received auth event"
```

### Check Related Data

**Capabilities:**
```bash
docker exec coresystem-postgres-local psql -U coresystem -d org_core -c \
  "SELECT org_id, capability_type, enabled FROM org_capabilities ORDER BY created_at DESC LIMIT 10;"
```

**Quotas:**
```bash
docker exec coresystem-postgres-local psql -U coresystem -d org_core -c \
  "SELECT org_id, max_products, max_rag_docs, max_api_calls_per_day FROM org_quotas ORDER BY created_at DESC LIMIT 5;"
```

**Usage Tracking:**
```bash
docker exec coresystem-postgres-local psql -U coresystem -d org_core -c \
  "SELECT org_id, date, api_calls, rag_docs_count FROM org_usage ORDER BY created_at DESC LIMIT 5;"
```

---

## 🎯 Phase 3 Status

### ✅ Completed
- [x] Organization plugin enabled and configured
- [x] Better Auth API integration working
- [x] NATS event publishing (auth.organization.created)
- [x] NATS event consumption (org-core subscriber)
- [x] Database persistence (all tables: organizations, org_capabilities, org_quotas, org_usage)
- [x] UUID handling (generates new UUID for org-core, stores Better Auth ID in metadata)
- [x] Complete test suite created

### 📋 Ready for Testing
- [ ] Member invitation acceptance flow
- [ ] Member_added event handling
- [ ] Member_removed event handling

---

## 🐛 Troubleshooting

### Common Issues

**401 Unauthorized:**
- Cookie might have expired
- Sign in again to get fresh session

**Organization not visible in org-core:**
- Check org-core logs for errors
- Verify NATS connection
- Check database column mappings

**Invitation fails:**
- Verify user exists before inviting
- Check organization ID is correct
- Ensure you have owner/admin role

### Debug Commands

```bash
# Check service health
docker-compose ps

# Tail logs
docker logs org-core-service -f
docker logs auth-service -f

# Check NATS connection
docker logs org-core-service 2>&1 | grep "NATS"

# Restart services if needed
cd /Volumes/Lagring/Triodelab/CoreSystem/backend
docker-compose restart org-core auth-service
```

---

## 📊 Expected Test Results

After running `test-org-lifecycle.sh`, you should see:

1. **New User Created** in auth_service database
2. **New Organization Created** with:
   - Entry in `auth_service.organization`
   - Entry in `org_core.organizations`  
   - 2 capabilities (RAG, vector_search)
   - Quota limits set
   - Usage tracking initialized
3. **Event Logged** in org-core showing organization.created
4. **Member Invited** (if invitation flow works)

---

## 🚀 Quick Start

```bash
# Run full test
cd /Volumes/Lagring/Triodelab/CoreSystem/backend
./test-org-lifecycle.sh

# Or just test events
cd Org-core
go run test/test-org-events.go all
```

---

For more information, see:
- [Organization Plugin Documentation](../docs/ORGANIZATION_PLUGIN.md)
- [Phase 3 Implementation Notes](../auth/docs/phase3-notes.md)
