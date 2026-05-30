# Phase 2 Priorities (Frontend Last)

**Date**: February 1, 2026  
**Status**: Ready to Execute

## Overview

With Convex Gateway complete, we continue Phase 2 improvements focusing on **production readiness** and **scalability**.

---

## ✅ Completed (Phase 1 + Convex)

1. ✅ Streaming Responses (ai-core)
2. ✅ Model Registry & Failover (ai-core)
3. ✅ WebSocket API (org-core)
4. ✅ Webhook System (org-core)
5. ✅ Qdrant Integration (ai-core)
6. ✅ Session Management (partially in Convex)
7. ✅ Job Queue System (org-core)
8. ✅ **Convex Gateway** (NEW - replaces custom Gateway Service)

---

## 🎯 Priority 1: Critical for Production (Start Now)

### 1. Rate Limiting (3-4 days) 🔥
**Why**: Prevent abuse, ensure fair usage, production requirement

**Implementation**:
- Token bucket algorithm with Redis
- Per-org limits: 60 req/min (RAG), 30 req/min (indexing), 120 req/min (chat)
- Middleware in Org Core + AI Core
- Return `429 Too Many Requests` with `Retry-After` header
- Add `X-RateLimit-Limit` and `X-RateLimit-Remaining` headers

**Files to Create**:
```
backend/Org-core/internal/middleware/ratelimit.go (200 lines)
backend/ai-core/app/middleware/rate_limiter.py (150 lines)
backend/Org-core/internal/ratelimit/limiter.go (180 lines)
```

**Dependencies**:
```bash
# Go
go get github.com/go-redis/redis_rate/v10

# Python
pip install redis-py python-redis-rate-limit
```

**Configuration**:
```env
RATE_LIMIT_ENABLED=true
REDIS_URL=redis://aquatiq-redis-local:6379
RATE_LIMIT_RAG_RPM=60
RATE_LIMIT_CHAT_RPM=120
RATE_LIMIT_INDEX_RPM=30
```

**Testing**:
```bash
# Rapid fire requests to trigger rate limit
for i in {1..70}; do
  curl -X POST http://localhost:8080/api/v1/rag/query \
    -H "X-Org-ID: test-org" \
    -d '{"query": "test"}'
done

# Should get 429 after 60 requests in 1 minute
```

**Success Metrics**:
- Enforces limits within 100ms overhead
- Per-org isolation (one org's limit doesn't affect others)
- Graceful degradation (Redis failure → allow requests)
- Prometheus metrics for rate limit hits

---

### 2. Prompt Template Management (3-5 days) 🔥
**Why**: Centralized prompt control, A/B testing, version management

**Implementation**:
- Database-backed templates with versioning
- Jinja2 templating for dynamic variables
- Template categories: reasoning, summarization, extraction, chat
- Admin UI to create/edit templates (simple CRUD)
- A/B testing support with percentage rollout

**Files to Create**:
```
backend/ai-core/app/services/template_manager.py (250 lines)
backend/ai-core/app/routes/templates.py (180 lines)
backend/ai-core/migrations/004_prompt_templates.sql (50 lines)
backend/ai-core/app/models/template.py (100 lines)
```

**Database Schema**:
```sql
CREATE TABLE prompt_templates (
    id UUID PRIMARY KEY,
    name VARCHAR(100) UNIQUE NOT NULL,
    category VARCHAR(50) NOT NULL, -- chat, reasoning, summarization
    template TEXT NOT NULL,
    version INTEGER NOT NULL,
    is_active BOOLEAN DEFAULT true,
    metadata JSONB,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

CREATE TABLE template_versions (
    id UUID PRIMARY KEY,
    template_id UUID REFERENCES prompt_templates(id),
    version INTEGER NOT NULL,
    template TEXT NOT NULL,
    created_by VARCHAR(100),
    created_at TIMESTAMP
);
```

**Usage Example**:
```python
# app/services/template_manager.py
from jinja2 import Template

class TemplateManager:
    async def render(self, name: str, variables: dict) -> str:
        template = await self.get_template(name)
        return Template(template.content).render(**variables)

# Usage
prompt = await template_mgr.render("chat_with_context", {
    "query": user_query,
    "context": rag_context,
    "thinking_level": "detailed"
})
```

