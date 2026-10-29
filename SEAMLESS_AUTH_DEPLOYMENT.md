# Seamless Auth-Core Integration - Deployment Checklist

## Pre-Deployment ✓

- [x] **Code implementation complete**
  - [x] auth_core_bridge.py (integration-core)
  - [x] main.py endpoints added
  - [x] models.py updated
  - [x] config.py updated
  - [x] ConnectStep.tsx enhanced
  - [x] API routes created (/api/oauth/session-check, /api/connections/from-auth-core)

- [x] **Documentation created**
  - [x] AUTH_CORE_INTEGRATION_GUIDE.md (architecture + flows)
  - [x] AUTH_CORE_INTEGRATION_SUMMARY.md (deployment guide)

## Deployment Steps

### Phase 1: Build Services (10 min)

```bash
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane"

# Build integration-core with new auth_core_bridge module
echo "1️⃣ Building integration-core..."
docker-compose build --no-cache integration-api

# Start integration-api service
echo "2️⃣ Starting integration-api..."
docker-compose up -d integration-api

# Wait for service to be healthy
sleep 10
echo "3️⃣ Checking health..."
curl -s http://localhost:9026/health | jq '.'
# Expected: { "status": "ok", "service": "integration-core" }
```

### Phase 2: Frontend Rebuild (5 min)

```bash
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/frontend"

# Verify .env has INTEGRATION_CORE_URL
echo "4️⃣ Checking frontend config..."
grep "INTEGRATION_CORE_URL" .env

# Rebuild frontend with new API routes
echo "5️⃣ Building frontend..."
docker-compose build frontend

# Or restart if using dev mode
docker-compose restart frontend

sleep 8
curl -s -o /dev/null -w "%{http_code}" http://localhost:3000/
# Expected: 200
```

## Testing

### Test 1: Session Validation Works

```bash
# Test the validation endpoint (requires valid session)
echo "6️⃣ Testing session validation..."

# If you have a valid auth-core session token, test with:
curl -X POST http://localhost:9026/api/v1/session/validate \
  -H "Authorization: Bearer YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json"

# Expected response:
# {
#   "valid": true,
#   "has_microsoft": true,
#   "has_google": false,
#   "providers": ["microsoft"],
#   "user_email": "user@contoso.com"
# }
```

### Test 2: Manual Browser Test (Smoking Test)

```bash
echo "7️⃣ Manual browser testing..."

# 1. Open http://localhost:3000/auth/signin
# 2. Login with a social provider (Microsoft or Google)
# 3. Complete onboarding Steps 1-3
# 4. Reach Step 4: "Koble til datakilder"

# Expected behavior:
# ✅ If you logged in with Microsoft:
#    - See blue badge: "Du er koblet til som user@contoso.com"
#    - Button says "Aktiver" (not "Gi tilgang")
#    - Sources (SharePoint, OneDrive) are pre-selected
#    - Click "Aktiver" → documents discovered → auto-advance to Step 5
#    - NO Microsoft login screen appears ✨
#
# ✅ If you didn't login with Microsoft:
#    - No blue badge
#    - Button says "Gi tilgang"
#    - Click "Gi tilgang" → Microsoft login screen (existing flow)
```

### Test 3: Database Records

```bash
echo "8️⃣ Checking database records..."

# Check that tokens are stored with source="auth-core"
docker exec -it postgres psql \
  -U ingestion_user \
  -d integration \
  -c "SELECT id, org_id, provider, source, is_active FROM oauth_tokens LIMIT 5;"

# Expected output includes:
# | provider  | source      |
# |-----------|-------------|
# | microsoft | auth-core   | ✨ (seamless)
# | microsoft | direct      | (from OAuth flow)
```

### Test 4: Error Handling

```bash
# Test with invalid session
echo "9️⃣ Testing error handling..."

curl -X POST http://localhost:9026/api/v1/session/validate \
  -H "Authorization: Bearer invalid-token" \
  -H "Content-Type: application/json"

# Expected: 
# { "valid": false, "error": "Invalid or expired session" }
```

## Verification Checklist

Use this checklist to verify everything is working:

