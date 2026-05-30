# Phase 2 Quick Reference Guide

**Last Updated**: February 1, 2026

Quick reference for using Phase 2 Priority 1 features in production.

---

## 🚦 Rate Limiting

### Check Rate Limit Status
```bash
# Get rate limit stats for an organization
curl http://localhost:8080/api/v1/admin/ratelimit/{org_id}/stats

# Example response:
{
  "org_id": "550e8400-e29b-41d4-a716-446655440000",
  "stats": {
    "chat": {
      "last_refill": "2026-02-01T16:19:12Z",
      "remaining": 169
    },
    "rag:query": {
      "last_refill": "2026-02-01T16:18:00Z",
      "remaining": 45
    }
  }
}
```

### Reset Rate Limit
```bash
# Reset specific operation
curl -X POST http://localhost:8080/api/v1/admin/ratelimit/{org_id}/reset \
  -H "Content-Type: application/json" \
  -d '{"operation": "chat"}'

# Reset all operations for an org
curl -X POST http://localhost:8080/api/v1/admin/ratelimit/reset-all \
  -H "Content-Type: application/json" \
  -d '{"org_id": "550e8400-e29b-41d4-a716-446655440000"}'
```

### Check Configuration
```bash
# Get current rate limit config
curl http://localhost:8080/api/v1/admin/ratelimit/config

# Example response:
{
  "enabled": true,
  "limits": {
    "chat": 120,
    "rag:query": 60,
    "rag:index": 30,
    "crawl": 10
  },
  "burst_multiplier": 1.5
}
```

### Response Headers
All API responses include rate limit headers:
```
X-Ratelimit-Limit: 120
X-Ratelimit-Remaining: 178
Retry-After: 30  (if rate limited)
```

### Default Limits (per minute)
- **Chat**: 120 requests
- **RAG Query**: 60 requests  
- **RAG Index**: 30 requests
- **Documents**: 90 requests
- **Embeddings**: 180 requests
- **Crawl**: 10 requests
- **Burst**: 1.5x limit

---

## 📝 Prompt Templates

### List All Templates
```bash
curl http://localhost:8040/api/v1/templates

# Returns all templates with metadata
```

### Get Specific Template
```bash
curl http://localhost:8040/api/v1/templates/chat_default
```

### Render Template
```bash
curl -X POST http://localhost:8040/api/v1/templates/render \
  -H "Content-Type: application/json" \
  -d '{
    "name": "chat_default",
    "variables": {
      "query": "What is machine learning?"
    }
  }'

# Response:
{
  "rendered": "You are a helpful AI assistant. Answer the following question:\n\nWhat is machine learning?",
  "template_name": "chat_default",
  "version": 1
}
```

### Default Templates

#### 1. chat_default
**Category**: chat  
**Variables**: `query`
```jinja2
You are a helpful AI assistant. Answer the following question:

{{query}}
```

#### 2. chat_with_context
**Category**: chat  
**Variables**: `query`, `context`
```jinja2
You are a helpful AI assistant. Use the following context to answer the question.

Context:
{{context}}

Question: {{query}}

Answer:
```

#### 3. reasoning_detailed
**Category**: reasoning  
**Variables**: `problem`
```jinja2
Think through this problem step by step:

Problem: {{problem}}

Provide your reasoning:
```

#### 4. summarize
**Category**: summarization  
**Variables**: `text`, `length`
```jinja2
Summarize the following text in {{length}} or less:

{{text}}

Summary:
```

#### 5. extract_entities
**Category**: extraction  
**Variables**: `text`, `schema`
```jinja2
Extract the following information from the text:
{{schema}}

Text:
{{text}}

Extracted information (JSON):
```

#### 6. code_review
**Category**: code  
**Variables**: `code`, `language`
```jinja2
Review the following code and provide feedback:

Language: {{language}}

Code:
```{{language}}
{{code}}
```

Review:
```

### Create Custom Template
```bash
curl -X POST http://localhost:8040/api/v1/templates \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my_custom_template",
    "category": "custom",
    "description": "My custom template",
    "template": "Custom prompt: {{variable1}}",
    "variables": {
      "variable1": "Description of variable"
    },
    "metadata": {
      "max_tokens": 2000
    }
  }'
```

### Update Template (creates new version)
```bash
curl -X PATCH http://localhost:8040/api/v1/templates/my_custom_template \
  -H "Content-Type: application/json" \
  -d '{
    "template": "Updated prompt: {{variable1}}"
  }'
```

