# Next Steps: High Impact Improvements

## Quick Wins Status ✅

All 4 Quick Wins have been **successfully implemented and deployed**:

| Feature | Service | Status | Verified |
|---------|---------|--------|----------|
| Streaming Responses | ai-core | ✅ Deployed | `/stream/health` → 200 OK |
| Model Registry & Failover | ai-core | ✅ Deployed | 4-model chain active |
| WebSocket API | org-core | ✅ Deployed | `/ws/stats` → 200 OK |
| Webhook System | org-core | ✅ Deployed | `/api/v1/webhooks/stats` → 200 OK |

---

## Phase 2: High Impact Improvements

### Priority 1 (Critical) - Start Immediately

#### 1. Session Management (1-2 weeks) 🎯
**Why Critical**: Multi-conversation support is fundamental for AI chat applications

**Implementation Plan**:
```
Files to Create:
- /backend/Org-core/internal/sessions/manager.go (250 lines)
  - SessionManager struct with PostgreSQL/Redis storage
  - CreateSession, GetSession, ListSessions, DeleteSession methods
  - Context window management (track token usage)
  - Automatic cleanup of old sessions

- /backend/Org-core/internal/sessions/handler.go (150 lines)
  - HTTP handlers for session CRUD
  - POST /api/v1/sessions - Create new session
  - GET /api/v1/sessions - List sessions for org
  - GET /api/v1/sessions/:id - Get session with full history
  - DELETE /api/v1/sessions/:id - Delete session
  - POST /api/v1/sessions/:id/messages - Add message to session

- /backend/Org-core/migrations/003_sessions.up.sql
  - sessions table (id, org_id, title, created_at, updated_at)
  - session_messages table (id, session_id, role, content, tokens, timestamp)
  - Indexes on org_id, session_id

Database Schema:
CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    title VARCHAR(255),
    metadata JSONB,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

CREATE TABLE session_messages (
    id UUID PRIMARY KEY,
    session_id UUID REFERENCES sessions(id) ON DELETE CASCADE,
    role VARCHAR(20) NOT NULL, -- user, assistant, system
    content TEXT NOT NULL,
    tokens INTEGER,
    timestamp TIMESTAMP
);
```

**Integration Points**:
- Integrate with ai-core chat endpoints
- Update WebSocket to broadcast per-session
- Add session_id to webhook events

**Testing**:
```bash
# Create session
curl -X POST http://localhost:8080/api/v1/sessions \
  -H "X-Org-ID: <org-id>" \
  -d '{"title": "Customer Support Chat"}'

# Add message
curl -X POST http://localhost:8080/api/v1/sessions/<session-id>/messages \
  -d '{"role": "user", "content": "Hello"}'

# Get history
curl http://localhost:8080/api/v1/sessions/<session-id>
```

**Success Metrics**:
- Support 1000+ concurrent sessions per org
- < 50ms session retrieval latency
- Automatic context window management (8K/16K/32K tokens)

---

#### 2. Job Queue System (1 week) 🎯
**Why Critical**: Async processing for RAG indexing and long-running operations

**Implementation Plan**:
```
Technology: Asynq (Redis-backed job queue for Go)

Files to Create:
- /backend/Org-core/internal/jobs/manager.go (200 lines)
  - JobManager with Asynq client
  - EnqueueRAGIndex, EnqueueCrawl, EnqueueExport methods
  - Job status tracking in PostgreSQL

- /backend/Org-core/internal/jobs/worker.go (250 lines)
  - Asynq worker with handlers
  - HandleRAGIndexJob - Process RAG indexing
  - HandleCrawlJob - Process web crawling
  - HandleExportJob - Process GDPR exports
  - Error handling and retry logic

- /backend/Org-core/internal/jobs/handlers.go (180 lines)
  - HTTP handlers for job management
  - POST /api/v1/jobs - Create new job
  - GET /api/v1/jobs/:id - Get job status
  - GET /api/v1/jobs - List jobs for org
  - DELETE /api/v1/jobs/:id - Cancel job

- /backend/Org-core/migrations/004_jobs.up.sql
  - jobs table (id, org_id, type, status, payload, result, created_at)

Job Types:
- rag_index - Index documents into RAG
- crawl - Web crawling
- export - GDPR data export
- batch_delete - Bulk deletion
```

**Dependencies**:
```bash
go get github.com/hibiken/asynq
```

**Integration Points**:
- Replace sync RAG indexing with async jobs
- Send WebSocket progress updates during job execution
- Trigger webhooks on job completion/failure
- Store job results in PostgreSQL for persistence

**Configuration**:
```env
REDIS_URL=redis://aquatiq-redis-local:6379
JOB_QUEUE_WORKERS=5
JOB_QUEUE_CONCURRENCY=10
```