**API Endpoints**:
```
POST /api/v1/templates - Create new template
GET /api/v1/templates - List all templates
GET /api/v1/templates/:name - Get template by name
PUT /api/v1/templates/:name - Update template (creates new version)
DELETE /api/v1/templates/:name - Deactivate template
POST /api/v1/templates/:name/test - Test render with sample variables
```

**Success Metrics**:
- < 10ms template rendering
- Support 100+ templates
- Version rollback in < 5 seconds
- A/B test 2+ prompt variants simultaneously

---

### 3. Audit Logging (4-5 days) 🟡
**Why**: Compliance, debugging, user activity tracking

**Implementation**:
- Structured logging to PostgreSQL
- Log all API requests with org_id, user_id, action, resource
- Separate table from application logs
- Retention policy: 90 days (configurable)
- Admin dashboard to view logs

**Files to Create**:
```
backend/Org-core/internal/audit/logger.go (200 lines)
backend/Org-core/internal/audit/handler.go (150 lines)
backend/Org-core/migrations/005_audit_logs.up.sql (40 lines)
```

**Database Schema**:
```sql
CREATE TABLE audit_logs (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    user_id UUID,
    action VARCHAR(100) NOT NULL, -- create, update, delete, query
    resource VARCHAR(100) NOT NULL, -- conversation, document, session
    resource_id UUID,
    ip_address VARCHAR(45),
    user_agent TEXT,
    request_body JSONB,
    response_status INTEGER,
    error_message TEXT,
    timestamp TIMESTAMP NOT NULL,
    duration_ms INTEGER
);

CREATE INDEX idx_audit_org_timestamp ON audit_logs(org_id, timestamp DESC);
CREATE INDEX idx_audit_user ON audit_logs(user_id, timestamp DESC);
CREATE INDEX idx_audit_action ON audit_logs(action, timestamp DESC);
```

**Middleware Integration**:
```go
// internal/middleware/audit.go
func AuditMiddleware() gin.HandlerFunc {
    return func(c *gin.Context) {
        start := time.Now()
        
        // Process request
        c.Next()
        
        // Log to audit table
        auditLog := audit.Log{
            OrgID:      c.GetString("org_id"),
            UserID:     c.GetString("user_id"),
            Action:     c.Request.Method,
            Resource:   c.Request.URL.Path,
            StatusCode: c.Writer.Status(),
            Duration:   time.Since(start).Milliseconds(),
        }
        go auditLogger.Write(c.Request.Context(), auditLog)
    }
}
```

**API Endpoints**:
```
GET /api/v1/audit/logs - List audit logs (paginated)
GET /api/v1/audit/logs/export - Export logs as CSV
GET /api/v1/audit/stats - Aggregate stats (requests/hour, top actions)
```

**Success Metrics**:
- < 5ms logging overhead per request
- Support 10K+ logs/minute
- 99.9% write success rate
- Queryable logs in < 1 second (with indexes)

---

## 🎯 Priority 2: Observability & Reliability (2-3 weeks)

### 4. Structured Logging + Tracing (1 week) 🟡
**Why**: Debugging, performance analysis, distributed tracing

**Implementation**:
- OpenTelemetry integration (Go + Python)
- Trace requests across services (Convex → AI Core → Org Core)
- Export to Jaeger or Tempo
- Structured JSON logs with trace_id

**Files to Create**:
```
backend/Org-core/internal/observability/tracing.go
backend/ai-core/app/middleware/tracing.py
docker-compose.jaeger.yml
```

**Dependencies**:
```bash
# Go
go get go.opentelemetry.io/otel

# Python
pip install opentelemetry-api opentelemetry-sdk opentelemetry-instrumentation-fastapi
```

---

### 5. Health Checks & Monitoring (3-4 days) 🟡
**Why**: Production readiness, uptime monitoring