### Delete Template
```bash
curl -X DELETE http://localhost:8040/api/v1/templates/my_custom_template
```

---

## 🔍 Audit Logging

### Query Audit Logs
```bash
# Get recent logs (last 7 days, limit 10)
curl http://localhost:8080/api/v1/admin/audit/logs?limit=10

# Filter by organization
curl "http://localhost:8080/api/v1/admin/audit/logs?org_id=550e8400-e29b-41d4-a716-446655440000&limit=20"

# Filter by action
curl "http://localhost:8080/api/v1/admin/audit/logs?action=query&limit=20"

# Filter by resource type
curl "http://localhost:8080/api/v1/admin/audit/logs?resource_type=rag_query&limit=20"

# Date range filter
curl "http://localhost:8080/api/v1/admin/audit/logs?start_time=2026-01-01T00:00:00Z&end_time=2026-02-01T00:00:00Z"
```

### Export Audit Logs (CSV)
```bash
curl http://localhost:8080/api/v1/admin/audit/logs/export \
  -o audit_logs.csv
```

### Audit Log Statistics
```bash
curl http://localhost:8080/api/v1/admin/audit/stats

# Example response:
{
  "total_logs": 1543,
  "logs_by_action": {
    "query": 1204,
    "create": 256,
    "update": 67,
    "delete": 16
  },
  "logs_by_resource": {
    "rag_query": 804,
    "chat": 487,
    "document": 252
  },
  "logs_by_status": {
    "200": 1425,
    "404": 87,
    "429": 31
  }
}
```

### Log Entry Format
```json
{
  "id": "ede96794-76ff-47a3-8aac-9478aed8c004",
  "org_id": "550e8400-e29b-41d4-a716-446655440000",
  "user_id": "123e4567-e89b-12d3-a456-426614174000",
  "action": "query",
  "resource_type": "rag_query",
  "resource_id": "789e0123-e45b-67c8-d901-234567890abc",
  "trace_id": "abc123def456",
  "request_id": "b4abe243-06a1-48a1-a150-fc2f2c15a1ee",
  "ip_address": "172.217.19.241",
  "user_agent": "curl/8.7.1",
  "details": {
    "method": "GET",
    "path": "/api/v1/rag/query",
    "status": 200,
    "duration_ms": 145,
    "request_body": {"query": "test"},
    "error": null
  },
  "created_at": "2026-02-01T16:19:12.729563Z"
}
```

### Direct Database Query
```sql
-- Get recent logs for an organization
SELECT 
  id, action, resource_type, 
  details->>'status' as status,
  details->>'duration_ms' as duration_ms,
  created_at
FROM audit_logs
WHERE org_id = '550e8400-e29b-41d4-a716-446655440000'
  AND created_at > NOW() - INTERVAL '1 day'
ORDER BY created_at DESC
LIMIT 20;

-- Count requests by status code
SELECT 
  (details->>'status')::int as status_code,
  COUNT(*) as count
FROM audit_logs
WHERE created_at > NOW() - INTERVAL '1 day'
GROUP BY status_code
ORDER BY count DESC;

-- Average response time by endpoint
SELECT 
  details->>'path' as endpoint,
  COUNT(*) as requests,
  AVG((details->>'duration_ms')::int) as avg_ms,
  MAX((details->>'duration_ms')::int) as max_ms
FROM audit_logs
WHERE created_at > NOW() - INTERVAL '1 day'
  AND details->>'duration_ms' IS NOT NULL
GROUP BY endpoint
ORDER BY avg_ms DESC;
```

---

## 🧪 Testing

### Test Organization
For testing purposes, use this pre-created organization:

**Name**: Ima Admin Org  
**UUID**: `550e8400-e29b-41d4-a716-446655440000`  
**Plan**: Enterprise  
**Database**: `aquatiq_dev`

### Test with Headers
```bash
# Include X-Org-ID header for testing
curl -H "X-Org-ID: 550e8400-e29b-41d4-a716-446655440000" \
     http://localhost:8080/api/v1/admin/ratelimit/stats

# Include both X-Org-ID and X-User-ID
curl -H "X-Org-ID: 550e8400-e29b-41d4-a716-446655440000" \
     -H "X-User-ID: 123e4567-e89b-12d3-a456-426614174000" \
     http://localhost:8080/health
```

