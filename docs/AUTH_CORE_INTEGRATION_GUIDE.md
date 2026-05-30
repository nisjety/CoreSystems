# Auth-Core Integration: Seamless M365 Connection

## Overview

This document describes how **integration-core** seamlessly integrates with **auth-core** to enable a frictionless user experience where users authenticate once and can immediately access M365 sources without requiring additional OAuth re-authentication.

## Architecture

### Problem Solved

Previously, users had to:
1. Sign in with Microsoft (via auth-core)
2. THEN on Step 4 of onboarding, authenticate with Microsoft AGAIN to connect M365 sources
3. Very poor UX - essentially asking for the same permissions twice

### Solution: Token Reuse

Now users:
1. Sign in with Microsoft (auth-core captures and stores token)
2. Onboarding Step 4 **detects existing Microsoft token** from auth-core
3. Shows "Quick Connect" UI with source checkboxes pre-selected
4. User clicks "Aktiver" → sources are auto-discovered
5. **No extra auth needed** ✨

## Component Flow

```
Browser
  ├─ /onboarding/connect (Step 4: ConnectStep.tsx)
  │  ├─ On mount: Check /api/oauth/session-check
  │  │  └─ Returns: { has_microsoft, has_google, user_email }
  │  │
  │  ├─ If has_microsoft=true:
  │  │  ├─ Show "Quick Connect" UI
  │  │  ├─ Button: "Aktiver" → handleQuickConnect()
  │  │  └─ → Call /api/connections/from-auth-core
  │  │
  │  └─ If has_microsoft=false:
  │     ├─ Show "Gi tilgang" UI (existing OAuth flow)
  │     ├─ Button: "Gi tilgang" → handleConnect()
  │     └─ → OAuth flow → MS login
  │
Backend (Next.js API Routes)
  ├─ /api/oauth/session-check
  │  └─ → integration-core:/api/v1/session/validate
  │
  └─ /api/connections/from-auth-core
     └─ → integration-core:/api/v1/connections/from-auth-core
  
Integration-Core Service
  ├─ POST /api/v1/session/validate
  │  ├─ Input: Authorization header (auth-core session token)
  │  ├─ Logic: Call auth-core validate, extract providers
  │  └─ Output: { has_microsoft, has_google, providers, user_email }
  │
  ├─ POST /api/v1/connections/from-auth-core
  │  ├─ Input: { org_id, user_id, sources }
  │  ├─ Auth: Validate session with auth-core
  │  ├─ Extract: Token from auth-core session
  │  ├─ Save: To integration_core.oauth_tokens (source="auth-core")
  │  ├─ Discover: Documents from selected sources
  │  └─ Output: { success, documents_discovered }
  │
  └─ AuthCoreBridge (app/auth_core_bridge.py)
     ├─ get_auth_core_session() → Validate session with auth-core
     ├─ extract_microsoft_token() → Get token from session
     ├─ extract_google_token() → Get Google token if present
     └─ save_token_from_auth_core() → Store token in DB (source="auth-core")

PostgreSQL (integration database)
  └─ oauth_tokens
     ├─ id, org_id, user_id, provider
     ├─ access_token, refresh_token, id_token
     ├─ provider ("microsoft" | "google")
     └─ source ("direct" | "auth-core")  ← Tracks where token came from
```

## Key Files

### Backend Code

**integration-core/app/auth_core_bridge.py** (NEW)
- `AuthCoreBridge` class
- Methods:
  - `get_auth_core_session()` - Validates session with auth-core
  - `extract_microsoft_token()` - Extracts Microsoft token from session
  - `extract_google_token()` - Extracts Google token from session
  - `get_available_providers()` - Lists OAuth providers user has
  - `save_token_from_auth_core()` - Stores token in integration-core DB

**integration-core/app/main.py** (UPDATED)
- New endpoints:
  - `POST /api/v1/session/validate` - Check if user has Microsoft/Google
  - `POST /api/v1/connections/from-auth-core` - Create connections using reused token

**integration-core/app/models.py** (UPDATED)
- `OAuthToken` model now includes:
  - `provider` field (default "microsoft", can be "google")
  - `source` field (tracks "direct" vs "auth-core")

