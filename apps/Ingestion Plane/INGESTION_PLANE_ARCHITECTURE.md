# Ingestion Plane Architecture

**Project:** Triodelab CoreSystem - Ingestion Plane  
**Version:** 1.0  
**Status:** Hybrid (Production + Development)  
**Last Updated:** April 8, 2026

## Pyramid Placement

The Ingestion Plane is a **capability consumer** in the CoreSystem pyramid.

It sits above Control Plane and Data Plane:

- It authenticates, authorizes, and validates org/user context through Control Plane.
- It lands imported, discovered, or synchronized content into Data Plane.
- It may use Model Plane capabilities for extraction or transformation, but it does not own model-runtime authority.

### Authority Rules

- Ingestion Plane is not the source of truth for auth, users, orgs, billing, entitlements, quotas, or sessions.
- Ingestion Plane is not the canonical owner of product documents, retrieval indexes, or grounding state.
- Ingestion Plane owns ingestion workflows, connector execution, crawl jobs, and import orchestration.
- Results produced by ingestion should be persisted through Data Plane contracts, not written into another plane's private database.

---

## Executive Summary

The Ingestion Plane is a comprehensive data acquisition layer that enables organizations to ingest data from multiple sources:
- **Web Scraping** (Quarry) - Production-ready
- **File Imports** (imports-core) - Active development
- **External Integrations** (Part of imports-core) - Active development

**Architecture Pattern:** Microservices with event-driven orchestration  
**Technology Stack:** Go (Quarry), Python (Imports), PostgreSQL, Redis, Temporal, NATS  
**Deployment:** Docker Compose (development), Kubernetes-ready (production)

## Downstream Contract Direction

- **Control Plane** provides auth, membership, quota, entitlement, and billing context.
- **Data Plane** provides canonical storage for imported/discovered product data.
- **Application Plane** may mirror ingestion progress for user experiences, but is not the durable owner.

---

## System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         INGESTION PLANE                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────────────┐   │
│  │   QUARRY     │  │ IMPORTS-CORE │  │  EXTERNAL INTEGRATIONS  │   │
│  │  (Crawler)   │  │  (Parsers)   │  │    (Connectors)         │   │
│  │              │  │              │  │                         │   │
│  │ • Web Scrape │  │ • CSV/Excel  │  │ • Notion                │   │
│  │ • AI Extract │  │ • PDF/DOCX   │  │ • HubSpot               │   │
│  │ • Map/Search │  │ • Text/JSON  │  │ • Salesforce            │   │
│  │ • Batch Jobs │  │ • Bulk Import│  │ • Odoo                  │   │
│  │              │  │              │  │ • Generic HTTP/REST     │   │
│  │ Port: 8090   │  │ Port: 3041   │  │ (via imports-core)      │   │
│  │ Lang: Go     │  │ Lang: Python │  │                         │   │
│  │ Status: ✅   │  │ Status: 🚧   │  │ Status: 🚧              │   │
│  └──────┬───────┘  └──────┬───────┘  └───────────┬─────────────┘   │
│         │                 │                       │                 │
│         └─────────────────┴───────────────────────┘                 │
│                            │                                         │
│                            ▼                                         │
│                  ┌─────────────────────┐                            │
│                  │  SHARED SERVICES    │                            │
│                  │                     │                            │
│                  │ • PostgreSQL        │                            │
│                  │ • Redis Cache       │                            │
│                  │ • Temporal          │                            │
│                  │ • NATS              │                            │
│                  │ • Qdrant (Vector)   │                            │
│                  └─────────────────────┘                            │
│                                                                       │
└─────────────────────────────────────────────────────────────────────┘
          │                              │
          ▼                              ▼
┌─────────────────────┐        ┌─────────────────────┐
│   CONTROL PLANE     │        │     DATA PLANE      │
│  (authority checks) │        │  (canonical storage)│
│                     │        │                     │
│ • Auth / sessions   │        │ • Documents         │
│ • Org membership    │        │ • Retrieval/indexes │
│ • Entitlements      │        │ • Knowledge state   │
│ • Quotas / billing  │        │ • Tasks/jobs data   │
└─────────────────────┘        └─────────────────────┘
```

---

## Component Details

### 1. Quarry (Web Crawler Service)

**Purpose:** Enterprise-grade web scraping with AI-powered extraction

**Status:** ✅ **Production Ready** (v0.1.0 - Phase 4 Complete)

**Technology:**
- **Language:** Go 1.24.1
- **Framework:** Fiber (HTTP), Temporal (Orchestration)
- **Browser:** Rod (headless Chrome)
- **AI:** gRPC client to ai-core service

**Capabilities:**
| Feature | Description | Status |
|---------|-------------|--------|
| `/v1/scrape` | Single-page scraping (<500ms) | ✅ |
| `/v1/crawl` | Multi-page crawling (async) | ✅ |
| `/v1/map` | URL discovery | ✅ |
| `/v1/search` | Web search integration | ✅ |
| `/v1/batch` | Batch processing (100+ URLs) | ✅ |
| AI Extraction | Intelligent content extraction | ✅ |
| Security Scanning | 5-provider URL validation | ✅ |
| Change Tracking | Git-style content diffing | ✅ |
| Output Formats | JSON, Markdown, HTML, Screenshot, TOON | ✅ |

**Architecture Highlights:**
- **Security:** 5-provider consensus scanning (URLhaus, PhishTank, Google Safe Browsing, AbuseIPDB, Heuristics)
- **Performance:** Sub-100ms latency (p50), browser pooling (10x speedup)
- **Cost Efficiency:** 70% AI cost reduction via caching + TOON encoding
- **Resilience:** Circuit breaker, automatic fallback to heuristic extraction
- **Orchestration:** Dual execution (immediate + Temporal-based scheduled)

**Key Files:**
```
Quarry/
├── cmd/
│   ├── api/main.go           # HTTP API server
│   └── worker/main.go        # Temporal worker
├── internal/
│   ├── api/                  # HTTP endpoints
│   ├── scraper/              # Core scraping logic
│   ├── ai/                   # AI extraction client
│   ├── security/             # URL scanning
│   ├── batch/                # Batch manager
│   ├── temporal/             # Workflow definitions
│   └── cache/                # Multi-layer caching
├── docs/
│   ├── API.md               # Complete API reference
│   ├── ARCHITECTURE.md      # System design
│   ├── DEPLOYMENT.md        # Deployment guide
│   └── SECURITY.md          # Security documentation
└── docker-compose.yml       # Full stack deployment
```

**API Examples:**

```bash
# Scrape single page with AI extraction
curl -X POST http://localhost:8090/v1/scrape \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com/article",
    "formats": ["markdown", "screenshot"],
    "onlyMainContent": true,
    "includeExtractedData": true
  }'

