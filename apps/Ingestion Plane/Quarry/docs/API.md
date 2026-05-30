# Quarry API Documentation

**Version:** 0.1.0  
**Base URL:** `http://localhost:8090` (development) | `https://api.quarry.example.com` (production)  
**Authentication:** API Key via `X-API-Key` header

---

## Table of Contents

1. [Introduction](#introduction)
2. [Authentication](#authentication)
3. [Endpoints](#endpoints)
   - [Health & System](#health--system)
   - [Scraping](#scraping)
   - [Crawling](#crawling)
   - [Discovery](#discovery)
   - [Extraction](#extraction)
   - [Jobs](#jobs)
4. [Request/Response Formats](#requestresponse-formats)
5. [Error Handling](#error-handling)
6. [Rate Limiting](#rate-limiting)
7. [Examples](#examples)

---

## Introduction

Quarry is a high-performance web scraper API that provides:
- **Intelligent Extraction**: AI-powered content extraction with automatic fallback
- **Multiple Modules**: Quick scraping, SEO analysis, multi-page crawling
- **Format Flexibility**: JSON, Markdown, HTML, Screenshots
- **Async Workflows**: Durable Temporal-based orchestration for large jobs
- **Enterprise Security**: Multi-provider security scanning, rate limiting, API key auth

---

## Authentication

All API requests require an API key passed via the `X-API-Key` header:

```bash
curl -H "X-API-Key: your-api-key-here" \
  https://api.quarry.example.com/v1/scrape
```

**Default Development Key:** `dev-test-key-12345`

---

## Endpoints

### Health & System

#### `GET /health`
Check if the API service is running.

**Response:**
```json
{
  "success": true,
  "service": "quarry",
  "status": "ok"
}
```

#### `GET /ready`
Check if all dependencies (cache, database, ai-core) are ready.

**Response:**
```json
{
  "success": true,
  "status": {
    "api": "ready",
    "cache": "ready",
    "redis": "ready",
    "postgres": "ready",
    "ai_core": "ready",
    "artifacts": "ready"
  }
}
```

#### `GET /metrics`
Get real-time operational metrics.

**Response:**
```json
{
  "success": true,
  "metrics": {
    "requests_total": 1523,
    "errors_total": 12,
    "requests_per_sec": 2.45,
    "avg_response_time_ms": 145.3,
    "error_rate": 0.008,
    "cache_hit_rate": 0.73
  },
  "ai": {
    "enabled": true,
    "health": {
      "healthy": true,
      "last_checked_unix": 1771433123,
      "last_latency_ms": 25.4
    },
    "circuit": {
      "state": "closed",
      "failure_threshold": 5,
      "consecutive_failures": 0
    },
    "slo": {
      "success_rate": 0.98,
      "avg_latency_ms": 127.5
    }
  },
  "ai_efficiency": {
    "hits": 1250,
    "misses": 450,
    "hit_ratio": 0.74,
    "estimated_ai_calls_saved": 1250
  }
}
```

#### `GET /v1/modules`
List available scraping modules.

**Response:**
```json
{
  "success": true,
  "modules": ["quick", "seo", "multi"],
  "default": "multi"
}
```

---

### Scraping

#### `POST /v1/scrape`
Synchronous single-page scraping (fast path, <5s response).

**Request:**
```json
{
  "url": "https://example.com",
  "collection": "quick",
  "formats": ["markdown", "json"],
  "onlyMainContent": true,
  "waitFor": 1000,
  "headers": {
    "User-Agent": "Custom Agent"
  }
}
```

**Parameters:**
- `url` (required): Target URL (must be absolute HTTP/HTTPS)
- `collection` (optional): Module to use (`quick`, `seo`, `multi`), default: inferred
- `formats` (optional): Output formats array, default: `["json"]`
  - Supported: `json`, `markdown`, `html`, `rawHtml`, `screenshot`, `pdf`, `links`
- `onlyMainContent` (optional): Strip boilerplate, default: `false`
- `waitFor` (optional): Wait milliseconds before extracting, default: `0`
- `headers` (optional): Custom HTTP headers
- `actions` (optional): Array of actions to perform before scraping (see below)

**Actions:**
```json
{
  "actions": [
    {"type": "wait", "milliseconds": 2000},
    {"type": "click", "selector": "#accept-cookies"},
    {"type": "scroll", "y": 1000},
    {"type": "type", "selector": "#search", "text": "query"},
    {"type": "screenshot"}
  ]
}
```

**Response:**
```json
{
  "success": true,
  "data": {
    "url": "https://example.com",
    "title": "Example Domain",
    "markdown": "# Example Domain\n\nThis domain is for use...",
    "metadata": {
      "title": "Example Domain",
      "description": "Example domain for documentation",
      "statusCode": 200,
      "contentType": "text/html"
    },
    "links": [
      "https://www.iana.org/help/example-domains"
    ]
  },
  "formats": ["markdown", "json"],
  "extractionMethod": "ai",
  "latencyMs": 145
}
```

---

### Crawling

#### `POST /v1/crawl`
Asynchronous multi-page crawling (returns job ID immediately).

**Request:**
```json
{
  "url": "https://example.com",
  "collection": "multi",
  "maxPages": 10,
  "maxDepth": 2,
  "includePaths": ["/blog/*"],
  "excludePaths": ["/admin/*"],
  "formats": ["markdown"],
  "webhook": {
    "url": "https://your-app.com/webhook",
    "events": ["crawl.completed", "crawl.failed"]
  }
}
```

**Parameters:**
- `url` (required): Starting URL
- `collection` (optional): Module, default: `multi`
- `maxPages` (optional): Max pages to crawl, default: `50`, max: `1000`
- `maxDepth` (optional): Max link depth, default: `3`
- `includePaths` (optional): URL patterns to include
- `excludePaths` (optional): URL patterns to exclude
- `formats` (optional): Output formats
- `webhook` (optional): Webhook delivery config

**Response:**
```json
{
  "success": true,
  "job_id": "crawl_abc123xyz",  "status": "started",
  "estimatedPages": 45
}
```

---

### Discovery

#### `POST /v1/map`
Discover all URLs on a website (sitemap + crawling).

**Request:**
```json
{
  "url": "https://example.com",
  "limit": 100,
  "ignoreSitemap": false
}
```

**Parameters:**
- `url` (required): Base URL
- `limit` (optional): Max URLs to return, default: `100`, max: `1000`
- `ignoreSitemap` (optional): Skip sitemap.xml, default: `false`

**Response:**
```json
{
  "success": true,
  "urls": [
    "https://example.com/",
    "https://example.com/about",
    "https://example.com/contact"
  ],
  "total": 3,
  "limit": 100,
  "sources": ["sitemap", "crawl"]
}
```

#### `POST /v1/search`
Search a site's discovered links or, when `sources` is provided, query external and internal source families including `web`, `news`, `images`, `github`, `documents`, and `index`.

**Request:**
```json
{
  "query": "machine learning",
  "blendMode": "ranked",
  "limit": 20,
  "site": "example.com",
  "sources": [
    "web",
    {"type": "github", "site": "triodelab", "weight": 2, "limit": 5}
  ]
}
```

**Parameters:**
- `query` (required): Search query
- `blendMode` (optional): `ranked` or `interleave`; defaults to `ranked`
- `limit` (optional): Max results, default: `20`, max: `200`
- `url` (optional): Absolute site URL for local link search or Brave domain restriction
- `site` (optional): Legacy alias for `url`; accepts a bare hostname like `example.com`
- `includeSubdomains` (optional): Include subdomains for local site-link search
- `sources` (optional): Search sources. Supported values: `web`, `news`, `images`, `github`, `documents`, `index`
- `sources[].weight` (optional): Relative weight applied during ranked blending; defaults to `1`
- `sources[].limit` (optional): Per-source cap applied before cross-source blending

**Behavior:**
- If `sources` is omitted, Quarry performs site-local search against extracted links from the supplied `url` or `site`.
- If `sources` is present, Quarry queries the configured backend for each requested source family and returns normalized result objects in the same order as the requested sources.
- `blendMode=ranked` applies source weights and de-duplicates repeated URLs before truncating to the final `limit`.
- `blendMode=interleave` alternates sources in request order, still suppressing duplicates.
- `web`, `news`, and `images` use Brave Search and require `BRAVE_SEARCH_API_KEY`. Optional overrides: `BRAVE_SEARCH_BASE_URL`, `BRAVE_SEARCH_TIMEOUT_SEC`.
- `github` uses the GitHub search API. Optional overrides: `GITHUB_API_BASE_URL`, `GITHUB_TOKEN`, `GITHUB_TIMEOUT_SEC`.
- `documents` and `index` use the Data Plane retrieval service and require `RETRIEVAL_BASE_URL` plus `DATAPLANE_INTERNAL_API_KEY`.
- Brave currently caps each upstream request to `20` results per source even if a larger `limit` is requested.

**Response:**
```json
{
  "success": true,
  "query": "machine learning",
  "count": 2,
  "results": [
    {
      "title": "Machine Learning Guide",
      "url": "https://example.com/ml-guide",
      "snippet": "Comprehensive guide to ML...",
      "source": "brave",
      "type": "web"
    }
  ],
  "query": "machine learning"
}
```

---

### Extraction

#### `POST /v1/extract`
Queue asynchronous single-page structured extraction. The page is fetched once and then passed to the AI extraction pipeline.

**Request:**
```json
{
  "url": "https://example.com/product",
  "schema": {
    "type": "object",
    "properties": {
      "name": {"type": "string"},
      "price": {"type": "string"}
    }
  },
  "timeout": 30,
  "maxAge": 60000
}
```

**Parameters:**
- `url` (required): Absolute HTTP/HTTPS URL to extract from
- `schema` (optional): JSON object or JSON-encoded string containing the extraction schema
- `prompt` (optional): Natural-language extraction prompt. Either `schema` or `prompt` is required
- `timeout` (optional): Extraction timeout in seconds, default: `30`, max: `300`
- `maxAge` (optional): Cache age in milliseconds for page reuse

**Response:**
```json
{
  "success": true,
  "jobId": "extract_a1b2c3d4",
  "status": "queued",
  "estimatedWaitTime": 5
}
```

#### `GET /v1/extract/:id`
Poll the current status of an extraction job.

**Response:**
```json
{
  "success": true,
  "jobId": "extract_a1b2c3d4",
  "status": "completed",
  "result": {
    "name": "Example Product",
    "price": "$19.99",
    "source_url": "https://example.com/product",
    "cached": true,
    "extracted_at": "2026-03-15T11:32:10Z"
  },
  "duration_ms": 842,
  "expires_at": "2026-03-15T12:32:10Z"
}
```

**Status values:**
- `queued`
- `processing`
- `completed`
- `failed`

#### `POST /v1/extract/:id/ingest`
Ingest a completed extraction job into the data plane document service.

**Request:**
```json
{
  "org_id": "org_123"
}
```

**Response:**
```json
{
  "success": true,
  "jobId": "extract_a1b2c3d4",
  "documentId": "doc_456",
  "status": "ingested"
}
```

---

### Batch Processing

#### `POST /v1/batch`
Process multiple URLs in parallel with webhook delivery.

**Request:**
```json
{
  "urls": [
    "https://example.com",
    "https://example.org"
  ],
  "collection": "quick",
  "formats": ["json"],
  "webhook": {
    "url": "https://your-app.com/webhook",
    "secret": "your-webhook-secret",
    "events": ["batch.item.completed", "batch.completed"]
  },
  "maxConcurrency": 5
}
```

**Parameters:**
- `urls` (required): Array of URLs, max: `200`
- `collection`, `formats`: Same as `/v1/scrape`
- `webhook` (optional): Webhook delivery
  - `url`: Webhook endpoint (must be HTTPS)
  - `secret`: HMAC signature secret
  - `events`: Array of events to subscribe to
- `maxConcurrency` (optional): Parallel scrapes, default: `5`

**Response:**
```json
{
  "success": true,
  "batch_id": "batch_xyz789",
  "total_urls": 2,
  "status": "processing"
}
```

#### `GET /v1/batch/:id`
Get batch job status.

**Response:**
```json
{
  "success": true,
  "batch_id": "batch_xyz789",
  "status": "completed",
  "total": 2,
  "completed": 2,
  "failed": 0,
  "results": [
    {
      "url": "https://example.com",
      "status": "completed",
      "data": {...}
    }
  ]
}
```

---

### Jobs

#### `GET /v1/jobs/:id`
Get job status for async operations (crawl, batch).

**Response:**
```json
{
  "success": true,
  "job_id": "crawl_abc123",
  "status": "running",
  "progress": {
    "current": 25,
    "total": 50,
    "percent": 50
  },
  "created_at": "2026-02-18T16:00:00Z",
  "updated_at": "2026-02-18T16:05:00Z"
}
```

**Status Values:**
- `pending`: Job queued
- `running`: In progress
- `completed`: Finished successfully
- `failed`: Error occurred
- `cancelled`: User cancelled

#### `GET /v1/jobs/:id/stream`
Stream job progress via Server-Sent Events (SSE).

**Response (SSE):**
```
event: progress
data: {"current": 10, "total": 50, "percent": 20}

event: page_completed
data: {"url": "https://example.com/page1", "status": "success"}

event: completed
data: {"total_pages": 50, "duration_ms": 45000}
```

---

## Request/Response Formats

### Output Formats

- **`json`**: Structured JSON (default)
- **`markdown`**: Clean Markdown with main content only
- **`html`**: Sanitized HTML
- **`rawHtml`**: Original HTML (minimal processing)
- **`screenshot`**: PNG screenshot bytes, or an artifact descriptor when object storage is enabled
- **`pdf`**: Generated PDF bytes, or an artifact descriptor when object storage is enabled
- **`links`**: Array of extracted URLs

When `ARTIFACT_STORE_BACKEND=minio`, Quarry replaces large binary payloads with:

```json
{
  "kind": "screenshot",
  "provider": "minio",
  "bucket": "quarry-artifacts",
  "key": "quarry/2026/03/15/example.com/screenshot/uuid.png",
  "url": "http://localhost:9010/quarry-artifacts/quarry/2026/03/15/example.com/screenshot/uuid.png",
  "contentType": "image/png",
  "size": 182044,
  "etag": "..."
}
```

### Standard Error Response

```json
{
  "success": false,
  "error": "Invalid URL format",
  "requestId": "req_abc123xyz",
  "details": {
    "field": "url",
    "reason": "must be absolute HTTP/HTTPS URL"
  }
}
```

---

## Error Handling

### HTTP Status Codes

- `200 OK`: Request succeeded
- `400 Bad Request`: Invalid request parameters
- `401 Unauthorized`: Missing or invalid API key
- `408 Request Timeout`: Request exceeded timeout limit
- `429 Too Many Requests`: Rate limit exceeded
- `500 Internal Server Error`: Server error
- `503 Service Unavailable`: Service or dependency unavailable

### Error Response Fields

- `success`: Always `false`
- `error`: Human-readable error message
- `requestId`: Unique request identifier for debugging
- `details`: Additional error context (optional)

---

## Rate Limiting

- **Per API Key**: `100 requests/minute`
- **Per IP (unauthenticated)**: `20 requests/minute`
- **Retry After**: Check `Retry-After` header when rate limited

**Rate Limit Response:**
```json
{
  "success": false,
  "error": "rate limit exceeded",
  "requestId": "req_xyz789"
}
```

**HTTP Headers:**
- `X-RateLimit-Limit`: Total requests allowed
- `X-RateLimit-Remaining`: Requests remaining
- `X-RateLimit-Reset`: Unix timestamp when limit resets

---

## Examples

### Example 1: Quick Scrape with Markdown

```bash
curl -X POST http://localhost:8090/v1/scrape \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "collection": "quick",
    "formats": ["markdown"]
  }'
```

### Example 2: Crawl with Webhook

```bash
curl -X POST http://localhost:8090/v1/crawl \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://blog.example.com",
    "maxPages": 20,
    "webhook": {
      "url": "https://your-app.com/webhooks/crawl",
      "events": ["crawl.completed"]
    }
  }'
```

### Example 3: Actions (Login Flow)

```bash
curl -X POST http://localhost:8090/v1/scrape \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://app.example.com/login",
    "actions": [
      {"type": "type", "selector": "#username", "text": "user@example.com"},
      {"type": "type", "selector": "#password", "text": "password123"},
      {"type": "click", "selector": "#login-btn"},
      {"type": "wait", "milliseconds": 2000}
    ],
    "formats": ["json", "screenshot"]
  }'
```

### Example 4: Stream Job Progress

```bash
curl -N http://localhost:8090/v1/jobs/crawl_abc123/stream \
  -H "X-API-Key: dev-test-key-12345"
```

---

## Best Practices

1. **Use Appropriate Modules**:
   - `quick`: Single-page, fast extraction (<5s)
   - `seo`: SEO analysis, meta tags, structured data
   - `multi`: Multi-page crawling with depth control

2. **Enable Webhooks for Async Jobs**:
   - Don't poll `/v1/jobs/:id` frequently
   - Use webhooks for production workloads

3. **Leverage Caching**:
   - Quarry caches AI responses and page content
   - Repeated requests are ~500% faster

4. **Use Actions for JavaScript-heavy Sites**:
   - Wait for content to load
   - Handle cookie banners, popups
   - Trigger lazy-loaded content

5. **Request Only Needed Formats**:
   - Each format adds processing time
   - `screenshot` is the slowest format

---

## Webhook Signatures

Webhook payloads include `X-Webhook-Signature` header (HMAC-SHA256):

```python
import hmac
import hashlib

def verify_signature(payload, signature, secret):
    expected = hmac.new(
        secret.encode(),
        payload.encode(),
        hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(signature, expected)
```

---

## SDKs & Clients

- **Go**: `go get github.com/triodelab/quarry/pkg/client`
- **Python**: Coming Soon
- **Node.js**: Coming Soon
- **REST**: Use any HTTP client

---

## Support

- **Documentation**: [docs.quarry.example.com](https://docs.quarry.example.com)
- **GitHub**: [github.com/triodelab/quarry](https://github.com/triodelab/quarry)
- **Issues**: [github.com/triodelab/quarry/issues](https://github.com/triodelab/quarry/issues)

---

**Last Updated:** 2026-02-18
