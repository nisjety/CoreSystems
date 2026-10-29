# Frontend Auth Update Summary

## Changes Made

### 1. Port Configuration ✅
All frontend files now correctly reference:
- **External Auth Service**: `http://localhost:3011` (client-side access)
- **Internal Docker**: `http://auth-service:3011` (server-side API routes)
- **Frontend**: `http://localhost:3000`

### 2. Files Updated

#### Configuration Files
✅ `/src/components/auth/config/auth.ts`
- `backendUrl: 'http://localhost:3011'` ✓
- `frontendUrl: 'http://localhost:3000'` ✓

#### API Proxy Routes (Server-Side)
✅ `/src/app/api/auth/[...path]/route.ts`
- Internal: `http://auth-service:3011` ✓
- Proxies all `/api/auth/*` requests to backend

✅ `/src/app/api/auth/session/route.ts`
- Internal: `http://auth-service:3011` ✓

✅ `/src/app/api/user/current/route.ts`
- Internal: `http://auth-service:3011` ✓

✅ `/src/app/api/graph/user/profile/route.ts`
- Internal: `http://auth-service:3011` ✓

#### Auth Libraries
✅ `/src/components/auth/lib/auth-server.ts`
- Internal: `http://auth-service:3011` ✓
- Used for server-side session management

✅ `/src/components/auth/lib/admin-server.ts`
- Internal: `http://auth-service:3011` ✓
- Used for admin operations

✅ `/src/components/auth/lib/auth-client-enterprise.ts`
- Already configured correctly ✓
- Uses frontend URL + `/api/auth` proxy path

#### Service Layer
✅ `/src/lib/services/auth-service.ts`
- **Completely rewritten** to use Better Auth client
- Now properly integrates with `authClient`
- Methods: `login()`, `logout()`, `register()`, `getCurrentUser()`, `getToken()`

#### Environment Configuration
✅ `/frontend/.env.example`
- Added comprehensive auth configuration
- Documented all required environment variables
- Separated client-side vs server-side variables

### 3. Documentation Created

✅ `/src/components/auth/AUTH_ARCHITECTURE.md`
- Complete architecture documentation
- Port mappings and flow diagrams
- Usage examples
- Troubleshooting guide
- API endpoints reference

## Architecture Overview

```
┌─────────────┐
│   Browser   │
└──────┬──────┘
       │ http://localhost:3000
       ▼
┌─────────────────────────┐
│  Next.js Frontend       │
│  (Port 3000)            │
│                         │
│  ┌──────────────────┐  │
│  │ Better Auth      │  │
│  │ Client           │  │
│  └────────┬─────────┘  │
│           │             │
│           │ /api/auth/* │
│           ▼             │
│  ┌──────────────────┐  │
│  │ API Proxy        │  │
│  │ [..path]/route   │  │
│  └────────┬─────────┘  │
└───────────┼─────────────┘
            │
            │ http://auth-service:3011
            │ (Docker internal)
            ▼
┌─────────────────────────┐
│  Auth Service           │
│  (Port 3011)            │
│                         │
│  ┌──────────────────┐  │
│  │ Better Auth      │  │
│  │ Server           │  │
│  └────────┬─────────┘  │
└───────────┼─────────────┘
            │
            ▼
     ┌──────────────┐
     │  PostgreSQL  │
     └──────────────┘
```

## Environment Variables

### Required for Frontend

```env
# Client-side (public)
NEXT_PUBLIC_APP_URL=http://localhost:3000
NEXT_PUBLIC_AUTH_SERVICE_URL=http://localhost:3011
NEXT_PUBLIC_API_URL=http://localhost:3011/api

# Server-side (private)
AUTH_SERVICE_URL=http://auth-service:3011
INTERNAL_API_KEY=dev-super-secret-internal-api-key
```

## Key Features Configured

### ✅ Better Auth Integration
- Client: `/src/components/auth/lib/auth-client-enterprise.ts`
- Server: `/src/components/auth/lib/auth-server.ts`
- Proxy: `/src/app/api/auth/[...path]/route.ts`

### ✅ Authentication Methods
- Email/Password
- OAuth/SSO (Microsoft, Google)
- Custom OAuth (Vipps, Okta via generic OAuth)
- Passkeys (WebAuthn)
- Two-Factor Authentication
- Phone Number Verification

### ✅ Organization Management
- Create organizations on signup
- Invite members
- Role-based access control
- Multi-tenant support

### ✅ Security Features
- Secure cookie handling (`__Secure-sid`)
- CSRF protection
- Rate limiting (backend)
- Internal API key for service-to-service auth
- Password strength validation

## Usage Examples

### Sign Up
```typescript
import { authClient } from '@/components/auth/lib/auth-client-enterprise'

const result = await authClient.signUp.email({
  email: "user@example.com",
  password: "SecurePass123!",
  name: "John Doe",
  organizationName: "Acme Corp"  // Optional
})
```