# Start async crawl job
curl -X POST http://localhost:8090/v1/crawl \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://docs.example.com",
    "maxDepth": 3,
    "limit": 100,
    "webhook": "https://your-app.com/webhook"
  }'
```

**Deployment:**
- **Docker Compose:** 8 services (API, Worker, Temporal, Temporal UI, PostgreSQL, Redis, Qdrant, NATS)
- **Ports:** 8090 (API), 8089 (Temporal UI), 7234 (Temporal), 5434 (PostgreSQL), 6380 (Redis)
- **Health Probes:** `/health` (liveness), `/ready` (readiness)
- **Metrics:** `/metrics` (Prometheus-compatible)

**Performance Metrics:**
- **Latency:** <100ms (health endpoint), <500ms (scrape with AI)
- **Throughput:** ~50-100 req/sec (single instance)
- **Memory:** ~100MB idle, ~300MB under load
- **Cost Savings:** 99.7% vs Firecrawl SaaS ($100K/mo → $280/mo)

---

### 2. Imports-Core (File Import Service)

**Purpose:** Parse and import structured/unstructured files at scale

**Status:** 🚧 **Active Development**

**Technology:**
- **Language:** Python 3.11+
- **Framework:** FastAPI
- **ORM:** SQLAlchemy (async)
- **Orchestration:** Temporal (Python SDK)
- **Messaging:** NATS (event publishing)

**Capabilities:**
| Feature | Description | Status |
|---------|-------------|--------|
| CSV/Excel Import | Parse tabular data | 🚧 |
| PDF Parsing | Extract text from PDFs | 🚧 |
| DOCX Parsing | Word document processing | 🚧 |
| JSON/Text Import | Structured/unstructured text | 🚧 |
| Bulk Upload | Multi-file batch processing | 🚧 |
| Progress Tracking | Real-time import status | 🚧 |
| SSE Streaming | Server-sent events for progress | 🚧 |
| Quota Enforcement | Integration with org-core | 🚧 |

**Architecture:**

```python
# High-Level Flow
1. Client uploads file → FastAPI endpoint
2. File validated & parsed → Parser selection (CSV/PDF/DOCX)
3. Job created → PostgreSQL job store
4. Orchestrator dispatches → Temporal workflow (optional)
5. Documents chunked & processed → Batch processing
6. Events published → NATS stream
7. Results sent to Data Plane → canonical document / knowledge storage
8. Progress updates → SSE to client
```

---

## Cross-Plane Contract Rules

### Allowed Calls

- `imports-core` may call Control Plane to validate auth, org membership, entitlements, and quotas.
- `integration-core` may call Control Plane to validate user/org ownership and provider/session context.
- `Quarry` may call Model Plane or AI services for extraction when needed.
- Ingestion services may call Data Plane to create or update canonical product data.

### Allowed Event Flows

- Control Plane → Ingestion Plane: user/org/billing/entitlement events that affect connector access or quota behavior
- Ingestion Plane → Data Plane: import, crawl, sync, and content-ready events
- Ingestion Plane → Application Plane / Frontend Plane: progress and status events for UX only

### Forbidden Patterns

- No Ingestion service writes directly into Control Plane databases.
- No Ingestion service treats org-core as canonical document storage.
- No Ingestion service becomes the durable owner of retrieval, vector, or grounding state.
- No cross-plane integration may rely on another plane's private schema or private network as the contract boundary.

**Key Files:**
```
imports-core/
├── app/
│   ├── main.py              # FastAPI application
│   ├── config.py            # Settings (env vars)
│   ├── service.py           # Import business logic
│   ├── orchestration.py     # Temporal client wrapper
│   ├── parsers.py           # File parsers (PDF, CSV, DOCX)
│   ├── connectors.py        # External integration connectors
│   ├── db.py                # Database session management
│   ├── models.py            # SQLAlchemy models
│   ├── schemas.py           # Pydantic request/response schemas
│   ├── events.py            # NATS event publisher
│   └── progress.py          # SSE progress hub
├── migrations/
│   └── 001_init.sql         # Database schema
└── requirements.txt         # Python dependencies
```

**API Endpoints:**

```python
# File Upload
POST /v1/import/file
- Accepts: multipart/form-data
- Supports: .csv, .xlsx, .pdf, .docx, .txt, .json
- Returns: Job ID + progress stream URL