**integration-core/app/config.py** (UPDATED)
- `auth_core_url` setting for calling auth-core service

### Frontend Code

**src/app/api/oauth/session-check/route.ts** (NEW)
- GET endpoint
- Calls integration-core `/api/v1/session/validate`
- Returns: `{ has_microsoft, has_google, user_email }`

**src/app/api/connections/from-auth-core/route.ts** (NEW)
- POST endpoint
- Calls integration-core `/api/v1/connections/from-auth-core`
- Creates connections using reused auth-core token

**src/components/onboarding/core/ConnectStep.tsx** (UPDATED)
- On mount: Calls `/api/oauth/session-check`
- Conditionally shows "Quick Connect" or OAuth flow
- New button: "Aktiver" (if has_microsoft) → `handleQuickConnect()`
- Existing button: "Gi tilgang" (no Microsoft token) → `handleConnect()` (OAuth)

## User Flows

### Flow 1: User with Existing Microsoft Auth-Core Token (Seamless ✨)

```
1. User arrives at /onboarding/connect (Step 4)
   ├─ Frontend: GET /api/oauth/session-check
   │  └─ Response: { has_microsoft: true, user_email: "user@contoso.com" }
   │
2. ConnectStep renders "Quick Connect" UI
   ├─ Badge: "Du er koblet til som user@contoso.com"
   ├─ Checkboxes (SharePoint, OneDrive, Teams, Outlook - all pre-selected)
   └─ Button: "Aktiver"
   │
3. User clicks "Aktiver"
   ├─ Frontend: POST /api/connections/from-auth-core
   │  ├─ Includes: { org_id, user_id, sources }
   │  └─ API Route: Calls integration-core
   │     └─ integration-core: Extract Microsoft token from auth-core session
   │        ├─ VALIDATE auth-core session
   │        ├─ EXTRACT token (from session.accounts[provider="microsoft"])
   │        ├─ SAVE to oauth_tokens (source="auth-core")
   │        └─ DISCOVER documents from sources
   │
4. Frontend: Receives { success: true, documents_discovered: 1250 }
   └─ Navigates to /onboarding/team (Step 5)

⏱️ Total time: ~2-3 seconds (no user interaction required!)
```

### Flow 2: User Without Microsoft Auth-Core Token (Existing OAuth Flow)

```
1. User arrives at /onboarding/connect (Step 4)
   ├─ Frontend: GET /api/oauth/session-check
   │  └─ Response: { has_microsoft: false, authenticated: false }
   │
2. ConnectStep renders OAuth UI (existing behavior)
   ├─ No blue badge
   ├─ Checkboxes (SharePoint, OneDrive pre-selected)
   └─ Button: "Gi tilgang"
   │
3. User clicks "Gi tilgang"
   ├─ Frontend: POST /api/oauth/initiate
   │  └─ Returns: { url: "https://login.microsoftonline.com/..." }
   │
4. Browser: Redirects to Microsoft OAuth
   ├─ User: Sees Microsoft consent screen
   ├─ User: Clicks "Accept"
   ├─ Microsoft: Redirects to /onboarding/connect/callback?code=...
   │
5. Frontend: Detects OAuth callback
   ├─ POST /api/oauth/callback (exchange code for token)
   ├─ POST /api/connections/create (create connections)
   └─ Navigate to /onboarding/team (Step 5)

⏱️ Total time: ~30-60 seconds (includes OAuth consent screen)
```

### Flow 3: User Signs In with Google (Future)

Currently implemented (scaffolding):
- ConnectStep checks for `has_google`
- Integration-core can extract Google tokens
- Future: Google Workspace integration (Docs, Drive, etc.)

## Database Schema

### `oauth_tokens` Table

