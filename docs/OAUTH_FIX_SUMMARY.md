# OAuth Authentication Fix Summary

## Issue
The frontend was trying to call the OAuth initiation endpoint at the wrong service:
- **Error**: `http://localhost:8080/api/auth/oauth/initiate` returned 404
- **Root Cause**: Frontend was configured to call Org Core (port 8080) instead of Auth Service (port 3011)

## Solution

### Updated Environment Variables in docker-compose.yml

**Changed:**
```yaml
# Before (WRONG)
- NEXT_PUBLIC_API_URL=http://localhost:8080  # Was pointing to Org Core
- BETTER_AUTH_URL=http://auth-service:3011   # Internal only

# After (CORRECT)  
- NEXT_PUBLIC_API_URL=http://localhost:3011        # Points to Auth Service
- NEXT_PUBLIC_AUTH_URL=http://localhost:3011       # Client-side auth URL
- NEXT_PUBLIC_BACKEND_URL=http://localhost:8080    # Org Core for other APIs
- BETTER_AUTH_URL=http://localhost:3011            # Public auth URL
```

## Service Port Mapping

| Service | Port | Purpose |
|---------|------|---------|
| **Auth Service** | 3011 | Authentication, OAuth, SSO |
| **User Service** | 3012 | User profile management |
| **Org Core** | 8080 | Organization APIs |
| **AI Core** | 8040 | AI agent core |
| **Convex** | 3210 | Realtime backend |

## OAuth Endpoints (Auth Service - Port 3011)

- `POST /api/v2/auth/oauth/initiate` - Initiate OAuth flow
- `GET /api/auth/oauth/callback/:provider` - OAuth callback
- `GET /api/auth/get-session` - Get current session

## Microsoft SSO Configuration

The auth service supports Microsoft Entra ID (Azure AD) SSO:

```env
MICROSOFT_CLIENT_ID=your-client-id
MICROSOFT_CLIENT_SECRET=your-secret
MICROSOFT_TENANT_ID=common
MICROSOFT_SCOPE=openid profile email User.Read
```

## Testing OAuth

After the frontend rebuild completes, test Microsoft sign-in:

1. Open http://localhost:3000
2. Click "Sign in with Microsoft"
3. The frontend should now correctly call:
   ```
   POST http://localhost:3011/api/v2/auth/oauth/initiate
   ```
4. Check auth service logs:
   ```bash
   docker-compose logs -f auth-service
   ```

## Build Process

The frontend needs a full rebuild because:
- `NEXT_PUBLIC_*` environment variables are baked into the JavaScript bundle at build time
- Changing them in docker-compose.yml only affects new builds
- Used `--no-cache` to ensure clean build with new variables

## Verification

After rebuild, verify in browser console:
```javascript
// These should now point to port 3011
console.log(process.env.NEXT_PUBLIC_API_URL);        // http://localhost:3011
console.log(process.env.NEXT_PUBLIC_AUTH_URL);       // http://localhost:3011
```

## Related Files

- `/Volumes/Lagring/Triodelab/CoreSystem/docker-compose.yml` - Updated frontend environment
- `/Volumes/Lagring/Triodelab/CoreSystem/frontend/Dockerfile` - Frontend build configuration
- Auth service logs showing successful endpoint registration:
  ```
  [RouterExplorer] Mapped {/api/v2/auth/oauth/initiate, POST} route
  ```
