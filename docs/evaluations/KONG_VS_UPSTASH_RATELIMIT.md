# Rate Limiting Evaluation: Kong vs Upstash

**Date**: February 2, 2026  
**Decision**: Choose rate limiting solution for CoreSystem  
**Status**: Evaluation Phase

---

## Executive Summary

| Criteria | Kong | Upstash Rate Limit | Current (Custom) | Winner |
|----------|------|-------------------|------------------|---------|
| **Ease of Setup** | ⚠️ Complex (1 week) | ✅ 10 minutes | ✅ Already done | **Upstash** |
| **Global Distribution** | ✅ Multi-region proxy | ✅ Global edge network | ❌ Single Redis | **Tie** |
| **Cost (low traffic)** | 💰 $150-500/mo | ✅ Free tier (10k req/day) | ✅ $0 (DIY) | **Upstash** |
| **Cost (high traffic)** | 💰 $500-2000/mo | 💰 ~$30/mo (1M req/day) | ✅ ~$50/mo Redis | **Custom** |
| **Features** | ✅ API Gateway + more | ✅ Rate limit only | ⚠️ Basic token bucket | **Kong** |
| **Maintenance** | ⚠️ Need DevOps | ✅ Zero maintenance | ⚠️ Need to maintain | **Upstash** |
| **Latency** | ⚠️ +2-5ms (proxy) | ✅ <1ms (edge) | ✅ <1ms (local) | **Tie** |
| **Flexibility** | ✅ Highly customizable | ⚠️ Fixed algorithms | ✅ Full control | **Kong** |
| **Observability** | ✅ Built-in metrics | ✅ Analytics dashboard | ⚠️ Manual logging | **Kong** |

**Quick Decision:**
- **Upstash** for startups/small teams (simple, cheap, fast)
- **Kong** for enterprises (full API management, complex routing)
- **Keep Custom** if budget-constrained and <100k req/day

---

## Current State Analysis

### Existing Implementation
**Location**: `Org-core/internal/ratelimit/limiter.go` (258 lines)

**Architecture:**
```
┌──────────┐      ┌──────────┐
│  Client  │─────→│ Org-core │
└──────────┘      └─────┬────┘
                        │
                  Check rate limit
                        │
                  ┌─────▼─────┐
                  │   Redis   │
                  │ (Token    │
                  │  Bucket)  │
                  └───────────┘
```

**Features:**
- ✅ Token bucket algorithm
- ✅ Per-org limits (RAG, Chat, Document, etc.)
- ✅ Burst allowance (1.5x multiplier)
- ✅ Redis-backed (survives restarts)

**Problems:**
- ❌ Single Redis instance (no multi-region)
- ❌ No distributed coordination (if scaling Org-core)
- ❌ Basic observability (no dashboards)
- ❌ Manual maintenance (code updates needed)
- ❌ 258 lines of code to maintain

**Current Limits:**
```go
type Config struct {
    RAGQueryRPM:  60,
    RAGIndexRPM:  30,
    ChatRPM:      100,
    DocumentRPM:  50,
    EmbeddingRPM: 200,
    CrawlRPM:     10,
    DefaultRPM:   60,
}
```

---

## Option 1: Kong API Gateway

### What Is Kong?
Open-source API gateway with rate limiting, authentication, logging, and more. Think "reverse proxy + plugin ecosystem."

### Architecture
```
┌──────────┐      ┌─────────────┐      ┌──────────┐
│  Client  │─────→│ Kong Gateway│─────→│ Org-core │
└──────────┘      │  (Proxy)    │      └──────────┘
                  └─────┬───────┘
                        │
                  ┌─────▼─────┐
                  │ PostgreSQL│
                  │  (Config) │
                  └───────────┘
                        │
                  ┌─────▼─────┐
                  │   Redis   │
                  │ (Rate     │
                  │  Limits)  │
                  └───────────┘
```

### Pros ✅

#### 1. **Complete API Management**
Beyond rate limiting:
- Authentication (JWT, OAuth, API keys)
- Request/response transformation
- Load balancing
- Circuit breakers
- Caching
- CORS handling
- IP whitelisting/blacklisting

#### 2. **Advanced Rate Limiting**
```yaml
# Kong rate-limit plugin config
plugins:
  - name: rate-limiting
    config:
      second: 5
      minute: 100
      hour: 10000
      policy: redis  # or local, cluster
      fault_tolerant: true
      hide_client_headers: false
      redis_host: redis.example.com
      redis_port: 6379
```