**Implementation**:
- Comprehensive health endpoints: `/health`, `/ready`, `/live`
- Check DB connections, Redis, external APIs
- Prometheus metrics export
- Grafana dashboards

**Endpoints**:
```
GET /health - Overall health (200 = healthy)
GET /health/ready - Ready to serve traffic
GET /health/live - Liveness probe (for k8s)
GET /metrics - Prometheus metrics
```

**Metrics to Track**:
- Request rate (req/sec)
- Error rate (4xx/5xx)
- P50/P95/P99 latency
- Database connection pool
- Model API latency
- RAG query time
- Job queue length

---

### 6. Backup & Recovery (2-3 days) 🟡
**Why**: Data protection, disaster recovery

**Implementation**:
- Automated PostgreSQL backups (daily)
- Qdrant vector backup
- Backup to S3/R2
- Point-in-time recovery
- Restore scripts

**Files to Create**:
```
scripts/backup-postgres.sh
scripts/backup-qdrant.sh
scripts/restore.sh
```

---

## 🎯 Priority 3: Advanced Features (3-4 weeks)

### 7. Multi-Model Orchestration (1 week)
**Why**: Cost optimization, performance

**Implementation**:
- Router that picks best model for task
- Classification: GPT-3.5
- Reasoning: GPT-4 / Claude
- Code: Claude / Codestral
- Summarization: GPT-3.5

---

### 8. Caching Layer (1 week)
**Why**: Reduce costs, improve speed

**Implementation**:
- Redis cache for LLM responses
- Semantic cache (embed query, match similar)
- TTL: 1 hour for chat, 24 hours for RAG
- Cache hit rate target: > 30%

---

### 9. Document Processing Pipeline (1-2 weeks)
**Why**: Support more file types

**Implementation**:
- PDF, DOCX, PPTX, HTML parsing
- Image OCR with Tesseract
- Table extraction
- Metadata extraction

---

### 10. Advanced RAG (1-2 weeks)
**Why**: Better retrieval quality

**Implementation**:
- Hybrid search (dense + sparse)
- Re-ranking with Cross-Encoder
- Query expansion
- Multi-query retrieval

---

## 🚀 Execution Plan

### Week 1-2 (Current)
- ✅ Convex Gateway (DONE)
- 🔥 **Rate Limiting** (Start immediately)
- 🔥 **Prompt Template Management** (Parallel)

### Week 3-4
- 🟡 Audit Logging
- 🟡 Structured Logging + Tracing
- 🟡 Health Checks & Monitoring

### Week 5-6
- 🟡 Backup & Recovery
- Advanced features (Multi-Model Orchestration, Caching)

### Week 7+
- Document Processing Pipeline
- Advanced RAG improvements
- Frontend integration with Convex

---

## Decision: Gateway Service

**Question**: Do we need a separate Gateway Service for:
1. WebSocket control plane
2. Service orchestration
3. Session coordination
4. Event distribution via NATS

**Answer**: **NO** ✅

**Rationale**:
1. **WebSocket control plane** → Convex provides automatic subscriptions
2. **Service orchestration** → Convex actions call AI Core/Org Core
3. **Session coordination** → Convex stores conversations/messages
4. **Event distribution via NATS** → Optional, use HTTP for now

**Recommendation**: Skip custom Gateway Service entirely. Convex replaces it.

**If you need NATS later**:
- Use for service-to-service pub/sub
- Add as standalone service
- Don't couple it to a "Gateway"

---

## Success Metrics (Phase 2)

### Production Readiness
- ✅ Rate limiting enforced
- ✅ All requests logged to audit table
- ✅ 99.9% uptime
- ✅ < 500ms P95 latency
- ✅ Automated backups

### Developer Experience
- ✅ Structured logs with trace IDs
- ✅ Grafana dashboards for all services
- ✅ Health checks pass in CI/CD
- ✅ One-command local setup

### Cost Efficiency
- ✅ 30%+ cache hit rate
- ✅ Smart model routing (save $500+/month)
- ✅ Rate limiting prevents abuse

---

**Next Action**: Implement Rate Limiting (Priority 1, Item 1)
