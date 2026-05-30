# Linear Setup - Quick Commands

**Time**: 30 minutes | **Cost**: Free (<10 users)

---

## 🚀 Quick Setup (Copy-Paste)

### 1. Create Linear Account (5 min)

Visit: https://linear.app/signup

- Sign up with work email
- Workspace name: **Triodelab** or **Aquatiq**
- Skip team creation for now

### 2. Create Team via Web UI (5 min)

1. Click **"Create Team"** button
2. Fill in:
   - Name: `Content Safety`
   - Key: `CS` (for ticket IDs)
   - Description: `AI content moderation and safety reviews`
   - Privacy: `Private`
3. Click **Create**

### 3. Get API Key (Browser)

1. Profile → Settings → API
2. Click **"Create new API key"**
3. Name: `AI-Core Integration`
4. Copy key (starts with `lin_api_`)
5. Save securely!

### 4. Get Team ID (Terminal)

```bash
# Set your API key
export LINEAR_API_KEY="lin_api_YOUR_KEY_HERE"

# Query for teams
curl -X POST https://api.linear.app/graphql \
  -H "Authorization: $LINEAR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "{ teams { nodes { id name key } } }"
  }' | jq '.data.teams.nodes[] | select(.name == "Content Safety")'

# Save the "id" value (UUID format)
```

### 5. Configure AI-Core

```bash
# Navigate to ai-core
cd /Volumes/Lagring/Triodelab/CoreSystem/backend/ai-core

# Update .env file (replace YOUR_KEY and YOUR_TEAM_ID)
cat > .env.linear << 'EOF'
LINEAR_API_KEY=lin_api_YOUR_KEY_HERE
LINEAR_TEAM_ID=YOUR_TEAM_UUID_HERE
LINEAR_WEBHOOK_SECRET=$(openssl rand -hex 32)
LINEAR_ENABLED=true
EOF

# Merge into .env
cat .env.linear >> .env
rm .env.linear
```

### 6. Deploy AI-Core

```bash
cd /Volumes/Lagring/Triodelab/CoreSystem

# Rebuild and deploy
docker compose -f backend/docker-compose.yml up -d --build ai-core

# Wait for startup (30 seconds)
sleep 30

# Check health
curl http://localhost:8040/health | jq '.'
```

### 7. Test Linear Integration

```bash
# Generate test org ID
export ORG_ID=$(uuidgen)

# Create safety violation that triggers Linear ticket
curl -X POST http://localhost:8040/api/v1/safety/moderate \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "text": "This content contains hate speech and needs immediate review",
    "context": {
      "user_id": "test-user-123",
      "content_type": "chat_message",
      "severity": "high"
    },
    "create_review_ticket": true
  }' | jq '.review_ticket'
```

**Expected Output:**
```json
{
  "id": "uuid-here",
  "identifier": "CS-1",
  "url": "https://linear.app/triodelab/issue/CS-1",
  "title": "Safety Review: High Severity",
  "status": "pending",
  "priority": 2
}
```

### 8. Verify in Linear

Open the URL from the response → You should see ticket CS-1! 🎉

---

## 🔍 Troubleshooting

### Can't find Team ID?

Try GraphQL Playground:
1. Go to https://linear.app/YOUR_WORKSPACE/settings/api
2. Click **"API Playground"**
3. Run:
   ```graphql
   {
     teams {
       nodes {
         id
         name
         key
       }
     }
   }
   ```

### API Key Not Working?

```bash
# Test key validity
curl -X POST https://api.linear.app/graphql \
  -H "Authorization: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"query":"{ viewer { id name email } }"}' \
  | jq '.'

# Should return your user info, not errors
```

### Service Not Starting?

```bash
# Check logs
docker logs ai-core-service --tail 50

# Look for:
# - LINEAR_API_KEY loaded
# - LINEAR_TEAM_ID configured
# - No connection errors to api.linear.app
```

---

## 📊 Verify Deployment

```bash
# 1. Check service health
curl http://localhost:8040/health

# 2. Check Linear connectivity
docker logs ai-core-service | grep -i linear

# 3. Test ticket creation
curl -X POST http://localhost:8040/api/v1/safety/moderate \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $(uuidgen)" \
  -d '{"text":"test","create_review_ticket":true}'

# 4. Check Linear web UI for ticket
open https://linear.app/triodelab
```

---

## 🎯 Success Criteria

- ✅ Linear account created
- ✅ Content Safety team exists
- ✅ API key obtained
- ✅ Team ID found
- ✅ AI-core configured
- ✅ AI-core deployed
- ✅ Test ticket created
- ✅ Ticket visible in Linear UI

---

**Time to complete**: 30 minutes  
**Cost**: $0 (Free tier)  
**Savings**: $55,000 over 2 years vs custom build

🚀 **Ready to deploy!**
