# Frontend Manual End-to-End Test Guide

Use this guide to manually test the running frontend system through the browser.
This is a **visual walk-through** explaining what you should see and how it connects to the backend.

---

## System Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│ Frontend (http://localhost:3000) — Next.js App Router           │
│ ├─ pages: /, /chat, /dashboard, /knowledge, /admin, /auth       │
│ ├─ api routes: /api/auth/*, /api/admin/*, /api/reasoning/*      │
│ └─ hot-reload volume mount (changes refresh instantly)          │
└─────────────────────────────────────────────────────────────────┘
                           ↓
                    (All requests via Next.js
                   API routes as proxies)
                           ↓
┌──────────────────────── Service Network ─────────────────────────┐
│  Docker Network: triodelab-net (all containers share this)       │
│                                                                  │
│  Control Plane (Port 3011)                                       │
│  ├─ auth-core — Authentication (Better Auth + oRPC)              │
│  ├─ user-core — User profiles (port 3012)                        │
│  ├─ org-core — Organization management (port 8080)               │
│  ├─ billing-core — Billing (port 3014)                           │
│  └─ nats — Event broker (internal)                               │
│                                                                    │
│  Reasoning Plane (Ports 8100-8101)                                │
│  ├─ ai-core — LLM service (8100) + gRPC (50051)                  │
│  └─ reasoning-core — Chain-of-thought reasoning (8101)           │
│                                                                    │
│  Data Plane (Ports 9401, 9404, 8092)                             │
│  ├─ documents-service — Document storage & indexing (9401)       │
│  ├─ retrieval-service — Vector/semantic search (9404)            │
│  └─ quarry-api — Web crawler & knowledge base (8092)             │
│                                                                    │
│  Ingestion Plane (Ports 9025, 9026)                              │
│  ├─ imports-api — File upload & conversion (9025)                │
│  └─ integration-api — External connectors (9026)                 │
└────────────────────────────────────────────────────────────────────┘
```

All services are reachable from localhost via their external ports listed above.

---

## How to Test: Manual Browser Walk-Through

Open **http://localhost:3000** in your browser and follow these steps.

---

## Test 1: Landing & Navigation

**What you should see:**
- The home page loads instantly (Next.js dev server with hot-reload)
- Navigation sidebar on the left with menu items:
  - ✅ **Inbox** (dashboard, route: `/`)
  - ✅ **AI Agents** (route: `/ai-studio`)
  - ✅ **Knowledge** (route: `/knowledge`) — *Previously "Notes", now disabled/removed from backend*
  - ✅ **Reports** (route: `/file-manager`)
  - ✅ **Outbound** (route: `/erp-connector`)
  - ✅ **Contacts** (route: `/contract-admin`)

**Code reference:**
- Sidebar config: [components/core/sidebar/config/nav-items.ts](components/core/sidebar/config/nav-items.ts) — defines these navigation items
- Navigation renders from: [components/core/sidebar/components/Navigation.tsx](components/core/sidebar/components/Navigation.tsx)

**Expected behavior:**
- All clicks navigate without full page reload (Next.js App Router)
- No JavaScript errors in browser console (F12 → Console tab)

---

## Test 2: Authentication & Sign-In

**Step 1: Navigate to `/sign-in`**

Click on any protected page (e.g., Dashboard) → you should be **redirected to `/sign-in` automatically**.

**Code:**
- Route: [apps/frontend/src/(auth)/sign-in/page.tsx](apps/frontend/src/(auth)/sign-in/page.tsx)
- Auth middleware redirects unauthenticated users using next-auth session check

**What you should see:**
- Sign-in form with multiple options:
  - Email/password login
  - **GitHub OAuth button** (will call our auth-service)
  - **Google OAuth button**
  - Other provider buttons

**Step 2: Click OAuth Button (e.g., GitHub)**

When you click GitHub:

1. Frontend calls: **POST /api/auth/oauth/initiate**
2. This proxies to: **auth-service:3011 /api/v2/auth/oauth/initiate**
3. Auth-service returns a redirect URL to GitHub's OAuth portal
4. Frontend redirects your browser to that URL
5. You authenticate with GitHub
6. GitHub redirects back to: **http://localhost:3000/api/oauth/callback** (handled by [apps/frontend/src/app/api/oauth/callback/route.ts](apps/frontend/src/app/api/oauth/callback/route.ts))
7. Callback route exchanges the auth code for a session token
8. Session is stored in browser cookies
9. You're redirected to the dashboard

**Code flow:**
```
Sign-in page (auth/lib/api/auth-provider-client.ts)
    ↓ user clicks GitHub
Frontend POST /api/auth/oauth/initiate
    ↓ (route: src/app/api/auth/oauth/initiate/route.ts)
Auth-service POST /api/v2/auth/oauth/initiate
    ↓ returns: {url: "https://github.com/login/oauth/authorize?..."}
Frontend redirects to GitHub
    ↓ user logs in
GitHub redirects to localhost:3000/api/oauth/callback?code=XXX
    ↓ (route: src/app/api/oauth/callback/route.ts)
Frontend exchanges code for session
    ↓ calls auth-service
Session stored → redirect to /dashboard
```

**Check in browser DevTools:**
- Open F12 → Network → repeat OAuth sign-in
- You should see:
  - `POST /api/auth/oauth/initiate` → 200 response with `{url: "https://github.com..."}`
  - After callback: `POST /api/oauth/callback` → 200, sets `session` cookie

---

## Test 3: Admin Dashboard (Unauthenticated)

Navigate to **http://localhost:3000/admin** while logged out.

**What happens:**
- You should see a redirect to `/sign-in` (no 500 error)
- This proves the middleware is working

**When signed in as an admin:**
- You should see:
  - User management table
  - System statistics (total users, organizations, etc.)
  - Admin controls

**Code:**
- Admin page: [apps/frontend/src/(admin)/admin/page.tsx](apps/frontend/src/(admin)/admin/page.tsx)
- Admin users component: [components/admin/AdminUserManagement.tsx](components/admin/AdminUserManagement.tsx)

**What happens under the hood:**
1. Component calls: `GET /api/admin/users?limit=10`
2. Frontend proxy (src/app/api/admin/users/route.ts) forwards with `x-internal-service-secret: change-me-internal-service-secret`
3. Auth-service validates the secret (hardcoded for dev)
4. Auth-service executes oRPC procedure: `POST /api/v2/auth/admin/users/list`
5. Returns: `{success: true, users: [...], total: N}`

**Admin Stats display:**
1. Component renders stats fetched from: `GET /api/admin/stats`
2. This calls `POST /api/v2/auth/admin/system/stats` on auth-service
3. If that fails, it falls back to counting users from `/api/v2/auth/admin/users/list`
4. You should see: **Total Users: 2** (Test Bruker, Ima Fernandes Da Costa)

**In DevTools (Network tab):**
- `GET /api/admin/users` → 200 → JSON with user list
- `GET /api/admin/stats` → 200 → JSON with `totalUsers: 2`

---

## Test 4: Chat Page & AI Core

Navigate to **http://localhost:3000/chat**

**What you should see:**
- Chat interface with message history on left (initially empty)
- Input box at bottom: "Send a message..."
- Conversation area saying "Start a new conversation"

**Code:**
- Chat page: [apps/frontend/src/(dashboard)/chat/page.tsx](apps/frontend/src/(dashboard)/chat/page.tsx)
- Chat session routes: [src/app/api/chat/sessions/route.ts](src/app/api/chat/sessions/route.ts)
- Chat stream: [src/app/api/chat/stream/route.ts](src/app/api/chat/stream/route.ts)

**Step 1: Send a message**

Type "Hello" in the input box and press Enter.

**What happens:**
1. Frontend calls: `POST /api/chat/stream`
2. This proxies to: **AI-Core (reasoning-ai-core) at port 8100**
3. Request body goes to: `POST http://ai-core:8000/stream/chat`
4. AI-Core streams response tokens in real-time (Server-Sent Events)
5. Frontend renders streaming text as it arrives
6. Message appears in chat history

**In DevTools (Network → WS or Fetch):**
- You should see `POST /api/chat/stream` as a streaming request
- Response starts flowing immediately (not waiting for full response)
- Text appears word-by-word in the UI

---

## Test 5: Knowledge / Documents Page

Navigate to **http://localhost:3000/knowledge**

**What you should see:**
- Tab 1: **Documents** — empty list (no documents uploaded yet)
- Tab 2: **Sources** — empty list
- Upload button to add documents

**Code:**
- Knowledge page: [apps/frontend/src/(dashboard)/knowledge/page.tsx](apps/frontend/src/(dashboard)/knowledge/page.tsx)
- Docs API route: [src/app/api/docs/[...path]/route.ts](src/app/api/docs/[...path]/route.ts)

**Step 1: Test Document Listing**

When the page loads, it calls:
1. `GET /api/docs/documents?org_id=<user_org>&limit=10`
2. Frontend proxy forwards query params to: **documents-service:8001/v1/documents**
3. Response: `{documents: [], total: 0}` (initially empty)

**Step 2: Upload a Document (Optional)**

If you have files:
1. Click "Upload" button
2. Select a file
3. Frontend calls: `POST /api/ingestion/ingest-job`
4. Ingestion service processes file
5. Solution gets indexed in documents-service
6. Full workflow: File → Imports-API → Retrieval-Service → Vector DB → Documents-Service

**In DevTools:**
- You should see `GET /api/docs/documents` → 200 with `total: 0`
- No errors or 404s

---

## Test 6: Search Feature

Navigate to **http://localhost:3000/search**

**What you should see:**
- Search input box
- Empty results area (no documents indexed yet)

**Code:**
- Search page: [apps/frontend/src/(dashboard)/search/page.tsx](apps/frontend/src/(dashboard)/search/page.tsx)
- AI Search route: [src/app/api/ai/search/route.ts](src/app/api/ai/search/route.ts)

**Step 1: Type a search query**

Type any question (e.g., "What is the system architecture?")

**What happens:**
1. Frontend calls: `GET /api/ai/search?q=<query>&org_id=<org>`
2. This proxies to multiple services:
   - **Retrieval-service**: `/v1/retrieve` — semantic vector search
   - **Reasoning-core**: `/api/v1/reason` — chains information together
3. Results stream back as: `{results: [...], reasoning: "..."}`

**In DevTools:**
- `GET /api/ai/search` → 200
- Response time: ~5-15 seconds (AI reasoning takes time)

---

## Test 7: Admin User Management (Create/Edit)

Navigate to **http://localhost:3000/admin/users**

**What you should see:**
- Table of users with columns: Email, Role, Status, Last Login, Created At
- Action buttons: View, Edit, Delete
- "Create New User" button

**Code:**
- Admin users page: [apps/frontend/src/(admin)/admin/users/page.tsx](apps/frontend/src/(admin)/admin/users/page.tsx)
- Admin users API: [src/app/api/admin/users/route.ts](src/app/api/admin/users/route.ts) + [[id]/route.ts](src/app/api/admin/users/[id]/route.ts)

**Step 1: Click "Create New User"**

A form opens to create a new user.

**What happens:**
1. You fill: Email, Name, Password, Role
2. Click Submit
3. Frontend calls: `POST /api/admin/users`
4. Request body: `{email, name, password, role}`
5. Proxy forwards to: `POST auth-service:3011/api/v2/auth/admin/users/create`
6. Auth-service validates & creates user in database
7. Returns: `{success: true, user: {...}}`
8. New user appears in table

**In DevTools:**
- `POST /api/admin/users` → 201 Created
- New row appears in table without page reload

**Step 2: Click Edit on a user**

Modal opens with user details.

**What happens:**
1. Frontend calls: `GET /api/admin/users/USER_ID`
2. Auth-service returns user details
3. Modal shows editable fields: Name, Role, Status
4. On Save: `PUT /api/admin/users/USER_ID`
5. Auth-service updates the user

**In DevTools:**
- `GET /api/admin/users/USER_ID` → 200
- `PUT /api/admin/users/USER_ID` → 200
- Row updates in real-time

---

## Test 8: OAuth Setup / Connectors (Stub)

Navigate to **http://localhost:3000/admin/organizations**

Look for **OAuth/Integration Settings** section.

**What you should see:**
- UI to register OAuth clients
- Fields: App Name, Redirect URLs, etc.

**Code:**
- OAuth2 proxy: [src/app/api/oauth2/[...path]/route.ts](src/app/api/oauth2/[...path]/route.ts)

**Important:** The `/api/connectors` route was removed (no backing service), so don't expect integration connectors to work yet.

**What happens when you register an app:**
1. Frontend calls: `POST /api/oauth2/clients`
2. Request: `{name, redirectUris: [...]}`
3. Proxy forwards to: `POST auth-service:3011/oauth2/clients`
4. Returns: `{client_id, client_secret, ...}`
5. You use these credentials to set up your OAuth provider

---

## Test 9: Reasoning Feature

Navigate to any page and trigger reasoning (via chat or search).

**Example:**
- In Chat: send a complex question "Explain the system architecture"
- In Search: ask a research question

**What happens:**
1. Frontend calls: `POST /api/reasoning/reason`
2. Reasoning-core executes chain-of-thought:
   - **Step 1:** Decompose query into sub-questions
   - **Step 2-N:** Answer each sub-question
   - **Final:** Synthesize into coherent answer
3. Returns: `{answer: "...", reasoning_trace: [{step, thought, result, confidence}, ...], confidence: 0.85}`

**In DevTools (Network → Fetch):**
- `POST /api/reasoning/reason` → takes ~8-10 seconds
- Response includes full reasoning trace with confidence scores
- You can see each step the AI took

**In the UI:**
- You should see the answer displayed
- In future, admin panel could show the reasoning trace

---

## Test 10: Env Vars & Service Discovery

The frontend `.env` file controls all service URLs.

**Current values:**
```dotenv
AI_CORE_URL=http://ai-core:8000              # Used by routing rules
REASONING_CORE_URL=http://reasoning-core:8000
DOCS_SERVICE_URL=http://documents-service:8001
RETRIEVAL_SERVICE_URL=http://retrieval-service:8004
QUARRY_URL=http://quarry-api:8090
AUTH_SERVICE_URL=http://auth-service:3011
INTERNAL_SERVICE_SECRET=change-me-internal-service-secret
```

**How they work:**
- Each API route file in `/src/app/api/*` reads from `process.env`
- Routes dynamically proxy to the correct service
- If a URL is wrong, requests will hang or fail with 500

**Example in code** ([src/app/api/reasoning/[...path]/route.ts](src/app/api/reasoning/[...path]/route.ts)):
```typescript
const REASONING_CORE_URL = process.env.REASONING_CORE_URL || 'http://localhost:8101'
// Routes POST /api/reasoning/* → {REASONING_CORE_URL}/api/v1/{path}
```

**To verify routes are correct:**
- Every API call in DevTools should show correct proxying
- No "Service Unavailable" errors (unless service is actually down)

---

## Checklist: What Should Work

| Feature | Status | How to Test |
|---|---|---|
| **Navigation** | ✅ | Sidebar menu works, no 404s |
| **Authentication** | ✅ | OAuth sign-in completes, session stored |
| **Admin Dashboard** | ✅ | See stats: totalUsers: 2 |
| **Admin User List** | ✅ | See "Test Bruker" and "Ima Fernandes Da Costa" |
| **Admin Create User** | ✅ | Can create new user via form |
| **Chat with AI** | ✅ | Send message, get streaming response |
| **Search** | ⚠️ | Works if documents indexed; empty if not |
| **Reasoning** | ✅ | Ask question, see reasoning trace |
| **Documents List** | ✅ | Empty list (no uploads yet) |
| **OAuth Setup** | ✅ | Can register OAuth clients |
| **Connectors** | ❌ | Removed (no backing service) |
| **Notes** | ❌ | Removed (no backing service) |

---

## Debugging: How to Use DevTools

**Open DevTools:** F12 (or Cmd+Option+I on Mac)

### Network Tab
1. Click **Network**
2. Reload page or perform action
3. Look for HTTP requests:
   - `POST /api/admin/users` should show 200
   - `GET /api/docs/documents` should show 200
   - If you see **Red X**, service is unreachable

### Console Tab
1. Click **Console**
2. Look for JavaScript errors (red messages)
3. Check for warnings about missing env vars
4. If you see "Uncaught TypeError", something broke in the route logic

### Application Tab (Cookies & Storage)
1. Click **Application**
2. Look for cookies named `session` or `auth-token`
3. This proves authentication worked
4. Session should contain user ID and email

---

## Performance Expectations

| Operation | Expected Time |
|---|---|
| Page load | < 1 second |
| Chat message | 10-30 seconds (LLM inference) |
| Reasoning | 8-15 seconds (chain-of-thought) |
| Search | 5-20 seconds (if documents indexed) |
| Admin user list | < 500ms |
| OAuth sign-in | 1-3 seconds (+ external OAuth service) |

If any operation takes > 2x these times, check:
- Docker container logs: `docker logs <container-name>`
- Network latency: `docker stats` (check CPU/memory)
- Service health: curl `http://localhost:PORT/health`

---

## Troubleshooting

### "Cannot GET /api/***"
**Problem:** Route file doesn't exist.
**Solution:** Check that the route file exists in `/src/app/api/**/route.ts`.

### "Service Unavailable" or "Connect ECONNREFUSED"
**Problem:** Backend service is down.
**Solution:**
```bash
docker ps | grep <service-name>
docker logs <container-name> | tail -20
```

### Chat returns empty response
**Problem:** AI Core isn't returning tokens.
**Solution:**
```bash
curl -s http://localhost:8100/health   # Check AI Core is up
docker logs reasoning-ai-core | tail -20  # Check for errors
```

### Admin endpoints return 401
**Problem:** `INTERNAL_SERVICE_SECRET` is wrong.
**Solution:** Check `.env`:
```bash
grep INTERNAL_SERVICE_SECRET /Volumes/Lagring/Triodelab/CoreSystem/apps/frontend/.env
# Should be: change-me-internal-service-secret
```

### OAuth sign-in fails
**Problem:** Auth service not responding.
**Solution:**
```bash
curl -s http://localhost:3011/api/v2/auth/oauth-providers | head
# Should return JSON with provider list (GitHub, Google, etc.)
```