# Bulk Import from Source
POST /v1/import/source
- Payload: source_type, connection, options
- Triggers: External connector (Notion, HubSpot, etc.)
- Returns: Job ID

# Job Status
GET /v1/import/jobs/{job_id}
- Returns: Job status, progress, errors

# Job Progress Stream (SSE)
GET /v1/import/jobs/{job_id}/stream
- Stream: Server-Sent Events
- Events: progress, item_processed, completed, failed
```

**Database Schema:**

```sql
-- Import Jobs
CREATE TABLE import_jobs (
    id UUID PRIMARY KEY,
    org_id VARCHAR NOT NULL,
    user_id VARCHAR,
    source_type VARCHAR NOT NULL,    -- 'file', 'notion', 'hubspot', etc.
    status VARCHAR NOT NULL,          -- 'queued', 'processing', 'completed', 'failed'
    total_items INTEGER DEFAULT 0,
    processed_items INTEGER DEFAULT 0,
    created_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP,
    metadata JSONB
);

-- Import Job Items (individual documents)
CREATE TABLE import_job_items (
    id UUID PRIMARY KEY,
    job_id UUID REFERENCES import_jobs(id),
    source_id VARCHAR,               -- External ID (notion page id, etc.)
    title VARCHAR,
    status VARCHAR NOT NULL,          -- 'pending', 'completed', 'failed'
    error_message TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    metadata JSONB
);
```

**Parsers Implemented:**

```python
# app/parsers.py - File type detection and parsing

from pathlib import Path
import pandas as pd
import PyPDF2
import docx

async def parse_uploaded_file(file: UploadFile) -> list[ImportDocument]:
    """Auto-detect file type and parse accordingly"""
    ext = Path(file.filename).suffix.lower()
    
    if ext in {'.csv', '.xlsx'}:
        return await parse_tabular(file)
    elif ext == '.pdf':
        return await parse_pdf(file)
    elif ext == '.docx':
        return await parse_docx(file)
    elif ext == '.json':
        return await parse_json(file)
    elif ext == '.txt':
        return await parse_text(file)
    else:
        raise UnsupportedFileTypeError(ext)
```

**Event Publishing:**

```python
# app/events.py - NATS event streaming

class EventPublisher:
    async def publish_import_completed(self, job_id: UUID, stats: dict):
        await self.nats.publish(
            subject="import.completed",
            payload={
                "job_id": str(job_id),
                "total_items": stats["total"],
                "processed_items": stats["processed"],
                "timestamp": datetime.now(timezone.utc).isoformat()
            }
        )
