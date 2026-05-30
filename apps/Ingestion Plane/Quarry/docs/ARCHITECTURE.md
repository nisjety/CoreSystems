# Quarry Architecture Documentation

**Version:** 0.1.0  
**Last Updated:** 2026-03-15

---

## Table of Contents

1. [Overview](#overview)
2. [System Architecture](#system-architecture)
3. [Component Details](#component-details)
4. [Data Flow](#data-flow)
5. [Technology Stack](#technology-stack)
6. [Scalability & Performance](#scalability--performance)
7. [Security](#security)
8. [Deployment Architecture](#deployment-architecture)

---

## Overview

Quarry is a high-performance, enterprise-grade web scraper built in Go, designed as a modern alternative to Firecrawl. It combines intelligent AI-powered extraction with robust security scanning and durable workflow orchestration.

### Core Capabilities

- **Intelligent Extraction**: AI-powered content extraction via ai-core gRPC service
- **Multi-Module System**: Quick scraping, SEO analysis, multi-page crawling
- **Dual Execution**: Immediate (<500ms) and scheduled (Temporal) execution paths
- **Security-First**: 5-provider security scanning (URLhaus, PhishTank, Google Safe Browsing, AbuseIPDB, heuristics)
- **Format Flexibility**: JSON, Markdown, HTML, Screenshots, PDFs
- **Production-Ready**: Rate limiting, API key auth, metrics, observability

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                           QUARRY SYSTEM                             │
└─────────────────────────────────────────────────────────────────────┘

┌──────────────┐         ┌──────────────┐         ┌──────────────┐
│   Client     │────────▶│   API Layer  │────────▶│  Security    │
│  (HTTP/REST) │         │  (Fiber v2)  │         │  Scanning     │
└──────────────┘         └──────────────┘         └──────────────┘
                                │                         │
                                ▼                         ▼
                       ┌─────────────────┐       ┌──────────────┐
                       │ Execution Router │       │ Reputation   │
                       │ (Dual Path)      │       │ Providers (5)│
                       └─────────────────┘       └──────────────┘
                          │           │
                ┌─────────┘           └─────────┐
                ▼                                ▼
        ┌──────────────┐               ┌──────────────┐
        │  Immediate   │               │  Scheduled   │
        │  Executor    │               │  Executor    │
        │  (<500ms)    │               │  (Temporal)  │
        └──────────────┘               └──────────────┘
                │                                │
                ▼                                ▼
        ┌───────────────────────────────────────────┐
        │          Scraper Engine                   │
        │  ┌────────┐  ┌────────┐  ┌────────┐      │
        │  │ Driver │  │ Module │  │Browser │      │
        │  │Selector│  │Registry│  │  Pool  │      │
        │  └────────┘  └────────┘  └────────┘      │
        └───────────────────────────────────────────┘
                         │
           ┌─────────────┼─────────────┐
           ▼             ▼             ▼
     ┌─────────┐  ┌─────────┐  ┌─────────┐
     │ ai-core │  │  Cache  │  │Pipeline │
     │ (gRPC)  │  │ (Redis) │  │ System  │
     └─────────┘  └─────────┘  └─────────┘
```

---

## Component Details

### 1. API Layer

**Technology:** Fiber v2 (Go HTTP framework)  
**Location:** `cmd/api/main.go`, `internal/api/`

**Responsibilities:**
- HTTP request handling and routing
- Authentication (API key middleware with constant-time comparison)
- Rate limiting (per-API-key or per-IP)
- Request validation (URL format, bounds checking)
- Error envelope standardization
- Metrics collection

**Middleware Stack (in order):**
1. **Recover**: Panic recovery
2. **CORS**: Cross-origin resource sharing
3. **RequestID**: Unique request tracking
4. **Logger**: Structured logging
5. **Timeout**: Global request timeout (configurable)
6. **Timing**: Human-like delay simulation (25-150ms)
7. **ProxyRotation**: Round-robin proxy selection
8. **CustomRetry**: Smart retry for 429/502/503/504
9. **JobMonitoring**: Structured job logging
10. **APIKey**: Authentication validation
11. **RateLimiter**: Request rate limiting

**Endpoints:**
- Health: `/health`, `/ready`, `/metrics`
- Scraping: `/v1/scrape`, `/v1/crawl`, `/v1/batch`
- Discovery: `/v1/map`, `/v1/search`
- Jobs: `/v1/jobs/:id`, `/v1/jobs/:id/stream`
- Modules: `/v1/modules`

---

### 2. Security Layer

**Location:** `internal/security/`

**Components:**
- **ReputationProvider Interface**: Abstraction for security checks
- **5 Provider Implementations**:
  1. **URLhaus** (malware URLshttps://urlhaus.abuse.ch)
  2. **PhishTank** (phishing URLs)
  3. **Google Safe Browsing** (multi-threat)
  4. **AbuseIPDB** (malicious IPs)
  5. **Heuristics** (pattern-based signatures)

**Features:**
- Multi-provider consensus model
- In-memory TTL caching (500% faster on repeated checks)
- DNS/TLS enrichment
- Redirect chain analysis

**Decision Flow:**
```
 URL → Cache Check → Hit?[Yes] → Return Cached Result
                 ↓ [No]
             Check All Providers → Any Threat?
                 ↓ [No]              ↓ [Yes]
            Allow Request       Block Request
                 ↓                   ↓
           Cache Result       Log & Return Error
```

---

### 3. Execution Router

**Location:** `internal/executor/`

**Dual Execution Pattern:**
```go
if isImmediate(request) {
    // Fast path: <500ms, SSE streaming
    return immediateExecutor.Execute(ctx, req)
} else {
    // Durable path: Temporal workflow
    jobID := scheduledExecutor.Start(ctx, req)
    return jobID
}
```

**Immediate Criteria:**
- Single URL
- No scheduled time (`schedule_at` not set)
- depth <= 1 or maxPages <= 5
- No webhooks configured

**Scheduled Criteria:**
- Multiple URLs (batch, crawl)
- Deep crawling (depth > 1)
- Large maxPages (>5)
- Webhook delivery

---

### 4. Scraper Engine

**Location:** `internal/scraper/`

**Core Components:**

#### **Browser Pool**
- Shared browser instance across requests
- Page (tab) pooling with size limit
- Automatic cleanup and error recovery
- Environment-aware browser path (Docker support)

**Configuration:**
```go
type BrowserPool struct {
    browser  *rod.Browser
    launcher *launcher.Launcher
    limit    chan struct{} // Semaphore
}
```

#### **Driver System**
**Location:** `internal/driver/`

**Drivers:**
1. **Colly** (static pages): Fast, minimal overhead
2. **Rod** (dynamic pages): Full browser, JS execution

**Selector Logic:**
```go
func SelectDriver(ctx context.Context, url string) Driver {
    if needsJS(url) {  // Check for dynamic content
        return rodDriver
    }
    return collyDriver
}
```

#### **Module Registry**
**Location:** `internal/modules/`

**Available Modules:**
- **quick**: Single URL, fast extraction (<5s)
- **seo**: SEO analysis (meta, structured data, performance)
- **multi**: Multi-page crawling with depth control

**Interface:**
```go
type Module interface {
    Name() string
    Execute(ctx context.Context, req Request) (Response, error)
}
```

---

### 5. AI Integration

**Location:** `internal/ai/`

**ai-core gRPC Service:**
- **Planning**: Analyze page and generate extraction plan
- **Extraction**: Extract structured content using AI models
- **Fallback**: Heuristic extraction if AI unavailable

**Reliability Layer:**
- **Health Monitoring**: Periodic probing (every 30s)
- **Circuit Breaker**: 
  - Closed → Open after 5 consecutive failures
  - Open → Half-Open after 60s
  - Half-Open → Closed after 2 successes
- **SLO Tracking**: Success rate, latency budget (5000ms)

**Efficiency Layer** (70% cost reduction):
- **Response Caching**:
  - Plan TTL: 24h
  - Extract TTL: 1h
- **TOON Transport**: used only on the Quarry ↔ ai-core boundary
- **Hit Ratio Tracking**: Metrics exposed via `/metrics`

**Request Flow:**
```
Request → Health Check → Healthy?
             ↓ [Yes]        ↓ [No]
        Circuit Closed?    Fallback
             ↓ [Yes]
        Cache Check → Hit?
             ↓ [No]    ↓ [Yes]
        AI Request   Return Cached
             ↓
        Cache Result → Return
```

---

### 6. Transform System

**Location:** `internal/transform/`

**Transformers:**
1. **Markdown** (`markdown.go`):
   - HTML → Markdown conversion
   - Boilerplate removal (`onlyMainContent`)
   - Uses `html-to-markdown/v2` with plugins

2. **Media** (`media.go`):
   - PDF text extraction (`ledongthuc/pdf`)
   - DOCX parsing (XML-based)
   - Image metadata extraction

---

### 7. Pipeline System

**Location:** `internal/pipeline/`

**Pattern:** Chain of Responsibility

**Pipelines:**
1. **Fingerprint**: Content-based deduplication (SHA-256)
2. **Metadata**: Timestamp, job ID, request ID enrichment
3. **Stats**: Aggregation and metrics collection
4. **Storage**: Persistence (PostgreSQL, Redis, MinIO artifacts)

**Flow:**
```
Raw Data → Fingerprint → Metadata → Stats → Storage → Final Result
```

---

### 8. Caching

**Location:** `internal/cache/`, `internal/ai/cache.go`

**Layers:**
1. **Security Cache**: Reputation check results (in-memory, 15min TTL)
2. **AI Cache**: Plan/extract responses (Redis, 1h-24h TTL)
3. **Content Cache**: Page content (Redis, configurable TTL)

**Benefits:**
- Security: 500% faster repeated checks
- AI: 70% cost reduction
- Content: ~3x faster page retrieval

---

### 9. Job Store & Orchestration

**Location:** `internal/jobs/`, Temporal workflows

**Job Persistence:**
- **Backend**: PostgreSQL or Redis
- **Schema**: Job ID, status, progress, metadata
- **TTL**: Configurable retention

**Temporal Workflows:**
- **CrawlWorkflow**: Multi-page crawling with durability
- **BatchWorkflow**: Parallel URL processing
- **Activities**:
  - `FetchPageActivity`
  - `AnalyzePageActivity`
  - `StoreResultActivity`

**Durability Guarantee:**
- Workflows survive process restarts
- Automatic retry with exponential backoff
- State persistence in Temporal server

---

## Data Flow

### Synchronous Scrape (`/v1/scrape`)

```
1. Client Request
   ↓
2. API Layer (validation, auth, rate limit)
   ↓
3. Security Scan (multi-provider)
   ↓ [SAFE]
4. Immediate Executor
   ↓
5. Driver Selection (Colly vs Rod)
   ↓
6. Page Fetch (with actions if specified)
   ↓
7. AI Extraction (or heuristic fallback)
   ↓
8. Transform (Markdown, JSON normalization, artifact persistence)
   ↓
9. Pipeline (fingerprint, metadata, stats)
   ↓
10. Response (JSON envelope)
```

**Typical Latency:** 100-500ms (with cache hits: 20-50ms)

---

### Asynchronous Crawl (`/v1/crawl`)

```
1. Client Request
   ↓
2. API Layer (validation)
   ↓
3. Security Scan
   ↓ [SAFE]
4. Scheduled Executor
   ↓
5. Temporal Workflow Start → Return Job ID immediately
   ↓ (async)
6. CrawlWorkflow Activities:
   - Fetch root page
   - Extract links
   - For each link (breadth-first):
     ├─  Security scan
     ├─ Fetch page
     ├─ AI extraction
     └─ Store result
   ↓
7. Job Status Updates (via `/v1/jobs/:id` or SSE)
   ↓
8. Webhook Delivery (if configured, HMAC-signed)
```

**Typical Duration:** 10s - 5min (depending on maxPages, depth)

---

## Technology Stack

### Core

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Runtime** | Go | 1.24.1 | Primary language |
| **HTTP Framework** | Fiber | v2.52.9 | High-performance web framework |
| **orchestration** | Temporal | v1.31.0 | Durable workflows |
| **Browser** | Rod | v0.116.2 | Headless browser automation |
| **Scraping** | Colly | v2.3.0 | Fast static scraping |

### Data Layer

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Job Store** | PostgreSQL | 16 | Job persistence |
| **Cache** | Redis | 7.4 | AI cache, content cache |
| **Artifact Store** | MinIO | latest | Binary artifact storage for screenshots/PDFs |
| **Vector DB** | Qdrant | v1.13.4 | Semantic search (future) |
| **Message Queue** | NATS | 2.11 | Event streaming |

### AI & Extraction

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **AI Service** | ai-core | gRPC | Intelligent extraction |
| **Markdown** | html-to-markdown | v2.5.0 | HTML→MD conversion |
| **PDF Parser** | ledongthuc/pdf | latest | PDF text extraction |

### Observability

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Logging** | zerolog | v1.33.0 | Structured logging |
| **Metrics** | Custom (atomic) | - | Request/error tracking |
| **Tracing** | OpenTelemetry | TBD | Distributed tracing (future) |

---

## Scalability & Performance

### Horizontal Scaling

**API Servers:**
- Stateless design
- Scale behind load balancer
- Shared Redis cache
- Session affinity not required

**Temporal Workers:**
- Scale independently from API
- Auto-discovery via task queue
- Workflow partitioning by shard

**Bottlenecks:**
- **ai-core gRPC**: Scale ai-core replicas
- **Browser Pool**: Increase pool size per instance
- **Redis**: Use Redis Cluster for cache layer

### Performance Characteristics

| Operation | Latency (p50) | Latency (p99) | Throughput |
|-----------|---------------|---------------|------------|
| `/health` | 2ms | 10ms | ~10,000 req/s |
| `/v1/scrape` (cache hit) | 25ms | 80ms | ~400 req/s |
| `/v1/scrape` (cache miss, AI) | 150ms | 500ms | ~100 req/s |
| `/v1/crawl` (job start) | 50ms | 150ms | ~200 req/s |

**Optimizations:**
- **Browser Pooling**: 10x faster than per-request browsers
- **Reputation Caching**: 500% faster security checks
- **AI Caching**: 70% cost reduction, 3x latency improvement
- **Connection Pooling**: gRPC, Redis, PostgreSQL

### Resource Usage (per instance)

| Resource | Idle | Light Load | Heavy Load |
|----------|------|------------|------------|
| **CPU** | 0.1 cores | 1-2 cores | 4-6 cores |
| **Memory** | 100MB | 300MB | 800MB |
| **Connections** | 50 | 200 | 500 |

---

## Security

### Defense Layers

1. **API Key Authentication**: Constant-time comparison (timing-attack safe)
2. **Rate Limiting**: Per-API-key and per-IP with differentiated limits
3. **Input Validation**: URL format, bounds checking, sanitization
4. **Reputation Scanning**: 5-provider consensus before fetching
5. **Content Security**: CSP headers, XSS prevention
6. **Network Security**: HTTPS only, webhook signature verification

### Security Best Practices

- **Secrets Management**: Environment variables, never in code
- **HMAC Signatures**: Webhook payload verification (SHA-256)
- **Least Privilege**: Service accounts with minimal permissions
- **Audit Logging**: Structured logs with request IDs

### Threat Model

| Threat | Mitigation |
|--------|------------|
| **Malicious URLs** | 5-provider security scanning |
| **DDoS** | Rate limiting, proxy rotation, Cloudflare |
| **Data Exfiltration** | API key scoping, webhook allowlists |
| **Timing Attacks** | Constant-time API key comparison |
| **Replay Attacks** | Request ID uniqueness, timestamp validation |

---

## Deployment Architecture

### Docker Compose (Development)

```yaml
services:
  - quarry-api (port 8090)
  - quarry-worker (Temporal worker)
  - quarry-temporal (Temporal server, port 7234)
  - quarry-temporal-ui (port 8089)
  - quarry-redis (port 6380)
  - quarry-postgres (port 5434)
  - quarry-qdrant (port 6335, 6336)
  - quarry-nats (port 4223, 8223)
```

**All ports offset to avoid conflicts with other local services.**

### Production (Kubernetes)

```
┌─────────────────────────────────────────────┐
│              Load Balancer (HTTPS)          │
└─────────────────────────────────────────────┘
              │
    ┌─────────┴──────────┐
    ▼                    ▼
┌────────┐          ┌────────┐
│API Pod │          │API Pod │  (Deployments,  Horizontal Pod Autoscaler)
└────────┘          └────────┘
    │                    │
    └────────┬───────────┘
             ▼
    ┌────────────────┐
    │ Redis Cluster  │  (StatefulSet)
    └────────────────┘
             ▼
    ┌────────────────┐
    │   PostgreSQL   │  (StatefulSet or Cloud RDS)
    └────────────────┘
             ▼
    ┌────────────────┐
    │Temporal Cluster│  (Helm chart)
    └────────────────┘
```

**HA Configuration:**
- **API**: 3+ replicas
- **Workers**: 5+ replicas (scale with queue depth)
- **Temporal**: 3-node cluster
- **Redis**: Sentinel or Cluster mode
- **PostgreSQL**: Primary + read replicas

---

## Monitoring & Observability

### Metrics (Exposed via `/metrics`)

**Request Metrics:**
- `requests_total`: Total requests
- `requests_per_sec`: Current throughput
- `avg_response_time_ms`: Latency average
- `error_rate`: Error ratio
- `errors_total`: Total errors

**AI Metrics:**
- `ai_health_healthy`: AI service health (boolean)
- `ai_circuit_state`: Circuit breaker state
- `ai_slo_success_rate`: AI SLO success rate
- `ai_cache_hit_ratio`: Cache efficiency

**System Metrics:**
- `cache_hit_rate`: Overall cache efficiency
- `browser_pool_size`: Active browser pages
- `goroutines`: Active goroutines (leak detection)

### Logging

**Structured JSON Logs:**
```json
{
  "level": "info",
  "time": "2026-02-18T16:00:00Z",
  "message": "Request completed",
  "request_id": "req_abc123",
  "method": "POST",
  "path": "/v1/scrape",
  "status": 200,
  "latency_ms": 145,
  "extraction_method": "ai"
}
```

**Log Levels:**
- `debug`: Verbose debugging
- `info`: Normal operations
- `warn`: Warnings (fallbacks, retries)
- `error`: Errors requiring attention
- `fatal`: Critical failures

---

## Future Enhancements

1. **OpenTelemetry Integration**: Distributed tracing (Jaeger/Tempo)
2. **GraphQL API**: Alternative to REST
3. **WebSocket Support**: Real-time job updates
4. **Advanced Caching**: Multi-tier with Memcached
5. **ML-based Driver Selection**: Automatic JS detection
6. **Edge Deployment**: Cloudflare Workers, Fastly Compute
7. **Multi-region**: Geo-distributed scraping

---

## Glossary

- **ai-core**: External gRPC service providing AI-powered extraction
- **TOON**: Token-Oriented Object Notation used only for Quarry ↔ ai-core transport
- **Circuit Breaker**: Pattern to prevent cascading failures
- **Temporal**: Durable workflow orchestration platform
- **Driver**: Abstraction for page fetching (Colly static, Rod dynamic)
- **Module**: Scraping strategy (quick, seo, multi)

---

**References:**
- [Fiber Documentation](https://docs.gofiber.io)
- [Temporal Documentation](https://docs.temporal.io)
- [Rod Documentation](https://go-rod.github.io)

**Last Updated:** 2026-02-18