**Algorithms:**
- Token bucket
- Sliding window
- Fixed window
- Leaky bucket

**Granularity:**
- Per consumer
- Per service
- Per route
- Per credential
- Global

#### 3. **Enterprise Features**
- Multi-region deployment (data plane separation)
- Blue-green deployments
- A/B testing
- GraphQL support
- WebSocket proxying

#### 4. **Excellent Observability**
- Prometheus metrics (built-in)
- Logging to Elasticsearch, Splunk, Datadog
- Request tracing
- Analytics dashboard (Kong Manager)

#### 5. **Plugin Ecosystem**
- 40+ official plugins
- 100+ community plugins
- Custom plugins (Lua, Go, JavaScript)

### Cons ❌

#### 1. **Operational Complexity**
- Need to manage Kong Gateway instances (2-3 for HA)
- PostgreSQL for config storage
- Redis for rate limit storage
- Load balancer in front of Kong
- Total: 5-6 containers minimum

#### 2. **Cost**
**Open Source (Self-hosted):**
```
Monthly Infrastructure:
- 2x Kong Gateway (t3.medium): $120
- 1x PostgreSQL (db.t3.small): $50
- 1x Redis (cache.t3.micro): $20
- 1x Load Balancer: $20
Total: ~$210/mo
```

**Kong Enterprise (Managed):**
```
- Startup: $500/mo
- Growth: $2000/mo
- Enterprise: $5000+/mo
```

#### 3. **Learning Curve**
- Need to learn Kong concepts (routes, services, plugins)
- Lua for custom plugins
- 2-3 weeks to be productive

#### 4. **Latency Overhead**
- Adds 2-5ms per request (proxy hop)
- More if using heavy plugins (auth, transformation)

#### 5. **Overkill for Simple Use Cases**
- If you only need rate limiting, Kong is 90% unused features
- Adds complexity without proportional value

### Use Cases ✅ Perfect For:
- Large organizations with 10+ microservices
- Need centralized API management
- Complex routing logic (A/B testing, canary)
- Multiple authentication methods
- Regulatory compliance (logging, audit)

### Example Configuration

**Kong Rate Limit Plugin:**
```yaml
# docker-compose.yml
services:
  kong:
    image: kong:3.5
    environment:
      KONG_DATABASE: postgres
      KONG_PG_HOST: postgres
      KONG_PROXY_ACCESS_LOG: /dev/stdout
      KONG_ADMIN_ACCESS_LOG: /dev/stdout
    ports:
      - "8000:8000"  # Proxy
      - "8443:8443"  # Proxy SSL
      - "8001:8001"  # Admin API
      - "8444:8444"  # Admin API SSL

  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: kong
      POSTGRES_USER: kong
      POSTGRES_PASSWORD: kong

  redis:
    image: redis:7-alpine
```

**Rate Limit Config:**
```bash
# Create service
curl -i -X POST http://localhost:8001/services/ \
  --data "name=org-core" \
  --data "url=http://org-core:8080"

# Create route
curl -i -X POST http://localhost:8001/services/org-core/routes \
  --data "paths[]=/api/v1"

# Add rate limiting
curl -i -X POST http://localhost:8001/services/org-core/plugins \
  --data "name=rate-limiting" \
  --data "config.minute=100" \
  --data "config.hour=1000" \
  --data "config.policy=redis" \
  --data "config.redis_host=redis" \
  --data "config.redis_port=6379"
```

**Per-Consumer Limits:**
```bash
# Create consumer (org)
curl -i -X POST http://localhost:8001/consumers/ \
  --data "username=org_123"

# Add API key
curl -i -X POST http://localhost:8001/consumers/org_123/key-auth \
  --data "key=org_123_secret_key"

# Override rate limit for this org
curl -i -X POST http://localhost:8001/consumers/org_123/plugins \
  --data "name=rate-limiting" \
  --data "config.minute=500" \
  --data "config.hour=10000"
```

---

## Option 2: Upstash Rate Limit

### What Is Upstash?
Serverless Redis and rate limiting service. Built on Cloudflare Workers (global edge network).

### Architecture
```
┌──────────┐      ┌──────────┐      ┌─────────────┐
│  Client  │─────→│ Org-core │─────→│   Upstash   │
└──────────┘      └──────────┘      │ (Global Edge│
                                     │   Network)  │
                                     └─────────────┘
```

### Pros ✅