**Testing**:
```bash
# Enqueue RAG indexing job
curl -X POST http://localhost:8080/api/v1/jobs \
  -d '{
    "type": "rag_index",
    "payload": {
      "documents": ["doc1.pdf", "doc2.pdf"],
      "collection": "knowledge-base"
    }
  }'

# Check job status
curl http://localhost:8080/api/v1/jobs/<job-id>
```

**Success Metrics**:
- Process 100+ jobs/second
- < 1% job failure rate
- Automatic retry with exponential backoff
- Dead-letter queue for failed jobs

---

### Priority 2 (High) - Next Week

#### 3. Rate Limiting (3-4 days)
**Why Important**: Protect service from abuse, ensure fair usage

**Implementation**:
```
Technology: Redis-backed token bucket algorithm

Files to Create:
- /backend/Org-core/internal/middleware/ratelimit.go (120 lines)
  - RateLimitMiddleware for Gin
  - Per-org token bucket implementation
  - Configurable limits per endpoint

Configuration:
type RateLimitConfig struct {
    RequestsPerMinute int
    BurstSize         int
    ByOrg             bool
}

Limits:
- /api/v1/rag/retrieve: 60 req/min per org
- /api/v1/rag/documents: 30 req/min per org
- /stream/chat: 120 req/min per org
```

**Testing**:
```bash
# Should succeed 60 times, then return 429
for i in {1..70}; do
  curl http://localhost:8080/api/v1/rag/retrieve
done
```

---

#### 4. Prompt Template Management (3-5 days)
**Why Important**: Version-controlled prompts, A/B testing

**Implementation**:
```
Files to Create:
- /backend/ai-core/app/services/template_manager.py (200 lines)
  - Jinja2 template system
  - Template versioning
  - A/B test support

Templates:
templates/
  rag_retrieval.j2
  summarization.j2
  chat_completion.j2
  agentic_reasoning.j2

Database:
CREATE TABLE prompt_templates (
    id UUID PRIMARY KEY,
    name VARCHAR(100),
    version INTEGER,
    template TEXT,
    variables JSONB,
    active BOOLEAN,
    created_at TIMESTAMP
);
```

---

## Implementation Roadmap

### Week 1-2: Session Management
- [ ] Day 1-2: Design schema and manager interface
- [ ] Day 3-5: Implement SessionManager with PostgreSQL
- [ ] Day 6-7: Create HTTP handlers and endpoints
- [ ] Day 8-9: Integration with ai-core chat
- [ ] Day 10: Testing and documentation

### Week 3: Job Queue System
- [ ] Day 1-2: Setup Asynq and worker infrastructure
- [ ] Day 3-4: Implement job handlers (RAG, crawl, export)
- [ ] Day 5: Create HTTP API for job management
- [ ] Day 6-7: Integration testing and monitoring

### Week 4: Rate Limiting + Templates
- [ ] Day 1-2: Implement rate limiting middleware
- [ ] Day 3-4: Configure limits per endpoint
- [ ] Day 5-7: Prompt template system

---

## Risk Mitigation

### Session Management Risks:
- **Risk**: Large conversation histories → High memory usage
- **Mitigation**: Implement pagination, context window truncation, Redis caching for recent messages

### Job Queue Risks:
- **Risk**: Redis outage → Jobs lost
- **Mitigation**: Enable Asynq persistence to disk, implement graceful degradation

### Rate Limiting Risks:
- **Risk**: Redis latency → Slow requests
- **Mitigation**: Local token bucket cache, async Redis updates

---

## Success Criteria

By end of Phase 2:
- ✅ Multi-conversation support with 10K+ sessions
- ✅ Async job processing for RAG indexing
- ✅ Protected endpoints with fair usage limits
- ✅ Version-controlled prompt templates
- ✅ 99.9% uptime maintained
- ✅ < 100ms p95 latency for all endpoints

---

## Next Phase Preview: Production Hardening (Week 5-6)

1. **Observability** (2-3 days)
   - Prometheus metrics for all services
   - Grafana dashboards
   - Distributed tracing with Jaeger

2. **Security Hardening** (2-3 days)
   - API key rotation
   - Request signing
   - Audit logging

3. **Performance Optimization** (2-3 days)
   - Query optimization
   - Connection pooling tuning
   - Cache warming strategies

4. **Disaster Recovery** (2-3 days)
   - Automated backups
   - Point-in-time recovery
   - Failover testing

---

**Ready to Start**: Session Management implementation
**Recommendation**: Begin with SessionManager design and schema definition
**Estimated Completion**: End of February 2026

---

Last Updated: 2026-02-01