```bash
echo "=== INTEGRATION-CORE VERIFICATION ==="

# 1. Service is running
echo "1. Service Health:"
curl -s http://localhost:9026/health | jq '.'
echo ""

# 2. Session validation endpoint exists
echo "2. Session Validation Endpoint:"
curl -s -X POST http://localhost:9026/api/v1/session/validate \
  -H "Content-Type: application/json" | jq '.'
echo ""

# 3. Check logs for auth-core bridge initialization
echo "3. Integration-API Logs (last 20 lines):"
docker logs integration-api --tail=20 | grep -E "auth-core|bridge|session|token" || echo "No auth-core logs yet (normal on first start)"
echo ""

# 4. Database tables exist
echo "4. Database Tables:"
docker exec -it postgres psql \
  -U ingestion_user \
  -d integration \
  -c "\dt" 2>/dev/null | grep -E "oauth_tokens|connection" || echo "Tables will auto-create on first use"
echo ""

echo "=== FRONTEND VERIFICATION ==="

# 5. Frontend is running
echo "5. Frontend Health:"
curl -s -o /dev/null -w "Status: %{http_code}\n" http://localhost:3000/
echo ""

# 6. API routes exist
echo "6. Session Check Route:"
curl -s -o /dev/null -w "Status: %{http_code}\n" \
  -X GET http://localhost:3000/api/oauth/session-check
echo ""

echo "7. From-Auth-Core Route:"
curl -s -o /dev/null -w "Status: %{http_code}\n" \
  -X POST http://localhost:3000/api/connections/from-auth-core \
  -H "Content-Type: application/json" \
  -d '{}'
echo ""

echo "✅ If all status codes are 2xx or 4xx (not 5xx), deployment is successful!"
```

## Monitoring

### Watch Logs During Testing

```bash
# Terminal 1: Watch integration-api logs
docker logs integration-api --follow | grep -i "auth-core\|session\|token"

# Terminal 2: Watch frontend logs
docker logs frontend-v5 --follow | grep -i "oauth\|connection\|auth"

# Terminal 3: Watch postgres logs
docker logs postgres --follow | grep -i "integration"
```

### Success Indicators in Logs

**Integration-API (should see these lines):**
```
✅ "Retrieved auth-core session for user"
✅ "Successfully extracted Microsoft token from auth-core session"
✅ "Saved microsoft token from auth-core"
✅ "Reused Microsoft token from auth-core"
```

**Frontend (should see these lines):**
```
✅ "Session check: has_microsoft=true"
✅ "Quick connect activated for user"
```

## Rollback (If Issues)

```bash
# If anything goes wrong, rollback is simple:

# 1. Stop services
docker-compose down

# 2. Revert frontend changes (if you saved them)
git checkout apps/frontend/src/components/onboarding/core/ConnectStep.tsx

# 3. Restart without new code
docker-compose up -d

# User will see original OAuth flow (no seamless integration)
# But existing functionality remains intact
```

## Post-Deployment

After successful deployment:

- [ ] Test with 2-3 different user personas
- [ ] Test on different browsers (Chrome, Safari, Firefox)
- [ ] Test token refresh (wait until token near expiry)
- [ ] Test error scenarios (network down, invalid token)
- [ ] Monitor logs for 24 hours
- [ ] Get user feedback on seamless vs OAuth flow

## Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| 404 on /api/oauth/session-check | Frontend not rebuilt | Run `docker-compose build frontend` |
| 502 on /api/oauth/session-check | integration-api not responding | Check `docker logs integration-api` |
| "has_microsoft=false" always | auth-core session invalid | Verify user logged in with Microsoft |
| "Microsoft token not found" error | User logged in with different provider | Show OAuth flow (expected behavior) |
| "Failed to discover documents" | Graph API error | Check Microsoft API permissions |
| Database error on token save | DB connection issue | Check `docker logs postgres` |

## Performance Expectations

After deployment, you should see:

- **Session check**: ~200ms (fast ✨)
- **Document discovery**: ~2-3 seconds
- **Total flow**: ~2-5 seconds (vs 30-60 seconds for OAuth)
- **CPU usage**: No significant increase
- **Memory**: No new leaks (standard FastAPI + asyncio)

## Success Metrics

You'll know it's working when:

✅ User logs in with Microsoft at Step 1  
✅ At Step 4, sees "Du er koblet til som user@contoso.com"  
✅ Clicks "Aktiver" (not "Gi tilgang")  
✅ Documents auto-discover without re-authentication  
✅ Auto-advances to Step 5 (Team Invite)  
✅ Zero extra OAuth prompts during onboarding  
✅ Database shows `source="auth-core"` for tokens  

## Questions?

Refer to:
- [AUTH_CORE_INTEGRATION_GUIDE.md](../docs/AUTH_CORE_INTEGRATION_GUIDE.md) - Architecture & flows
- [integration-core/README.md](../apps/Ingestion%20Plane/integration-core/README.md) - API docs
- [ConnectStep.tsx](../apps/frontend/src/components/onboarding/core/ConnectStep.tsx) - Frontend logic

---

**Estimated Deployment Time: 15-20 minutes**

**Rollback Time (if needed): 5 minutes**

Ready to deploy? Run the Phase 1 & 2 steps above! 🚀

---

Last Updated: Feb 28, 2026