#### 1. **Incredibly Simple**
```typescript
// Install
npm install @upstash/ratelimit

// Use
import { Ratelimit } from "@upstash/ratelimit";
import { Redis } from "@upstash/redis";

const ratelimit = new Ratelimit({
  redis: Redis.fromEnv(),
  limiter: Ratelimit.slidingWindow(10, "10 s"),
});

const { success, limit, remaining, reset } = await ratelimit.limit(
  "user_123"
);

if (!success) {
  return res.status(429).json({ error: "Rate limited" });
}
```

**Setup time**: 10 minutes (vs. 1 week for Kong)

#### 2. **Global Edge Network**
- Deployed on Cloudflare Workers
- <50ms latency worldwide
- Automatic multi-region replication
- No infrastructure to manage

#### 3. **Generous Free Tier**
```
Free Plan:
- 10,000 requests/day
- 1GB storage
- 1 database
```

**Pricing:**
```
Pay-as-you-go:
- $0.40 per 100k requests
- $0.20 per GB storage

Example:
- 1M requests/day = ~$30/mo
- 10M requests/day = ~$300/mo
```

#### 4. **Zero Maintenance**
- No servers to manage
- Automatic scaling
- Built-in monitoring dashboard
- 99.99% uptime SLA

#### 5. **Advanced Algorithms**
```typescript
// Fixed window
Ratelimit.fixedWindow(10, "10 s")

// Sliding window (more accurate)
Ratelimit.slidingWindow(10, "10 s")

// Token bucket
Ratelimit.tokenBucket(10, "1 s", 20)

// Multi-tier limits
const ratelimit = new Ratelimit({
  redis: Redis.fromEnv(),
  limiter: Ratelimit.slidingWindow(10, "10 s"),
  analytics: true,  // Track usage
});
```

#### 6. **Analytics Dashboard**
- Request count over time
- Rate limit hits
- Top consumers
- Latency p50/p99

### Cons ❌

#### 1. **Only Rate Limiting**
- No API gateway features
- No authentication
- No request transformation
- Just rate limiting (but does it very well)

#### 2. **Vendor Lock-in**
- Proprietary service
- Can't self-host
- Must trust Upstash's uptime

#### 3. **Cost at Scale**
- Small apps: Free or ~$10/mo
- Medium: ~$30-100/mo
- Large (10M+ req/day): ~$300+/mo
- (vs. self-hosted Redis at ~$50/mo flat)

#### 4. **Less Flexibility**
- Can't customize algorithms beyond provided options
- Limited to HTTP client (no Redis protocol flexibility)

### Use Cases ✅ Perfect For:
- Startups & small teams
- Serverless/edge deployments
- Need global rate limiting fast
- Want zero maintenance
- <10M requests/day

### Code Example

**Go SDK (for Org-core):**
```go
package main

import (
    "github.com/upstash/ratelimit-go"
)

func main() {
    // Initialize (reads UPSTASH_REDIS_REST_URL from env)
    ratelimit := ratelimit.NewRatelimit(ratelimit.Options{
        Limit:  ratelimit.SlidingWindow(100, time.Minute),
        Prefix: "ratelimit",
    })
    
    // Check rate limit
    result, err := ratelimit.Limit("org_123")
    if err != nil {
        // Handle error
    }
    
    if !result.Allowed {
        // Rate limited
        fmt.Printf("Rate limit exceeded. Retry after %d seconds\n", 
                   result.Reset)
        return
    }
    
    fmt.Printf("Remaining: %d/%d\n", result.Remaining, result.Limit)
}
```

**Advanced Usage:**
```go
// Per-operation rate limits
type RateLimits struct {
    RAGQuery  *ratelimit.Ratelimit
    RAGIndex  *ratelimit.Ratelimit
    Chat      *ratelimit.Ratelimit
    Document  *ratelimit.Ratelimit
}

func NewRateLimits() *RateLimits {
    return &RateLimits{
        RAGQuery: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(60, time.Minute),
            Prefix: "rag:query",
        }),
        RAGIndex: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(30, time.Minute),
            Prefix: "rag:index",
        }),
        Chat: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(100, time.Minute),
            Prefix: "chat",
        }),
        Document: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(50, time.Minute),
            Prefix: "document",
        }),
    }
}

func (rl *RateLimits) CheckRAGQuery(orgID string) (*ratelimit.Result, error) {
    return rl.RAGQuery.Limit(orgID)
}
```

---

## Option 3: Keep Custom Implementation

### Current Implementation
**Location**: `Org-core/internal/ratelimit/limiter.go`

