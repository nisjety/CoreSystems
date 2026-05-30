# User Profile 500 Error - Root Cause Analysis

## Problem Summary

User profileendpoint returning 500 error:
```
GET /api/user/users/me/profile?email=testbruker@aquatiq.com → 500 Internal Server Error
```

## Root Causes Identified

### 1. ✅ FIXED: Incorrect API Endpoint

**Issue**: Frontend calling `/users/me/profile` but backend endpoint is `/users/me`

**Location**: `frontend/src/lib/services/user-service.ts`

**Fix Applied**:
```typescript
// BEFORE (wrong):
this.getUserEndpoint(`/users/me/profile?email=${email}`)

// AFTER (correct):
this.getUserEndpoint(`/users/me?email=${email}`)
```

### 2. 🔄 IN PROGRESS: UUID Type Mismatch

**Issue**: Better Auth generates  base62 user IDs (e.g., `12J7tfIL8Gu6DqQzkPCsZE1NDMxTAUPs`) but user-core database expects standard UUIDs (e.g., `123e4567-e89b-12d3-a456-426614174000`)

**Evidence from Logs**:
```
ERROR: invalid input syntax for type uuid: "12J7tfIL8Gu6DqQzkPCsZE1NDMxTAUPs" (SQLSTATE 22P02)
```

**Affected Tables**:
- `users` (id column)
- `user_profiles`, `user_sessions`, `user_activities`, `user_roles`, `user_devices` (user_id foreign keys)
- `roles` (id column)

**Solution Created**:
- Migration file: `/user-core/migrations/003_change_uuid_to_text.up.sql`
- Converts all `UUID` columns to `TEXT` to accept Better Auth's base62 IDs
- Drops and re-creates foreign key constraints

## Service Architecture

```
Frontend (localhost:3000)
  ↓
/api/user/[...path] (Next.js API Route)
  ↓ Proxy to USER_SERVICE_URL
localhost:3012 (user-core HTTP)
  ↓
user_service PostgreSQL database (TEXT IDs needed)
```

**Current Service Status**:
- ✅ org-core: Running on localhost:8090
- ✅ auth-core: Running on localhost:3011  
- ⚠️ user-core: Service container running but database connection issues

## Database Setup Issues Encountered

**Problem**: user-service container cannot connect to `user_service` database

**Logs**:
```
FATAL: database "user_service" does not exist (SQLSTATE 3D000)
```

**PostgreSQL Containers**:
- `aquatiq-postgres-local`: Port 5432
- `controlplane-postgres`: Port 5433

**Database Exists On**: controlplane-postgres  
**Service Trying**: controlplane-postgres at 172.20.0.4:5432 (Docker network IP)

## Next Steps to Complete Fix

1. **Debug user-service database connection**:
   - Verify the DATABASE_URL environment variable in user-service container
   - Check if database name is correct in connection string
   - Ensure PostgreSQL is accepting connections from user-service

2. **Apply UUID → TEXT migration**:
   ```bash
   # Once service connects, run:
   docker exec controlplane-postgres psql -U aquatiq -d user_service < migrations/003_change_uuid_to_text.up.sql
   ```

3. **Restart user-service** to pick up schema changes

4. **Test user profile endpoint**:
   ```bash
   curl -H "x-internal-api-key: dev-super-secret-internal-api-key" \
        "http://localhost:3012/api/v1/users/me?email=testbruker@aquatiq.com"
   ```

## Alternative Solution (If Migration Fails)

Rebuild user-service database from scratch with TEXT IDs:
1. Drop and recreate `user_service` database  
2. Apply `001_init_text_ids.sql` (already created)
3. Apply migration `002_user_settings_providers` with TEXT modifications
4. Restart user-service

## Files Modified

✅ `/frontend/src/lib/services/user-service.ts` - Fixed endpoint paths  
✅ `/user-core/migrations/003_change_uuid_to_text.up.sql` - UUID → TEXT migration  
✅ `/user-core/migrations/003_change_uuid_to_text.down.sql` - Rollback migration
✅ `/user-core/migrations/001_init_text_ids.sql` - Fresh schema with TEXT IDs

## Related Better Auth Configuration

Better Auth uses `uid` package for ID generation (base62 format by default).  
There's no built-in option to use standard UUIDs, so database schema must adapt.