# Frontend Auth Architecture Documentation

## Overview
The frontend uses **Better Auth** for authentication, with a Next.js API proxy pattern to communicate with the backend auth service.

## Port Configuration

### Service Ports
| Service | Internal (Docker) | External (localhost) | Purpose |
|---------|------------------|---------------------|---------|
| Frontend | 3000 | 3000 | Next.js app |
| Auth Service | 3011 | 3011 | Backend auth API |

### URL Structure
- **Client-side requests**: `http://localhost:3000/api/auth/*` → proxied to auth service
- **Server-side requests**: `http://auth-service:3011` (Docker internal network)
- **Direct backend access**: `http://localhost:3011` (external/testing)

## Architecture Flow

```
┌─────────────────────────────────────────────────────────────┐
│                     Browser (Client)                         │
└────────────────┬────────────────────────────────────────────┘
                 │
                 │ HTTP Requests
                 ▼
┌─────────────────────────────────────────────────────────────┐
│             Next.js Frontend (Port 3000)                     │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Auth Client (Better Auth React)                      │  │
│  │  - authClient.signIn.email()                          │  │
│  │  - authClient.signUp.email()                          │  │
│  │  - authClient.getSession()                            │  │
│  └───────────────────┬──────────────────────────────────┘  │
│                      │                                       │
│                      │ Calls /api/auth/*                    │
│                      ▼                                       │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  API Routes Proxy                                     │  │
│  │  /app/api/auth/[...path]/route.ts                    │  │
│  │  - Forwards to auth service                           │  │
│  │  - Handles cookies                                    │  │
│  │  - Adds internal API key                             │  │
│  └───────────────────┬──────────────────────────────────┘  │
└────────────────────┬─┘                                       │
                     │                                         │
                     │ HTTP Proxy                             │
                     ▼                                         │
┌─────────────────────────────────────────────────────────────┐
│        Auth Service (Backend - Port 3011)                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Better Auth Server                                   │  │
│  │  - Handles authentication                             │  │
│  │  - Manages sessions                                   │  │
│  │  - OAuth/SSO integration                              │  │
│  │  - Organization management                            │  │
│  └───────────────────┬──────────────────────────────────┘  │
└────────────────────┬─┘                                       │
                     │                                         │
                     ▼                                         │
              ┌──────────────┐                                │
              │  PostgreSQL   │                                │
              │  (Database)   │                                │
              └──────────────┘                                 │
```

## File Structure

### Core Auth Files

#### `/src/components/auth/`
- **`lib/auth-client.ts`** - Better Auth React client configuration
- **`lib/auth-server.ts`** - Server-side auth utilities (session management)
- **`config/auth.ts`** - Auth configuration (ports, features)
- **`hooks/use-auth.tsx`** - React hooks for auth state
- **`contexts/AuthContext.tsx`** - Auth context provider

#### `/src/app/api/auth/`
- **`[...path]/route.ts`** - Main API proxy (forwards all `/api/auth/*` to backend)
- **`session/route.ts`** - Session management endpoint
- **`get-session/route.ts`** - Get current session

#### `/src/lib/services/`
- **`auth-service.ts`** - Auth service class (convenience wrapper)

## Configuration

### Environment Variables

#### Client-Side (NEXT_PUBLIC_*)
```env
NEXT_PUBLIC_APP_URL=http://localhost:3000
NEXT_PUBLIC_AUTH_SERVICE_URL=http://localhost:3011
NEXT_PUBLIC_API_URL=http://localhost:3011/api
```

#### Server-Side
```env
AUTH_SERVICE_URL=http://auth-service:3011  # Docker internal
BACKEND_URL=http://localhost:3011          # External/testing
INTERNAL_API_KEY=dev-super-secret-internal-api-key
```

### Auth Client Configuration
File: `/src/components/auth/lib/auth-client.ts`

```typescript
export const authClient = createAuthClient({
  baseURL: "http://localhost:3000",  // Frontend URL
  basePath: "/api/auth",              // Proxy path
  plugins: [
    genericOAuthClient(),  // Custom OAuth (Vipps, Okta)
    twoFactorClient(),     // 2FA support
    passkeyClient(),       // WebAuthn/Passkeys
    adminClient(),         // Admin features
  ],
});
```

## API Endpoints

### Authentication Endpoints (via Better Auth)

#### Sign Up
```typescript
authClient.signUp.email({
  email: "user@example.com",
  password: "password",
  name: "User Name"
})
```
Calls: `POST /api/auth/sign-up/email` → `POST http://auth-service:3011/api/auth/sign-up/email`

#### Sign In
```typescript
authClient.signIn.email({
  email: "user@example.com",
  password: "password"
})
```
Calls: `POST /api/auth/sign-in/email` → `POST http://auth-service:3011/api/auth/sign-in/email`

#### Get Session
```typescript
authClient.getSession()
```
Calls: `GET /api/auth/get-session` → `GET http://auth-service:3011/api/auth/get-session`

#### Sign Out
```typescript
authClient.signOut()
```
Calls: `POST /api/auth/sign-out` → `POST http://auth-service:3011/api/auth/sign-out`

### OAuth/SSO Endpoints

#### Microsoft/Google Sign In
```
GET /api/auth/sign-in/microsoft
GET /api/auth/sign-in/google
```

#### Custom OAuth (Vipps, Okta)
```
GET /api/auth/oauth/initiate?provider=vipps
```

### Organization Endpoints