```

---

### 3. External Integrations (Connector Library)

**Purpose:** Connect to third-party APIs and sync data into the platform

**Status:** 🚧 **Active Development** (part of imports-core)

**Location:** `imports-core/app/connectors.py`

**Supported Integrations:**

| Source | Type | SDK | Status | Authentication |
|--------|------|-----|--------|----------------|
| **Notion** | Knowledge Base | `notion-client` | 🚧 | API Token |
| **HubSpot** | CRM | `hubspot-api-client` | 🚧 | Access Token (OAuth) |
| **Salesforce** | CRM | `simple-salesforce` | 🚧 | Username + Password + Security Token |
| **Odoo** | ERP/PIM | `OdooRPC` | 🚧 | Database + Username + Password |
| **Generic HTTP/REST** | Custom | `httpx` | 🚧 | Custom Headers |

**Integration Architecture:**

```python
# Unified Connector Interface
async def import_from_source(
    source_type: str,
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Unified connector interface
    
    Args:
        source_type: 'notion', 'hubspot', 'salesforce', etc.
        connection: Authentication credentials
        options: Query/filter parameters
    
    Returns:
        List of ImportDocument objects
    """
```

**Notion Connector:**

```python
async def import_from_notion(
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Import pages/databases from Notion
    
    Connection:
        - token: Notion integration token
    
    Options:
        - filter: Search filter query
        - sort: Sort parameters
    
    Returns:
        Documents with title, text, metadata
    """
    notion = NotionClient(auth=connection.get("token"))
    result = notion.search(filter=options.get("filter"), sort=options.get("sort"))
    
    documents = []
    for item in result.get("results", []):
        # Extract title from properties
        title = extract_title_from_properties(item.get("properties", {}))
        documents.append(ImportDocument(
            source_id=item.get("id"),
            source_name=title,
            title=title,
            text=str(item),  # Full object as text
            metadata={"source": "notion", "object": item.get("object")}
        ))
    return documents
```

**HubSpot Connector:**

```python
async def import_from_hubspot(
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Import contacts/deals/companies from HubSpot
    
    Connection:
        - access_token: OAuth access token
    
    Options:
        - object_type: 'contacts', 'deals', 'companies'
        - limit: Max records to fetch
    """
    client = HubSpot(access_token=connection.get("access_token"))
    object_type = options.get("object_type", "contacts")
    limit = int(options.get("limit", 100))
    
    response = client.crm.objects.basic_api.get_page(
        object_type=object_type,
        limit=limit
    )
    
    documents = [
        ImportDocument(
            source_id=item.id,
            source_name=item.id,
            title=item.id,
            text=str(item.to_dict()),
            metadata={"source": "hubspot", "object_type": object_type}
        )
        for item in response.results
    ]
    return documents
```

**Salesforce Connector:**

```python
async def import_from_salesforce(
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Import records via SOQL query
    
    Connection:
        - username: SF username
        - password: SF password
        - security_token: SF security token
        - domain: 'login' or 'test'
    
    Options:
        - query: SOQL query string
    """
    sf = Salesforce(
        username=connection.get("username"),
        password=connection.get("password"),
        security_token=connection.get("security_token"),
        domain=connection.get("domain", "login")
    )
    
    query = options.get("query", "SELECT Id, Name FROM Account LIMIT 100")
    result = sf.query_all(query)
    
    documents = [
        ImportDocument(
            source_id=record.get("Id"),
            source_name=record.get("Name") or record.get("Id"),
            title=record.get("Name") or record.get("Id"),
            text=str(record),
            metadata={"source": "salesforce", "query": query}
        )
        for record in result.get("records", [])
    ]
    return documents
```

**Odoo Connector:**

```python
async def import_from_odoo(
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Import records from Odoo ERP/PIM
    
    Connection:
        - host: Odoo server hostname
        - port: XML-RPC port (default: 8069)
        - database: Database name
        - username: Odoo username
        - password: Odoo password
    
    Options:
        - model: Odoo model (e.g., 'product.template', 'res.partner')
        - fields: List of field names to retrieve
        - domain: Search domain filter
        - limit: Max records
    """
    odoo = ODOO(connection.get("host"), port=int(connection.get("port", 8069)))
    odoo.login(
        connection.get("database"),
        connection.get("username"),
        connection.get("password")
    )
    
    model = options.get("model", "product.template")
    fields = options.get("fields", ["id", "name", "description"])
    domain = options.get("domain", [])
    limit = int(options.get("limit", 100))
    
    records = odoo.env[model].search_read(domain, fields, limit=limit)
    
    documents = [
        ImportDocument(
            source_id=str(record.get("id")),
            source_name=str(record.get("name") or record.get("id")),
            title=str(record.get("name") or record.get("id")),
            text=str(record),
            metadata={"source": "odoo", "model": model}
        )
        for record in records
    ]
    return documents
```

**Generic HTTP/REST Connector:**

```python
async def import_from_http_system(
    source_name: str,
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    """
    Import from any HTTP/REST API
    
    Connection:
        - url: API endpoint URL
        - headers: HTTP headers (authorization, etc.)
    
    Options:
        - params: Query parameters
    """
    url = connection.get("url")
    headers = connection.get("headers", {})
    params = options.get("params", {})
    
    async with httpx.AsyncClient(timeout=30.0) as client:
        response = await client.get(url, headers=headers, params=params)
        response.raise_for_status()
    
    payload = response.json()
    items = payload if isinstance(payload, list) else payload.get("items", [payload])
    
    documents = [
        ImportDocument(
            source_id=str(item.get("id") or item.get("uuid") or ""),
            source_name=str(item.get("name") or item.get("title") or "record"),
            title=str(item.get("name") or item.get("title") or "record"),
            text=str(item),
            metadata={"source": source_name, "url": url}
        )
        for item in items
    ]
    return documents
```

**Usage Example:**

```python
# Import from Notion
POST /v1/import/source
{
    "org_id": "org_12345",
    "user_id": "user_67890",
    "source_type": "notion",
    "connection": {
        "token": "secret_abc123..."
    },
    "options": {
        "filter": {"property": "Status", "select": {"equals": "Published"}},
        "sort": [{"timestamp": "created_time", "direction": "descending"}]
    }
}

# Import from HubSpot
POST /v1/import/source
{
    "org_id": "org_12345",
    "source_type": "hubspot",
    "connection": {
        "access_token": "oauth_token_xyz..."
    },
    "options": {
        "object_type": "contacts",
        "limit": 500
    }
}

# Import from Salesforce
POST /v1/import/source
{
    "org_id": "org_12345",
    "source_type": "salesforce",
    "connection": {
        "username": "admin@company.com",
        "password": "password123",
        "security_token": "abc123token",
        "domain": "login"
    },
    "options": {
        "query": "SELECT Id, Name, Email FROM Contact WHERE CreatedDate = THIS_YEAR"
    }
}
```

---

## Shared Infrastructure

### Database (PostgreSQL)

**Schemas:**

```
┌─────────────────────────────────────────────┐
│          PostgreSQL (Port 5434)             │
├─────────────────────────────────────────────┤
│  Databases:                                 │
│  - quarry           (Quarry jobs)           │
│  - temporal         (Temporal workflows)    │
│  - temporal_visibility (Temporal UI data)   │
│  - imports          (Import jobs)           │
└─────────────────────────────────────────────┘
```

**Quarry Schema:**
- `jobs` - Crawl/scrape job metadata
- `job_pages` - Individual page results
- `job_webhooks` - Webhook delivery status

**Imports Schema:**
- `import_jobs` - Import job metadata
- `import_job_items` - Individual document items

### Cache (Redis)

**Structure:**

```
┌─────────────────────────────────────────────┐
│           Redis (Port 6380)                 │
├─────────────────────────────────────────────┤
│  Namespaces:                                │
│  - quarry:cache:*       (Response cache)    │
│  - quarry:reputation:*  (URL security)      │
│  - quarry:change:*      (Change tracking)   │
│  - quarry:ai:plan:*     (AI plan cache)     │
│  - quarry:ai:extract:*  (AI extract cache)  │
│  - imports:progress:*   (Import progress)   │
└─────────────────────────────────────────────┘
```

**Cache Strategies:**
- **AI Plans:** 24h TTL (rarely change)
- **AI Extractions:** 1h TTL (more dynamic)
- **URL Reputation:** 15min TTL (security data)
- **Change Tracking:** 720h TTL (30 days)

### Orchestration (Temporal)

**Workflows:**

```
Quarry Workflows:
- CrawlWorkflow         (Multi-page crawling)
- BatchWorkflow         (Batch URL processing)
- MapWorkflow           (URL discovery)

Imports Workflows:
- FileImportWorkflow    (File parsing + processing)
- BulkImportWorkflow    (Multi-file batch)
- SourceSyncWorkflow    (External integration sync)
```

**Task Queues:**
- `quarry-task-queue` - Quarry worker tasks
- `import-task-queue` - Imports worker tasks

**Benefits:**
- Durable execution (survives crashes)
- Automatic retries with exponential backoff
- Workflow versioning
- Built-in monitoring (Temporal UI on port 8089)

### Messaging (NATS)

**Subjects:**

```
┌─────────────────────────────────────────────┐
│           NATS (Port 4223)                  │
├─────────────────────────────────────────────┤
│  Subjects:                                  │
│  - crawl.started                            │
│  - crawl.completed                          │
│  - crawl.failed                             │
│  - import.started                           │
│  - import.completed                         │
│  - import.failed                            │
│  - integration.sync.started                 │
│  - integration.sync.completed               │
└─────────────────────────────────────────────┘
```

**Consumers:**
- org-core service (document storage)
- Analytics service (usage tracking)
- Billing service (quota enforcement)

### Vector Database (Qdrant)

**Collections:**

```
┌─────────────────────────────────────────────┐
│         Qdrant (Port 6335, 6336)            │
├─────────────────────────────────────────────┤
│  Future Use:                                │
│  - Semantic search across ingested content  │
│  - Document similarity detection            │
│  - RAG (Retrieval-Augmented Generation)     │
│  - Duplicate detection                      │
└─────────────────────────────────────────────┘
```

**Status:** Infrastructure ready, not yet utilized

---

## Data Flow

### Scenario 1: Web Scraping (Quarry)

```
1. Client Request
   └─> POST /v1/scrape { url: "https://example.com" }
   
2. API Layer (Quarry)
   ├─> Validate API key
   ├─> Rate limit check
   └─> Security scan (5 providers)
   
3. Scraper Layer
   ├─> Select driver (Colly vs Rod)
   ├─> Fetch HTML content
   └─> Take screenshot (if requested)
   
4. AI Extraction (optional)
   ├─> Check AI cache (Redis)
   ├─> Generate extraction plan (gRPC → ai-core)
   ├─> Execute extraction (gRPC → ai-core)
   └─> Cache result (24h TTL)
   
5. Transform Layer
   ├─> Convert to Markdown
   ├─> Convert to TOON (40% token reduction)
   └─> Generate structured JSON
   
6. Response
   └─> Return to client (200 OK, <500ms)
```

### Scenario 2: File Import (Imports-Core)

```
1. Client Upload
   └─> POST /v1/import/file (multipart/form-data)
   
2. API Layer (Imports)
   ├─> Validate org quota (→ org-core)
   ├─> Detect file type (.csv, .pdf, .docx)
   └─> Create import job (PostgreSQL)
   
3. Parser Layer
   ├─> CSV: pandas DataFrame → rows
   ├─> PDF: PyPDF2 → text pages
   ├─> DOCX: python-docx → paragraphs
   └─> JSON: Native parser → objects
   
4. Processing Layer
   ├─> Chunk large documents
   ├─> Extract metadata
   └─> Create ImportDocument objects
   
5. Orchestration (Temporal)
   ├─> Dispatch FileImportWorkflow
   ├─> Process items in batches
   └─> Update job progress
   
6. Event Publishing (NATS)
   ├─> Publish progress events
   └─> Notify org-core on completion
   
7. SSE Streaming
   └─> Stream progress to client (Server-Sent Events)
```

### Scenario 3: External Integration (Notion Sync)

```
1. Client Request
   └─> POST /v1/import/source { source_type: "notion", connection: {...} }
   
2. API Layer (Imports)
   ├─> Validate credentials
   ├─> Check quota (→ org-core)
   └─> Create import job
   
3. Connector Layer
   ├─> Authenticate with Notion API (token)
   ├─> Execute search query (filter, sort)
   └─> Fetch pages (pagination)
   
4. Transformation
   ├─> Extract title from properties
   ├─> Convert Notion blocks → text
   └─> Create ImportDocument objects
   
5. Processing
   ├─> Dispatch SourceSyncWorkflow (Temporal)
   ├─> Process documents in batches
   └─> Handle rate limits (Notion API: 3 req/sec)
   
6. Storage
    ├─> Send to Data Plane ingestion/document contract
    └─> Store in canonical document / knowledge collection
   
7. Events
   ├─> Publish import.completed (NATS)
   └─> Update job status (PostgreSQL)
```

---

## Integration Patterns

### Pattern 1: Async Job Pattern

**Used by:** Quarry crawl, Imports bulk processing

```go
// Quarry Example
POST /v1/crawl → Returns: { "jobId": "uuid-123" }
GET /v1/jobs/uuid-123 → Returns: { "status": "processing", "progress": 45 }
Webhook → POST https://client.com/webhook { "jobId": "uuid-123", "status": "completed" }
```

```python
# Imports Example
POST /v1/import/file → Returns: { "jobId": "uuid-456" }
GET /v1/import/jobs/uuid-456 → Returns: { "status": "completed", "total": 100, "processed": 100 }
GET /v1/import/jobs/uuid-456/stream → SSE: data: {"event": "progress", "percent": 75}
```

### Pattern 2: Event-Driven Pattern

**Used by:** Cross-service communication

```
┌─────────┐         ┌──────┐         ┌──────────┐
│ Quarry  │─event─>│ NATS │─event─>│ Org-Core │
└─────────┘         └──────┘         └──────────┘
   crawl.completed              → Store documents

┌─────────┐         ┌──────┐         ┌──────────┐
│ Imports │─event─>│ NATS │─event─>│ Analytics│
└─────────┘         └──────┘         └──────────┘
   import.completed             → Track usage
```

### Pattern 3: Quota Check Pattern

**Used by:** All ingestion services

```python
# Pre-flight quota check
async def check_quota(org_id: str, items: int) -> bool:
    response = await httpx.post(
        f"{ORG_CORE_URL}/api/v1/quota/check",
        json={"org_id": org_id, "operation": "import", "items": items}
    )
    return response.json().get("allowed", False)

# Usage
if not await check_quota(org_id, len(documents)):
    raise HTTPException(status_code=402, detail="Quota exceeded")
```

---

## Deployment Architecture

### Development (Docker Compose)

```yaml
# Complete stack
docker-compose up

# Services started:
# - quarry-api (8090)
# - quarry-worker
# - imports-api (3041)
# - postgresql (5434)
# - redis (6380)
# - temporal (7234)
# - temporal-ui (8089)
# - nats (4223, 8223)
# - qdrant (6335, 6336)
```

### Production (Kubernetes - Planned)

```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: ingestion-plane

---
# Services:
# - quarry-api (Deployment, HPA)
# - quarry-worker (Deployment, HPA)
# - imports-api (Deployment, HPA)
# - postgresql (StatefulSet)
# - redis (StatefulSet)
# - temporal (Helm chart)
# - nats (Helm chart)
# - qdrant (StatefulSet)

# Ingress:
# - ingestion.triodelab.com → quarry-api
# - imports.triodelab.com → imports-api
```

---

## Monitoring & Observability

### Metrics Endpoints

```
Quarry:
  GET /metrics
    - quarry_scrape_requests_total
    - quarry_scrape_duration_seconds
    - quarry_ai_cache_hit_ratio
    - quarry_security_scan_duration_seconds
    - quarry_batch_active_jobs

Imports:
  GET /metrics  (Future)
    - imports_files_processed_total
    - imports_parsing_duration_seconds
    - imports_connector_requests_total
```

### Health Checks

```
Quarry:
  GET /health     → Liveness probe
  GET /ready      → Readiness probe (includes ai-core check)

Imports:
  GET /health     → Liveness probe (Future)
  GET /ready      → Readiness probe (Future)
```

### Logging

**Quarry:** Structured JSON logs (zerolog)
```json
{
  "level": "info",
  "time": "2026-02-19T10:30:45Z",
  "service": "quarry-api",
  "job_id": "uuid-123",
  "url": "https://example.com",
  "duration_ms": 234,
  "message": "scrape completed"
}
```

**Imports:** Structured logs (Python logging)
```json
{
  "timestamp": "2026-02-19T10:30:45Z",
  "level": "INFO",
  "service": "imports-api",
  "job_id": "uuid-456",
  "file_type": "csv",
  "rows_processed": 1000,
  "message": "import job completed"
}
```

### Temporal UI

**URL:** http://localhost:8089
- Workflow execution history
- Task queue metrics
- Workflow versioning
- Manual workflow triggers

---

## Security

### Authentication

| Service | Method | Implementation |
|---------|--------|----------------|
| Quarry | API Key | `X-API-Key` header, constant-time comparison |
| Imports | API Key | `X-API-Key` header (planned) |
| Internal (gRPC) | mTLS | Certificate-based (planned) |

### Authorization

| Operation | Check | Enforcement |
|-----------|-------|-------------|
| Scrape | Quota | Pre-flight check to org-core |
| Import | Quota | Pre-flight check to org-core |
| Integration | Connection ownership | Verify org_id owns connection |

### Network Security

```
┌─────────────────────────────────────────────┐
│         INTERNET                            │
└───────────────┬─────────────────────────────┘
                │
         ┌──────▼──────┐
         │   TLS       │
         │  Termination│
         └──────┬──────┘
                │
    ┌───────────▼──────────────┐
    │  Service Mesh (Optional) │
    │  - Istio/Linkerd         │
    │  - mTLS between services │
    └───────────┬──────────────┘
                │
    ┌───────────┴───────────┐
    │                       │
┌───▼────┐            ┌────▼────┐
│ Quarry │            │ Imports │
└────────┘            └─────────┘
```

### Data Protection

- **In Transit:** TLS 1.3 (production)
- **At Rest:** PostgreSQL encryption (AWS RDS)
- **Secrets:** Kubernetes Secrets / AWS Secrets Manager
- **API Keys:** Hashed (SHA-256) in database
- **Sensitive Logs:** PII redaction

---

## Future Roadmap

### Phase 1: Microservices Refactoring (8-12 weeks)

**Goal:** Split into 4 independent services

```
Current:
  Quarry (monolith) + imports-core (standalone)

Future:
  - query-service (Port 3040)    → API Gateway
  - import-service (Port 3041)   → File parsing
  - integration-service (Port 3042) → External connectors
  - crawler-service (Port 3043)  → Web crawling
```

**Benefits:**
- Independent scaling
- Technology diversity (Go + Python)
- Fault isolation
- Team autonomy

**Details:** See [INGESTION_PLANE_ROADMAP.md](Quarry/docs/INGESTION_PLANE_ROADMAP.md)

### Phase 2: Enhanced Integrations (4-6 weeks)

**New Connectors:**
- Google Drive
- Dropbox
- SharePoint
- Confluence
- Jira
- GitHub
- GitLab

**Features:**
- OAuth 2.0 flow management
- Incremental sync (delta imports)
- Bi-directional sync (write-back)
- Connection management UI

### Phase 3: Advanced AI Features (6-8 weeks)

**Capabilities:**
- Custom extraction models
- Multi-modal extraction (images, tables)
- RAG integration (Qdrant)
- Semantic deduplication
- Content classification
- Entity extraction

### Phase 4: Enterprise Features (8-10 weeks)

**Features:**
- Multi-tenancy isolation
- RBAC (Role-Based Access Control)
- Audit logging (compliance)
- Data retention policies
- Encryption at rest
- SOC 2 compliance
- GDPR compliance

---

## Developer Guide

### Prerequisites

```bash
# System requirements
- Docker Desktop (4.25+)
- Go 1.24+ (for Quarry development)
- Python 3.11+ (for Imports development)
- Make (build automation)

# Optional
- grpcurl (gRPC testing)
- kubectl (Kubernetes deployment)
```

### Local Development Setup

```bash
# 1. Clone repository
git clone https://github.com/triodelab/coresystem.git
cd "Ingestion Plane"

# 2. Start infrastructure
docker-compose up -d postgres redis temporal nats qdrant

# 3. Start Quarry
cd Quarry
make build
./bin/quarry-api              # API server (8090)
./bin/quarry-worker           # Temporal worker

# 4. Start Imports
cd ../imports-core
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
uvicorn app.main:app --reload --port 3041

# 5. Verify
curl http://localhost:8090/health       # Quarry
curl http://localhost:3041/health       # Imports (if endpoint exists)
```

### Testing

```bash
# Quarry tests
cd Quarry
go test ./...
./scripts/test-endpoints.sh
./scripts/test-performance.sh

# Imports tests
cd imports-core
pytest tests/
```

### Environment Variables

**Quarry:**
```bash
PORT=8090
CACHE_BACKEND=redis
REDIS_URL=redis://localhost:6380/0
TEMPORAL_ENABLED=true
TEMPORAL_ADDRESS=localhost:7234
AI_CORE_GRPC_ADDR=localhost:50051
QUARRY_API_KEY=your-secure-key
```

**Imports:**
```bash
PORT=3041
DATABASE_URL=postgresql://user:pass@localhost:5434/imports
TEMPORAL_ENABLED=true
TEMPORAL_HOST_PORT=localhost:7234
ORG_SERVICE_URL=http://localhost:3030/api/v1
NATS_URL=nats://localhost:4223
```

---

## Cost Analysis

### Self-Hosted vs SaaS

**Assumptions:**
- 1M pages/month scraped
- 100K files/month imported
- 50K integration syncs/month

**Self-Hosted (Ingestion Plane):**
```
Infrastructure:
  - EC2 instances (t3.medium × 3):  $90/mo
  - RDS PostgreSQL (db.t3.medium):  $80/mo
  - ElastiCache Redis (t3.small):   $40/mo
  - Data transfer (100GB):          $10/mo
  - Total Infrastructure:           $220/mo

AI Costs (ai-core):
  - Without caching: $2,000/mo
  - With 70% cache hit: $600/mo

TOTAL: $820/mo
```

**SaaS Alternative (Firecrawl + Import SaaS):**
```
Scraping (Firecrawl):
  - 1M pages × $0.10/page = $100,000/mo

File Import (Hypothetical SaaS):
  - 100K files × $0.05/file = $5,000/mo

Integration (Zapier/MuleSoft):
  - 50K syncs × $0.10/sync = $5,000/mo

TOTAL: $110,000/mo
```

**Savings:** $109,180/mo (**99.3%** cost reduction)  
**Annual Savings:** $1,310,160

---

## Support & Maintenance

### Runbooks

- See [DEPLOYMENT.md](Quarry/docs/DEPLOYMENT.md) for deployment procedures
- See [SECURITY.md](Quarry/docs/SECURITY.md) for security best practices
- See [INGESTION_PLANE_ROADMAP.md](Quarry/docs/INGESTION_PLANE_ROADMAP.md) for refactoring plan

### Incident Response

**Severity Levels:**
- **P0 (Critical):** Service unavailable, data loss
- **P1 (High):** Degraded performance, partial outage
- **P2 (Medium):** Non-critical feature broken
- **P3 (Low):** Minor bug, enhancement request

**On-Call Rotation:**
- Primary: Backend team
- Secondary: Platform team
- Escalation: CTO

### SLA Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Availability | 99.9% | Uptime monitoring |
| API Latency (p95) | <1s | Prometheus metrics |
| Error Rate | <1% | Error tracking |
| Data Loss | 0% | Backup verification |

---

## Glossary

**Terms:**
- **Quarry:** Web scraping service (formerly monolithic, becoming crawler-service)
- **Imports-Core:** File import service (Python FastAPI)
- **Integration:** External API connector (Notion, HubSpot, etc.)
- **Temporal:** Workflow orchestration platform
- **NATS:** Message broker for event streaming
- **TOON:** Tree Object Notation (AI-optimized format, 40% token reduction)
- **SSE:** Server-Sent Events (real-time streaming)
- **gRPC:** High-performance RPC framework
- **ai-core:** Centralized AI service (extraction, planning, agents)

---

## Conclusion

The Ingestion Plane provides a comprehensive, production-ready data acquisition layer with:
- ✅ **Quarry:** Enterprise web scraping (production)
- 🚧 **Imports:** File parsing and bulk imports (development)
- 🚧 **Integrations:** External API connectors (development)

**Status:** Hybrid production/development  
**Next Steps:** Complete imports-core APIs, begin microservices refactoring  
**Timeline:** 8-12 weeks to full microservices architecture  
**Cost Savings:** 99.3% vs SaaS alternatives ($110K → $820/mo)

---

**Documentation:**
- [Quarry PROJECT_SUMMARY.md](Quarry/PROJECT_SUMMARY.md)
- [Quarry API Documentation](Quarry/docs/API.md)
- [Quarry Architecture](Quarry/docs/ARCHITECTURE.md)
- [Deployment Guide](Quarry/docs/DEPLOYMENT.md)
- [Security Documentation](Quarry/docs/SECURITY.md)
- [Ingestion Plane Roadmap](Quarry/docs/INGESTION_PLANE_ROADMAP.md)

**Repository:** https://github.com/triodelab/coresystem  
**Maintainer:** Triodelab Engineering Team  
**Last Updated:** February 19, 2026 (architecture); 2026-05-20 (runtime audit appendix below)

---

## Appendix: 2026-05-20 Velion Build Runtime Audit

The "System Overview" diagram above is **stale** — current Quarry v2 architecture is captured below from the live compose + `Quarry-v2/docs/gap-quarry.md`.

### Quarry v2 — current architecture
- **quarry-edge** — Rust hot path (REST/SSE), port **8082** (not 8090 as the legacy diagram shows)
- **quarry-control** — Go durable plane, port **8081**
- **quarry-orchestrator** — Go Temporal worker, no exposed port
- Legacy "Quarry · Port 8090 · Lang Go" line in the System Overview diagram should be retired.

### Build-time and runtime fixes applied 2026-05-20
1. `crates/quarry-runtime/build.rs` — `protoc` couldn't find well-known types (`google/protobuf/timestamp.proto` and `.../struct.proto`) inside the `rust:1-slim-bookworm` build image. Vendored both `.proto` files into `crates/quarry-runtime/proto/google/protobuf/` so the existing `&["proto"]` include path resolves them. `build.rs` extended with a `PROTOC_INCLUDE` fallback that probes `/usr/include`, `/usr/local/include`, `/opt/homebrew/include`, and `/usr/local/opt/protobuf/include`.

### Observed runtime (2026-05-20 build run)
- All Quarry v2 + Nango + ingestion infra containers came up `healthy`.
- `support-worker` (declared in `services/support-worker/`) initially fails with `getaddrinfo ENOTFOUND velion-nats` because `velion-nats` lives in the Frontend Plane Velion stack (index 5) which has not started yet. `restart: unless-stopped` self-heals after stack 5.
- One-shot `nango-seed` runs once and is removed from `docker ps -a` by the build script.

### Additional services in compose (not in the legacy diagram)
- `finspo-api` (port 3130) — SharePoint Manager
- `integration-api` (port 3026) — integration-core REST
- `imports-api` — imports-core REST (status was 🚧 in February doc; container now boots healthy)
- `connector-runtime-engine` (port 3003) — self-hosted Nango
- `connector-runtime-db` / `connector-runtime-redis` — Nango infra (NOT on `inter-plane-bus`, only `ingestion-net`)

### Velion server-side wiring — verified correct for this stack
The following `apps/Frontend Plane/velion/.env` entries resolve correctly via `inter-plane-bus`:
```
QUARRY_API_URL=http://quarry-control:8081       ✅
QUARRY_EDGE_URL=http://quarry-edge:8082         ✅
INTEGRATION_CORE_URL=http://integration-api:3026 ✅
```
Stale or unverified entries:
```
IMPORTS_API_URL=http://imports-api:3025       ⚠️ verify container name + port
INTEGRATION_ENGINE_URL=http://integration-engine-go-api:3126 ⚠️ verify presence
```