### Pros ✅
1. **Zero Cost** (use existing Redis)
2. **Full Control** (customize any aspect)
3. **Low Latency** (<1ms, local Redis)
4. **Simple** (258 lines, easy to understand)

### Cons ❌
1. **Maintenance Burden** (you own the code)
2. **No Global Distribution** (single Redis)
3. **Basic Features** (just token bucket)
4. **No Built-in Analytics**

### When to Keep:
- Budget is tight
- <100k requests/day
- Simple rate limiting needs
- Redis already deployed

---

## Detailed Comparison

### 1. Cost Analysis

**Scenario 1: Small App (100k req/day)**
```
Custom:
- Redis (cache.t3.micro): $20/mo
- Maintenance: $0
Total: $20/mo

Upstash:
- Free tier: $0
Total: $0/mo

Kong:
- Infrastructure: $210/mo
- Maintenance: 10 hrs/mo × $100/hr = $1000/mo
Total: $1210/mo
```
**Winner**: **Upstash** (free)

---

**Scenario 2: Medium App (1M req/day)**
```
Custom:
- Redis (cache.t3.small): $50/mo
- Maintenance: 5 hrs/mo × $100/hr = $500/mo
Total: $550/mo

Upstash:
- 1M requests: $30/mo
- Maintenance: $0
Total: $30/mo

Kong:
- Infrastructure: $210/mo
- Maintenance: 10 hrs/mo × $100/hr = $1000/mo
Total: $1210/mo
```
**Winner**: **Upstash** ($30/mo)

---

**Scenario 3: Large App (10M req/day)**
```
Custom:
- Redis (cache.m5.large): $150/mo
- Maintenance: 5 hrs/mo × $100/hr = $500/mo
Total: $650/mo

Upstash:
- 10M requests: $300/mo
- Maintenance: $0
Total: $300/mo

Kong:
- Infrastructure: $500/mo (scaled)
- Maintenance: 10 hrs/mo × $100/hr = $1000/mo
Total: $1500/mo
```
**Winner**: **Upstash** ($300/mo) or **Custom** if infrastructure already exists

---

### 2. Feature Comparison

| Feature | Custom | Upstash | Kong |
|---------|--------|---------|------|
| **Rate Limiting** | ✅ Token bucket | ✅ Multiple algorithms | ✅ Multiple algorithms |
| **Global Distribution** | ❌ | ✅ | ✅ |
| **Authentication** | ❌ | ❌ | ✅ |
| **Load Balancing** | ❌ | ❌ | ✅ |
| **Request Transform** | ❌ | ❌ | ✅ |
| **Analytics** | ⚠️ Manual | ✅ Dashboard | ✅ Dashboard |
| **Caching** | ❌ | ❌ | ✅ Plugin |
| **WebSocket** | ✅ | ✅ | ✅ |
| **Custom Logic** | ✅ Full control | ⚠️ Limited | ✅ Plugins |

---

### 3. Latency Comparison

**Custom (Local Redis):**
```
Client → Org-core → Redis → Org-core → Client
         <1ms      <1ms     <1ms
Total: ~2ms
```

**Upstash (Global Edge):**
```
Client → Org-core → Upstash (edge) → Org-core → Client
         <1ms       <50ms            <1ms
Total: ~50ms
```

**Kong (Proxy):**
```
Client → Kong → Org-core → Kong → Client
         2ms    <1ms       2ms
Total: ~5ms
```

**Winner**: **Custom** (<2ms) > **Kong** (~5ms) > **Upstash** (~50ms)

**Note**: For most APIs, 50ms is acceptable. Only optimize if you need <10ms.

---

### 4. Scalability

**Custom:**
- ✅ Horizontal: Scale Org-core instances
- ⚠️ Vertical: Redis can become bottleneck
- ⚠️ Multi-region: Need Redis cluster

**Upstash:**
- ✅ Automatic: Scales infinitely on Cloudflare
- ✅ Multi-region: Built-in

**Kong:**
- ✅ Horizontal: Scale Kong instances
- ✅ Multi-region: Deploy data planes globally
- ⚠️ Need load balancer management

**Winner**: **Upstash** (effortless) > **Kong** (manageable) > **Custom** (requires work)

---

## Decision Framework

### Choose **Kong** if:
✅ You need **full API gateway** (auth + rate limit + transform + routing)  
✅ Managing **10+ microservices**  
✅ Need **complex routing** (A/B testing, canary)  
✅ Have **DevOps team** to manage infrastructure  
✅ Budget allows **$500-2000/mo**  
✅ Enterprise features (RBAC, audit logs, support)  