### Run Full Test Suite
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/backend
bash test_phase2_manual.sh
```

---

## ⚙️ Configuration

### Environment Variables

**Org Core** (`.env.local`):
```env
# Rate Limiting
RATE_LIMIT_ENABLED=true
RATE_LIMIT_RAG_QUERY_RPM=60
RATE_LIMIT_RAG_INDEX_RPM=30
RATE_LIMIT_CHAT_RPM=120
RATE_LIMIT_DOCUMENT_RPM=90
RATE_LIMIT_EMBEDDING_RPM=180
RATE_LIMIT_CRAWL_RPM=10
RATE_LIMIT_DEFAULT_RPM=60
RATE_LIMIT_BURST_MULTIPLIER=1.5

# Audit Logging
AUDIT_LOG_ENABLED=true

# Database
POSTGRES_DSN=postgres://postgres:postgres@aquatiq-postgres-local:5432/aquatiq_dev?sslmode=disable

# Redis
REDIS_URL=redis://:redis@aquatiq-redis-local:6379/0
```

**AI Core** (docker-compose.yml):
```yaml
environment:
  - RATE_LIMIT_ENABLED=true
  - RATE_LIMIT_CHAT_RPM=120
  - RATE_LIMIT_EMBEDDING_RPM=180
  - RATE_LIMIT_DOCUMENT_RPM=90
  - RATE_LIMIT_SAFETY_RPM=200
  - RATE_LIMIT_TRANSLATE_RPM=100
  - RATE_LIMIT_IMAGE_GEN_RPM=30
  - DATABASE_URL=postgresql://postgres:postgres@aquatiq-postgres-local:5432/ai_core
  - REDIS_URL=redis://:redis@aquatiq-redis-local:6379/4
```

---

## 🔧 Troubleshooting

### Rate Limiting Issues

**Problem**: Rate limit headers not appearing  
**Solution**: Ensure `RATE_LIMIT_ENABLED=true` and Redis is running
```bash
docker ps | grep redis
docker exec aquatiq-redis-local redis-cli ping
```

**Problem**: Rate limits not enforcing  
**Solution**: Check Redis Lua script execution
```bash
docker logs org-core-service | grep "rate limit"
```

### Audit Logging Issues

**Problem**: Logs not being written  
**Solution**: Verify test org exists in correct database
```sql
-- Connect to correct database
docker exec aquatiq-postgres-local psql -U postgres -d aquatiq_dev

-- Check if org exists
SELECT org_id, name FROM organizations 
WHERE org_id = '550e8400-e29b-41d4-a716-446655440000';
```

**Problem**: FK constraint violation  
**Solution**: Ensure organization exists before making requests
```bash
docker logs org-core-service | grep "Failed to write audit log"
```

### Template Issues

**Problem**: Template rendering fails  
**Solution**: Check variable names match template
```bash
# List template variables
curl http://localhost:8040/api/v1/templates/chat_default | jq '.variables'
```

**Problem**: Template not found  
**Solution**: Verify template exists in database
```sql
SELECT name, category, is_active FROM prompt_templates;
```

---

## 📊 Monitoring

### Key Metrics to Watch

**Rate Limiting**:
- `ratelimit_hits_total` - Total rate limit hits
- `ratelimit_remaining_avg` - Average remaining tokens
- `ratelimit_redis_errors_total` - Redis connection errors

**Audit Logging**:
- `audit_logs_written_total` - Total logs written
- `audit_log_write_duration_ms` - Write latency
- `audit_log_errors_total` - Write failures

**Templates**:
- `template_render_total` - Total renders
- `template_render_duration_ms` - Render latency
- `template_cache_hits_total` - Template cache hits

### Health Check Endpoints
```bash
# Overall health
curl http://localhost:8080/health

# AI Core health
curl http://localhost:8040/health

# Metrics (Prometheus format)
curl http://localhost:9091/metrics
```

---

## 📚 Additional Resources

- [PHASE_2_COMPLETE.md](./PHASE_2_COMPLETE.md) - Full completion report
- [PHASE_2_PRIORITY_1_COMPLETE.md](./PHASE_2_PRIORITY_1_COMPLETE.md) - Detailed implementation docs
- [test_phase2_manual.sh](./test_phase2_manual.sh) - Test suite
- [PHASE_2_PRIORITIES.md](./PHASE_2_PRIORITIES.md) - Original requirements

---

**Phase 2 Priority 1: Production Ready** ✅