### Sign In
```typescript
const result = await authClient.signIn.email({
  email: "user@example.com",
  password: "SecurePass123!"
})
```

### Get Session (Client-Side)
```typescript
const session = await authClient.getSession()
if (session?.data?.user) {
  console.log("User:", session.data.user)
}
```

### Get Session (Server-Side)
```typescript
import { getServerSession } from '@/components/auth/lib/auth-server'

const session = await getServerSession()
if (!session) {
  return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
}
```

### Using Auth Service (Alternative)
```typescript
import { authService } from '@/lib/services/auth-service'

// Sign in
const session = await authService.login({
  email: "user@example.com",
  password: "password"
})

// Get current user
const user = await authService.getCurrentUser()

// Check if authenticated
const isAuth = authService.isAuthenticated()
```

## Testing

### 1. Test Proxy Route
```bash
curl http://localhost:3000/api/auth/status
```

### 2. Test Sign Up
```bash
curl -X POST http://localhost:3000/api/auth/sign-up/email \
  -H "Content-Type: application/json" \
  -d '{
    "email": "test@example.com",
    "password": "Test123!",
    "name": "Test User"
  }'
```

### 3. Test Sign In
```bash
curl -X POST http://localhost:3000/api/auth/sign-in/email \
  -H "Content-Type: application/json" \
  -d '{
    "email": "test@example.com",
    "password": "Test123!"
  }'
```

### 4. Test Session
```bash
curl http://localhost:3000/api/auth/get-session \
  -H "Cookie: __Secure-sid=YOUR_SESSION_TOKEN"
```

## Verification Checklist

- [x] All API routes use correct internal Docker URL (`http://auth-service:3011`)
- [x] Client-side code uses correct external URL (`http://localhost:3011`)
- [x] Auth client configured with proper proxy path (`/api/auth`)
- [x] Environment variables documented
- [x] Service layer updated to use Better Auth
- [x] Architecture documentation created
- [x] Security features configured (CSRF, secure cookies, API keys)
- [x] Organization management enabled
- [x] SSO/OAuth support configured

## Next Steps

### 1. Create .env.local
Copy the example and configure:
```bash
cd frontend
cp .env.example .env.local
# Edit .env.local with your values
```

### 2. Start Frontend
```bash
npm install
npm run dev
```

### 3. Test Authentication Flow
1. Open http://localhost:3000/sign-in
2. Try signing up with a new user
3. Verify session persistence
4. Test organization creation

### 4. Configure OAuth (Optional)
If using Microsoft/Google SSO:
1. Set up OAuth app in provider console
2. Add credentials to `.env.local`
3. Update redirect URIs to `http://localhost:3011/api/auth/callback/[provider]`

## Troubleshooting

### Issue: "Failed to fetch"
**Solution**: Ensure auth-service is running:
```bash
cd backend
docker-compose up -d auth-service
docker-compose ps auth-service
```

### Issue: Session not persisting
**Solution**: Check cookie configuration in backend auth service. Ensure `BETTER_AUTH_URL` matches frontend exactly.

### Issue: CORS errors
**Solution**: Verify proxy route includes CORS headers. Check `/src/app/api/auth/[...path]/route.ts`.

### Issue: OAuth redirect fails
**Solution**: Verify callback URLs in OAuth provider settings match `http://localhost:3011/api/auth/callback/[provider]`.

## Production Deployment

Before deploying to production:

1. **Update Environment Variables**
   ```env
   NEXT_PUBLIC_APP_URL=https://yourdomain.com
   NEXT_PUBLIC_AUTH_SERVICE_URL=https://auth.yourdomain.com
   AUTH_SERVICE_URL=http://auth-service:3011
   ```

2. **Generate Secure Secrets**
   ```bash
   openssl rand -base64 32  # BETTER_AUTH_SECRET
   openssl rand -base64 32  # INTERNAL_API_KEY
   ```

3. **Configure OAuth Redirect URIs**
   Update all OAuth providers to use production URLs:
   - `https://auth.yourdomain.com/api/auth/callback/microsoft`
   - `https://auth.yourdomain.com/api/auth/callback/google`

4. **Enable HTTPS**
   Ensure all connections use TLS/SSL in production.

5. **Set up Monitoring**
   - Error tracking (Sentry)
   - Performance monitoring
   - Session analytics

## Summary

✅ **Complete**: Frontend auth system is now properly configured
✅ **Secure**: All security features enabled
✅ **Documented**: Comprehensive architecture docs created
✅ **Production-Ready**: Ready for deployment with proper configuration

All files now correctly reference:
- `http://localhost:3011` for external/client access
- `http://auth-service:3011` for internal Docker communication
- `/api/auth/*` proxy pattern for Better Auth integration