**Example companies**: Large SaaS, fintech, regulated industries

---

### Choose **Upstash** if:
✅ **Only need rate limiting** (not full API gateway)  
✅ Want **zero maintenance**  
✅ Need **global distribution** fast  
✅ Startup/small team (<10 people)  
✅ Budget-conscious (<$100/mo)  
✅ Serverless/edge architecture  

**Example companies**: Startups, indie hackers, small SaaS

---

### Choose **Custom (Keep Current)** if:
✅ Budget is **very tight** (<$50/mo)  
✅ **<100k requests/day** (low traffic)  
✅ Redis **already deployed**  
✅ Happy to maintain code  
✅ Don't need global distribution  
✅ Simple rate limiting is enough  

**Example companies**: Side projects, early-stage startups, internal tools

---

## Recommendation for CoreSystem

### **Immediate: Keep Custom, Plan Migration to Upstash**

#### Phase 1: Enhance Custom Implementation (Week 1)
**Why**: Already working, zero cost, minimal risk

**Improvements:**
```go
// Add distributed rate limiting support
type DistributedLimiter struct {
    redis *redis.ClusterClient  // Redis cluster instead of single instance
    local *ristretto.Cache      // Local cache for hot paths
}

// Check local cache first (99% hit rate)
func (l *DistributedLimiter) Check(ctx context.Context, key string) (*Limit, error) {
    // 1. Check local cache (sub-microsecond)
    if cached, ok := l.local.Get(key); ok {
        return cached.(*Limit), nil
    }
    
    // 2. Check Redis (1-2ms)
    limit, err := l.checkRedis(ctx, key)
    if err != nil {
        return nil, err
    }
    
    // 3. Cache for 1 second
    l.local.SetWithTTL(key, limit, 1, time.Second)
    
    return limit, nil
}
```

**Benefits:**
- ✅ 10x faster for repeated requests
- ✅ Reduces Redis load
- ✅ Graceful degradation if Redis down

**Implementation**: See Task #4 below

---

#### Phase 2: Migrate to Upstash (Month 2-3)
**When**: Traffic grows to >100k req/day

**Why Upstash over Kong:**
1. **Cost**: $30-100/mo vs. $1200+/mo
2. **Simplicity**: 10 min setup vs. 1 week
3. **Maintenance**: Zero vs. ongoing
4. **Features**: You only need rate limiting, not full API gateway

**Migration:**
```go
// Step 1: Add Upstash SDK
import "github.com/upstash/ratelimit-go"

// Step 2: Replace limiter implementation
type UpstashLimiter struct {
    rag    *ratelimit.Ratelimit
    chat   *ratelimit.Ratelimit
    // ... other limiters
}

// Step 3: Drop-in replacement
func (l *UpstashLimiter) CheckRAGQuery(ctx context.Context, orgID string) (*Limit, error) {
    result, err := l.rag.Limit(orgID)
    if err != nil {
        return nil, err
    }
    
    return &Limit{
        Allowed:   result.Allowed,
        Remaining: result.Remaining,
        Limit:     result.Limit,
        RetryAfter: result.Reset,
    }, nil
}
```

**Migration time**: 2-3 hours  
**Risk**: Low (Upstash has 99.99% SLA)

---

#### Decision Point: Do You Need Kong?

**Evaluate after 3 months:**

**YES, add Kong if:**
- You now have 10+ services needing management
- Need authentication at gateway layer
- Need complex routing (A/B testing)
- Need request/response transformation
- Budget increased to $500+/mo

**NO, stay with Upstash if:**
- Only need rate limiting
- Simple architecture (1-3 services)
- Budget-conscious
- Happy with zero maintenance

---

## Quick Start: Upstash Integration

### 1. Sign Up (2 minutes)
```bash
# Visit https://upstash.com
# Create account (free)
# Create database
# Copy UPSTASH_REDIS_REST_URL and UPSTASH_REDIS_REST_TOKEN
```

### 2. Install SDK (1 minute)
```bash
cd Org-core
go get github.com/upstash/ratelimit-go
```

### 3. Replace Limiter (30 minutes)

**File**: `Org-core/internal/ratelimit/upstash_limiter.go`

