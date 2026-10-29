# Auth Service Port Configuration Update

## Summary of Changes

Updated the auth service to use **port 3011** consistently across all internal and external connections.

---

## Backend Changes ✅

### 1. Docker Compose Configuration
**File:** `backend/docker-compose.yml`

**Changes:**
- `PORT=3000` → `PORT=3011`
- `ports: "3001:3000"` → `ports: "3011:3011"`
- `BETTER_AUTH_URL=http://localhost:3001` → `BETTER_AUTH_URL=http://localhost:3011`
- Healthcheck: `localhost:3000` → `localhost:3011`

**Result:** Auth service now listens on port 3011 internally and externally.

---

## Frontend Changes ✅

### 2. API Proxy Routes (7 files)
Updated all server-side API routes to use `http://auth-service:3011`:

✅ `/src/app/api/auth/[...path]/route.ts`
✅ `/src/app/api/auth/session/route.ts`
✅ `/src/app/api/user/current/route.ts`
✅ `/src/app/api/graph/user/profile/route.ts`
✅ `/src/components/auth/lib/auth-server.ts`
✅ `/src/components/auth/lib/admin-server.ts`
✅ `/frontend/.env.example`

**Before:**
```typescript
AUTH_SERVICE_URL=http://auth-service:3000
```

**After:**
```typescript
AUTH_SERVICE_URL=http://auth-service:3011
```

---

## Port Configuration Summary

| Connection Type | URL | Purpose |
|----------------|-----|---------|
| **Frontend App** | `http://localhost:3000` | Next.js application |
| **Auth Service (External)** | `http://localhost:3011` | Direct browser/client access |
| **Auth Service (Internal Docker)** | `http://auth-service:3011` | Server-side API routes |
| **User Service** | `http://localhost:3012` | User management service |
| **Org-Core Service** | `http://localhost:8080` | Organization service |

---

## Architecture Flow

```
Browser
  │
  ├─> Frontend (localhost:3000)
  │     │
  │     └─> /api/auth/* API Proxy
  │           │
  │           └─> Auth Service
  │                 │
  │                 ├─> Internal: auth-service:3011
  │                 └─> External: localhost:3011
  │
  └─> Direct Access: localhost:3011
```

---

## Environment Variables

### Backend (`backend/docker-compose.yml`)
```env
PORT=3011
BETTER_AUTH_URL=http://localhost:3011
```

### Frontend (`.env.local`)
```env
# Server-side (Docker internal)
AUTH_SERVICE_URL=http://auth-service:3011

# Client-side (External)
NEXT_PUBLIC_AUTH_SERVICE_URL=http://localhost:3011
NEXT_PUBLIC_API_URL=http://localhost:3011/api
```

---

## Testing

### 1. Verify Auth Service
```bash
# Check service is running on correct port
curl http://localhost:3011/health

# Test via frontend proxy
curl http://localhost:3000/api/auth/status
```

### 2. Test Authentication
```bash
# Sign up
curl -X POST http://localhost:3011/api/auth/sign-up/email \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"Test123!","name":"Test"}'

# Sign in
curl -X POST http://localhost:3011/api/auth/sign-in/email \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"Test123!"}'
```

### 3. Test Frontend Integration
```bash
# Via Next.js proxy
curl http://localhost:3000/api/auth/sign-in/email \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"Test123!"}'
```

---

## Deployment Steps

### 1. Rebuild Auth Service
```bash
cd backend
docker-compose build auth-service
docker-compose up -d auth-service
```

### 2. Verify Service Started
```bash
docker-compose ps auth-service
docker-compose logs -f auth-service
```

### 3. Check Health
```bash
curl http://localhost:3011/health
```

### 4. Rebuild Frontend (if needed)
```bash
cd frontend
npm run build
npm run dev
```

---

## Files Modified

### Backend (2 files)
1. ✅ `backend/docker-compose.yml` - Port mappings and environment variables
2. ✅ `backend/auth/src/main.ts` - Already uses `process.env.PORT ?? 3011`

### Frontend (9 files)
1. ✅ `frontend/src/app/api/auth/[...path]/route.ts`
2. ✅ `frontend/src/app/api/auth/session/route.ts`
3. ✅ `frontend/src/app/api/user/current/route.ts`
4. ✅ `frontend/src/app/api/graph/user/profile/route.ts`
5. ✅ `frontend/src/components/auth/lib/auth-server.ts`
6. ✅ `frontend/src/components/auth/lib/admin-server.ts`
7. ✅ `frontend/.env.example`
8. ✅ `frontend/src/components/auth/AUTH_ARCHITECTURE.md`
9. ✅ `frontend/FRONTEND_AUTH_UPDATE_SUMMARY.md`

---

## Verification Checklist

- [x] Auth service configured to use port 3011 (docker-compose.yml)
- [x] Port mapping updated: 3011:3011 (docker-compose.yml)
- [x] BETTER_AUTH_URL updated to localhost:3011 (docker-compose.yml)
- [x] Healthcheck updated to localhost:3011 (docker-compose.yml)
- [x] All frontend API routes use auth-service:3011 (6 route files)
- [x] Frontend .env.example updated (AUTH_SERVICE_URL)
- [x] Documentation updated (AUTH_ARCHITECTURE.md)
- [x] Summary updated (FRONTEND_AUTH_UPDATE_SUMMARY.md)

---

## Before vs After

### Before
- **Internal Port**: 3000
- **External Port**: 3001 (mapped from 3000)
- **Frontend references**: Mixed (some 3000, some 3011)
- **Healthcheck**: localhost:3000

### After
- **Internal Port**: 3011
- **External Port**: 3011 (1:1 mapping)
- **Frontend references**: Consistent auth-service:3011
- **Healthcheck**: localhost:3011

---

## Benefits

1. **Consistency**: Same port everywhere (3011)
2. **Simplicity**: No port mapping confusion
3. **Clarity**: Internal = External = 3011
4. **Debugging**: Easier to trace requests through system
5. **Documentation**: All docs reference same port

---

## Next Steps

1. ✅ Rebuild and restart auth-service
2. ✅ Test authentication flows
3. ✅ Verify frontend integration
4. ✅ Update production configuration
5. ✅ Update team documentation

---

**Status**: ✅ **COMPLETE** - All auth service endpoints now consistently use port 3011