#### Create Organization
```typescript
POST /api/auth/organization/create
Body: { name: "Company Name", slug: "company-slug" }
```

#### Invite Member
```typescript
POST /api/auth/organization/invite-member
Body: { email: "member@example.com", role: "member" }
```

## Usage Examples

### 1. Sign Up with Organization
```typescript
import { authClient } from '@/components/auth/lib/auth-client'

// Sign up user
const result = await authClient.signUp.email({
  email: "user@company.com",
  password: "SecurePass123!",
  name: "John Doe",
  organizationName: "Acme Corp" // Creates org automatically
})

if (result.error) {
  console.error(result.error)
} else {
  // User signed up and org created
  console.log(result.data.user)
}
```

### 2. SSO Sign In (Microsoft)
```typescript
import { buildOAuthURL } from '@/components/auth/utils/oauth'

const oauthUrl = buildOAuthURL('microsoft', '/dashboard')
window.location.href = oauthUrl
```

### 3. Check Authentication (Server-Side)
```typescript
import { getServerSession } from '@/components/auth/lib/auth-server'

export async function GET() {
  const session = await getServerSession()
  
  if (!session) {
    return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
  }
  
  return NextResponse.json({ user: session.user })
}
```

### 4. Protected Component (Client-Side)
```typescript
import { useAuth } from '@/components/auth/hooks/use-auth'

export function ProtectedComponent() {
  const { user, isLoading } = useAuth()
  
  if (isLoading) return <div>Loading...</div>
  if (!user) return <div>Please sign in</div>
  
  return <div>Welcome, {user.name}!</div>
}
```

## Session Management

### Session Storage
- **Backend**: Sessions stored in PostgreSQL with Redis cache
- **Frontend**: Session cookie (`__Secure-sid`) managed by Better Auth
- **Expiry**: Configurable (default: 7 days)

### Session Flow
1. User signs in via `authClient.signIn.email()`
2. Auth service creates session and sets secure cookie
3. Cookie automatically included in subsequent requests
4. Frontend reads session via `authClient.getSession()`
5. Server-side routes validate session via `getServerSession()`

## Security Features

### CSRF Protection
- Better Auth includes built-in CSRF tokens
- Cookies are `httpOnly`, `secure`, and `sameSite`

### Rate Limiting
- Implemented in auth service
- Prevents brute-force attacks

### Password Requirements
- Minimum 8 characters
- Must include uppercase, lowercase, number, special char
- Validated both client and server-side

### 2FA Support
- TOTP (Time-based One-Time Password)
- SMS verification (optional)
- Configured via `twoFactorClient()` plugin

### Passkey Support (WebAuthn)
- Passwordless authentication
- Biometric/hardware key support
- Configured via `passkeyClient()` plugin

## Troubleshooting

### Common Issues

#### 1. "Failed to fetch" errors
**Cause**: Auth service not running or wrong port
**Fix**: 
```bash
cd backend
docker-compose up -d auth-service
curl http://localhost:3011/api/auth/status
```

#### 2. CORS errors
**Cause**: Missing CORS headers in proxy
**Fix**: Check `/app/api/auth/[...path]/route.ts` includes:
```typescript
headers: {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
}
```

#### 3. Session not persisting
**Cause**: Cookie configuration issues
**Fix**: Ensure `BETTER_AUTH_URL` matches frontend URL exactly

#### 4. OAuth redirect fails
**Cause**: Callback URL mismatch
**Fix**: Verify OAuth provider settings match `http://localhost:3011/api/auth/callback/[provider]`

### Debug Mode

Enable detailed logging:
```typescript
// In auth-client.ts
export const authClient = createAuthClient({
  baseURL: "http://localhost:3000",
  basePath: "/api/auth",
  fetchOptions: {
    onRequest(context) {
      console.log('Request:', context.request)
    },
    onResponse(context) {
      console.log('Response:', context.response)
    }
  }
})
```

## Testing

### Test Authentication Flow
```bash
# 1. Sign up
curl -X POST http://localhost:3000/api/auth/sign-up/email \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"Test123!","name":"Test User"}'

# 2. Sign in
curl -X POST http://localhost:3000/api/auth/sign-in/email \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"Test123!"}'

# 3. Get session
curl http://localhost:3000/api/auth/get-session \
  -H "Cookie: __Secure-sid=SESSION_TOKEN"
```

## Production Deployment

### Environment Variables
```env
# Production
NEXT_PUBLIC_APP_URL=https://yourdomain.com
NEXT_PUBLIC_AUTH_SERVICE_URL=https://auth.yourdomain.com
AUTH_SERVICE_URL=http://auth-service:3000
BETTER_AUTH_SECRET=generated-secret-key
INTERNAL_API_KEY=generated-api-key
```

### Security Checklist
- [ ] Update `BETTER_AUTH_SECRET` to strong random value
- [ ] Update `INTERNAL_API_KEY` to strong random value
- [ ] Enable HTTPS/TLS for all connections
- [ ] Configure proper CORS origins
- [ ] Set up rate limiting
- [ ] Enable session encryption
- [ ] Configure secure cookie settings
- [ ] Set up logging and monitoring
- [ ] Configure OAuth redirect URLs for production domain

## Related Documentation
- [Better Auth Docs](https://www.better-auth.com/docs)
- [Backend Auth Service](../../../backend/auth/README.md)
- [Organization Management](./ORGANIZATION.md)
- [SSO Configuration](./SSO_SETUP.md)