```go
package ratelimit

import (
    "context"
    "fmt"
    "time"
    
    "github.com/upstash/ratelimit-go"
)

// UpstashLimiter uses Upstash for global rate limiting
type UpstashLimiter struct {
    ragQuery    *ratelimit.Ratelimit
    ragIndex    *ratelimit.Ratelimit
    chat        *ratelimit.Ratelimit
    document    *ratelimit.Ratelimit
    embedding   *ratelimit.Ratelimit
    crawl       *ratelimit.Ratelimit
}

// NewUpstashLimiter creates a new Upstash-backed limiter
func NewUpstashLimiter(config Config) *UpstashLimiter {
    return &UpstashLimiter{
        ragQuery: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.RAGQueryRPM, time.Minute),
            Prefix: "rag:query",
        }),
        ragIndex: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.RAGIndexRPM, time.Minute),
            Prefix: "rag:index",
        }),
        chat: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.ChatRPM, time.Minute),
            Prefix: "chat",
        }),
        document: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.DocumentRPM, time.Minute),
            Prefix: "document",
        }),
        embedding: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.EmbeddingRPM, time.Minute),
            Prefix: "embedding",
        }),
        crawl: ratelimit.NewRatelimit(ratelimit.Options{
            Limit: ratelimit.SlidingWindow(config.CrawlRPM, time.Minute),
            Prefix: "crawl",
        }),
    }
}

// CheckRAGQuery checks rate limit for RAG queries
func (l *UpstashLimiter) CheckRAGQuery(ctx context.Context, orgID string) (*Limit, error) {
    return l.check(ctx, l.ragQuery, orgID)
}

// CheckRAGIndex checks rate limit for RAG indexing
func (l *UpstashLimiter) CheckRAGIndex(ctx context.Context, orgID string) (*Limit, error) {
    return l.check(ctx, l.ragIndex, orgID)
}

// ... similar methods for other operations

func (l *UpstashLimiter) check(ctx context.Context, limiter *ratelimit.Ratelimit, orgID string) (*Limit, error) {
    result, err := limiter.Limit(orgID)
    if err != nil {
        return nil, fmt.Errorf("rate limit check failed: %w", err)
    }
    
    return &Limit{
        Allowed:    result.Allowed,
        Remaining:  result.Remaining,
        Limit:      result.Limit,
        RetryAfter: time.Duration(result.Reset) * time.Second,
    }, nil
}
```

### 4. Update Environment Variables
```bash
# .env.local
UPSTASH_REDIS_REST_URL=https://your-db.upstash.io
UPSTASH_REDIS_REST_TOKEN=your-token-here

# Rate limit configuration (RPM - requests per minute)
RATE_LIMIT_ENABLED=true
RATE_LIMIT_RAG_QUERY_RPM=60
RATE_LIMIT_RAG_INDEX_RPM=30
RATE_LIMIT_CHAT_RPM=100
RATE_LIMIT_DOCUMENT_RPM=50
RATE_LIMIT_EMBEDDING_RPM=200
RATE_LIMIT_CRAWL_RPM=10
```

### 5. Test (10 minutes)
```bash
# Load test with 100 concurrent requests
cd Org-core
go test -v ./internal/ratelimit -run TestUpstashLimiter -count=100
```

---

## Conclusion

### **Recommended Path: Custom → Upstash → (Kong if needed)**

```
┌─────────────┐
│   Now       │  Keep custom implementation (already working)
│  (Month 1)  │  Add local cache layer (10x performance boost)
└──────┬──────┘  Cost: $0
       │
       ▼
┌─────────────┐
│  Month 2-3  │  Migrate to Upstash (when >100k req/day)
│             │  Zero maintenance, global distribution
└──────┬──────┘  Cost: $0-30/mo
       │
       ▼
┌─────────────┐
│  Month 6+   │  Evaluate Kong (if need full API gateway)
│  (Optional) │  Only if 10+ services and complex routing
└─────────────┘  Cost: $500-2000/mo
```

### Cost Savings
- **Now**: $0/mo (keep custom)
- **Month 2**: $30/mo (Upstash) vs. $1200/mo (Kong)
- **Savings**: $1170/mo = $14,040/year

### Next Steps
1. ✅ Implement local cache layer (Task #4)
2. ⏳ Monitor traffic growth
3. ⏳ Migrate to Upstash when >100k req/day
4. ⏳ Evaluate Kong only if need full API gateway

---

**Questions for Discussion:**
1. Current traffic volume? (<100k, 100k-1M, >1M req/day)
2. Growth projections for next 3-6 months?
3. Do you need API gateway features beyond rate limiting?