```sql
CREATE TABLE oauth_tokens (
    id UUID PRIMARY KEY,
    org_id VARCHAR(255) NOT NULL,         -- Organization ID
    user_id VARCHAR(255) NOT NULL,        -- User ID
    provider VARCHAR(50) DEFAULT 'microsoft',  -- "microsoft" or "google"
    microsoft_user_id VARCHAR(255),       -- Microsoft Graph OID (for Microsoft tokens)
    access_token TEXT NOT NULL,           -- Current access token
    refresh_token TEXT,                   -- For token refresh
    id_token TEXT,                        -- JWT with claims
    token_type VARCHAR(50) DEFAULT 'Bearer',
    expires_at TIMESTAMP NOT NULL,        -- When access_token expires
    scopes TEXT,                          -- Space-separated OAuth scopes
    source VARCHAR(50) DEFAULT 'direct',  -- "direct" (user re-auth) or "auth-core" (reused)
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    last_used_at TIMESTAMP,
    
    INDEX idx_org_user (org_id, user_id),
    INDEX idx_provider (provider)
);
```

Key difference: 
- `source='direct'` → User re-authenticated in ConnectStep OAuth flow
- `source='auth-core'` → Token reused from login session (seamless) ✨

## Security Considerations

### Token Handling

1. **No token passed to frontend** - Tokens stay server-side
2. **Session validation** - Always validate auth-core session before trusting tokens
3. **Token encryption** - Auth-core encrypts tokens before storage; integration-core receives plaintext
4. **Expiration handling** - Tokens auto-refresh if needed via MSAL library
5. **Tenant isolation** - `org_id + user_id` ensures data isolation

### CORS & Cross-Service Communication

1. **Internal calls only** - integration-core → auth-core uses docker internal network (`http://auth-core:3011`)
2. **Session validation** - All token extraction gated behind auth-core session validation
3. **No user data leaked** - Only tokens are exchanged, no user PII

## Testing the Integration

### Test 1: Quick Connect (Seamless UX)

```bash
# 1. User logs in with Microsoft
# 2. Navigate to /onboarding/connect
# 3. Should show:
#    - Blue badge: "Du er koblet til som user@contoso.com"
#    - "Aktiver" button
# 4. Click "Aktiver"
# 5. Should show documents discovered
# 6. Auto-advances to Step 5
```

### Test 2: Manual OAuth (Existing Flow)

```bash
# 1. User WITHOUT Microsoft in auth-core
# 2. Navigate to /onboarding/connect
# 3. Should show:
#    - NO blue badge
#    - "Gi tilgang" button
# 4. Click "Gi tilgang"
# 5. Redirect to Microsoft login
# 6. Complete OAuth consent
# 7. Auto-advances to Step 5
```

### Test 3: Token Refresh

```bash
# Tokens are auto-refreshed by MSAL if expired
# oauth.py: refresh_token_if_needed()
# Refreshes if token expires within 5 minutes
```

## Error Handling

### Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| "Invalid or expired session" | auth-core session invalid | User should sign in again |
| "Microsoft token not found" | User didn't auth with Microsoft in auth-core | Show OAuth flow |
| "Failed to discover documents" | Microsoft Graph API error | Check permissions, retry |
| "Connection creation failed" | Database error | Check integration-core logs |

### Logging

Enable debug logs:
```bash
export LOG_LEVEL=DEBUG
# integration-core will log:
# - Session validation attempts
# - Token extraction steps
# - Document discovery progress
```

## Future Enhancements

1. **Google Workspace** - Reuse Google tokens for Docs, Drive, GMail
2. **Selective Sync** - User picks which documents to ingest (not all)
3. **Incremental Sync** - Only sync new/updated documents
4. **Conflict Resolution** - Handle duplicate documents across services
5. **Sync History** - Track which documents came from which source
6. **Offline Sync** - Queue syncs if services unavailable

## Debugging

### Check if auth-core session is valid

```bash
curl -X POST http://localhost:9026/api/v1/session/validate \
  -H "Authorization: Bearer <session-token>" \
  -H "Content-Type: application/json"
```

### Check if token was extracted

```sql
-- In integration-core DB
SELECT * FROM oauth_tokens 
WHERE org_id='...' AND user_id='...' AND source='auth-core';
```

### Check integration-core logs

```bash
docker logs integration-api --tail=50
# Watch for auth-core bridge logs
# Look for: "Successfully extracted Microsoft token from auth-core session"
```

## References

- **better-auth**: Account linking, social providers
- **MSAL.py**: Microsoft OAuth, token refresh
- **Microsoft Graph API**: Document discovery
- **SQLAlchemy**: ORM for token storage

---

Last Updated: Feb 28, 2026  
Status: ✅ Complete and tested
