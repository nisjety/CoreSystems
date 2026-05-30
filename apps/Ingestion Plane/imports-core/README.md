# Imports-Core Service

**Purpose:** Parse and import structured/unstructured files and external APIs at scale  
**Language:** Python 3.11+  
**Framework:** FastAPI  
**Status:** 🚧 Active Development  
**Version:** 1.0.0

---

## Features

### File Parsing
- **PDF** - Extract text from PDFs using PyPDF2
- **DOCX** - Parse Word documents using python-docx
- **CSV/Excel** - Parse tabular data using pandas
- **JSON** - Validate and parse JSON structures
- **HTML** - Extract text from HTML using BeautifulSoup
- **Plain Text** - TXT and Markdown files
- **Metadata** - File hash, size, content type tracking

### External Integrations
- **Notion** - Sync pages and databases from Notion
- **HubSpot** - Import contacts, deals, companies
- **Salesforce** - Query and import records via SOQL
- **Odoo** - Import records from ERP/PIM systems
- **Generic REST APIs** - Custom HTTP/JSON sources

### Job Management
- **Async Processing** - Long-running jobs with Temporal orchestration
- **Progress Tracking** - Real-time job status via SSE
- **Quota Enforcement** - Integration with org-core for quota checks
- **Event Streaming** - NATS-based event publishing
- **Error Handling** - Comprehensive error tracking and retry logic

---

## Architecture

```
┌─────────────────────────────────────────┐
│      Imports-Core Service (Python)      │
├─────────────────────────────────────────┤
│                                         │
│  ┌─────────────────────────────────┐   │
│  │     FastAPI HTTP Layer          │   │
│  │ • Upload endpoint               │   │
│  │ • Source import endpoint        │   │
│  │ • Job status endpoint           │   │
│  │ • Event streaming (SSE)         │   │
│  └─────────────────────────────────┘   │
│                 │                       │
│  ┌──────────────▼──────────────────┐   │
│  │     Business Logic (Service)    │   │
│  │ • Job creation & management     │   │
│  │ • Quota enforcement             │   │
│  │ • Progress tracking             │   │
│  └──────────────┬──────────────────┘   │
│                 │                       │
│  ┌──────────────▼──────────────┐       │
│  │   Support Modules           │       │
│  ├──────────────────────────┤   │
│  │ Parsers    │ Connectors  │   │
│  │ • PDF      │ • Notion    │   │
│  │ • DOCX     │ • HubSpot   │   │
│  │ • CSV      │ • Salesforce│   │
│  │ • JSON     │ • Odoo      │   │
│  │ • HTML     │ • Generic   │   │
│  └──────────────────────────┘   │
│                                         │
└─────────────────────────────────────────┘
         │                    │
         ▼                    ▼
    ┌─────────────┐    ┌──────────────┐
    │  PostgreSQL │    │   NATS       │
    │  (Job Store)│    │  (Events)    │
    └─────────────┘    └──────────────┘
         │
         ▼
    ┌─────────────────┐
    │ Document        │
    │ Service (Org)   │
    └─────────────────┘
```

---

## Installation

### Prerequisites

```bash
# System requirements
- Python 3.11+
- PostgreSQL 14+
- NATS (optional, for event streaming)
- Temporal (optional, for advanced orchestration)
```

### Local Setup

```bash
# 1. Clone and navigate
cd imports-core

# 2. Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# 3. Install dependencies
pip install -r requirements.txt

# 4. Configure environment
cp .env.example .env
# Edit .env with your settings

# 5. Initialize database
psql -U postgres -d imports < migrations/001_init.sql

# 6. Run service
uvicorn app.main:app --reload --port 3025
```

### Docker

```bash
# Build image
docker build -t imports-core:latest .

# Run container
docker run -p 3025:3025 \
  -e DATABASE_URL=postgresql://user:pass@db:5432/imports \
  -e DOCUMENT_SERVICE_URL=http://localhost:3030 \
  -e ORG_SERVICE_URL=http://localhost:3010 \
  imports-core:latest
```

---

## API Reference

### 1. File Upload

**Endpoint:** `POST /api/v1/import/jobs/upload`

**Parameters:**
- `org_id` (form) - Organization ID
- `user_id` (form, optional) - User ID
- `files` (form) - Multiple file uploads

**Request:**
```bash
curl -X POST http://localhost:3025/api/v1/import/jobs/upload \
  -F "org_id=org_12345" \
  -F "user_id=user_67890" \
  -F "files=@document.pdf" \
  -F "files=@data.csv"
```

**Response:**
```json
{
  "id": "uuid-123",
  "org_id": "org_12345",
  "user_id": "user_67890",
  "source_type": "upload",
  "status": "queued",
  "total_items": 2,
  "processed_items": 0,
  "failed_items": 0,
  "created_at": "2026-02-19T10:30:45Z",
  "metadata": {"upload_count": 2}
}
```

### 2. Source Import

**Endpoint:** `POST /api/v1/import/jobs/source`

**Payload:**
```json
{
  "org_id": "org_12345",
  "user_id": "user_67890",
  "source_type": "notion",
  "connection": {
    "token": "secret_abc123..."
  },
  "options": {
    "filter": {"property": "Status", "select": {"equals": "Published"}}
  }
}
```

**Supported Sources:**
- `notion` - Notion workspace pages
- `hubspot` - HubSpot contacts/deals/companies
- `salesforce` - Salesforce records
- `odoo` - Odoo ERP models
- `cms` - Generic HTTP/REST endpoints

**Response:**
```json
{
  "id": "uuid-456",
  "org_id": "org_12345",
  "source_type": "notion",
  "status": "queued",
  "total_items": 50,
  "processed_items": 0,
  "failed_items": 0,
  "created_at": "2026-02-19T10:30:45Z"
}
```

### 3. Get Job Status

**Endpoint:** `GET /api/v1/import/jobs/{job_id}`

**Response:**
```json
{
  "id": "uuid-123",
  "status": "completed",
  "total_items": 100,
  "processed_items": 98,
  "failed_items": 2,
  "items": [
    {
      "id": "item-1",
      "job_id": "uuid-123",
      "source_id": "notion-page-abc",
      "status": "completed",
      "created_at": "2026-02-19T10:30:45Z",
      "completed_at": "2026-02-19T10:31:15Z"
    },
    {
      "id": "item-2",
      "job_id": "uuid-123",
      "source_id": "notion-page-xyz",
      "status": "failed",
      "error_message": "Page parse error",
      "created_at": "2026-02-19T10:30:45Z"
    }
  ]
}
```

### 4. Stream Job Events (SSE)

**Endpoint:** `GET /api/v1/import/jobs/{job_id}/events`

**Response:** Server-Sent Events stream

```
event: import.snapshot
data: {"job_id":"uuid-123","status":"running","processed_items":0,"total_items":100}

event: import.progress
data: {"event":"import.progress","processed_items":10,"total_items":100,"percent":10}

event: import.progress
data: {"event":"import.progress","processed_items":50,"total_items":100,"percent":50}

event: import.completed
data: {"job_id":"uuid-123","status":"completed","processed_items":100,"total_items":100}
```

### 5. Health Check

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "ok",
  "service": "import-service"
}
```

---

## Configuration

### Environment Variables

```bash
# Service
IMPORT_SERVICE_NAME=import-service
IMPORT_SERVICE_PORT=3025
LOG_LEVEL=INFO

# Database
DATABASE_URL=postgresql+asyncpg://user:pass@localhost:5432/imports

# Downstream Services
DOCUMENT_SERVICE_URL=http://localhost:3030
ORG_SERVICE_URL=http://localhost:3010

# Messaging
NATS_URL=nats://localhost:4222
NATS_TOKEN=optional-token

# Orchestration
TEMPORAL_ENABLED=false
TEMPORAL_HOST_PORT=localhost:7233

# Limits
MAX_UPLOAD_FILES=100
MAX_FILE_SIZE_MB=50
ALLOWED_FILE_TYPES=pdf,docx,txt,md,csv,json,html
```

---

## Data Models

### ImportJob

```python
class ImportJob(Base):
    id: UUID                      # Primary key
    org_id: str                   # Organization ID
    user_id: str | None           # User ID (optional)
    source_type: str              # 'upload', 'notion', 'hubspot', etc.
    status: str                   # 'queued', 'running', 'completed', 'failed'
    total_items: int              # Total documents to import
    processed_items: int          # Successfully processed
    failed_items: int             # Failed to process
    error_message: str | None     # Job-level error
    metadata: dict                # Custom metadata (JSON)
    created_at: datetime
    started_at: datetime | None
    completed_at: datetime | None
    items: list[ImportJobItem]    # Related items
```

### ImportJobItem

```python
class ImportJobItem(Base):
    id: UUID                      # Primary key
    job_id: UUID                  # Foreign key to job
    source_id: str | None         # External source ID
    source_name: str | None       # Human-readable name
    status: str                   # 'queued', 'running', 'completed', 'failed'
    error_message: str | None     # Item-level error
    metadata: dict                # Custom metadata (JSON)
    created_at: datetime
    completed_at: datetime | None
```

---

## File Parsers

### PDF Parser

```python
from app.parsers import parse_uploaded_file

content = open("document.pdf", "rb").read()
doc = parse_uploaded_file("document.pdf", content, "application/pdf")

# Result:
# ImportDocument(
#     source_name="document.pdf",
#     title="document",
#     text="Extracted text from PDF...",
#     metadata={
#         "filename": "document.pdf",
#         "content_type": "application/pdf",
#         "size_bytes": 12345,
#         "sha256": "abc123...",
#         "extension": "pdf"
#     }
# )
```

### CSV Parser

```python
content = b"name,email\nJohn,john@example.com\nJane,jane@example.com"
doc = parse_uploaded_file("data.csv", content, "text/csv")

# Result:
# ImportDocument(text="name\temail\nJohn\tjohn@example.com\n...")
```

### Connectors

```python
from app.connectors import import_from_source

# Notion
docs = await import_from_source(
    source_type="notion",
    connection={"token": "secret_abc..."},
    options={"filter": {...}}
)

# HubSpot
docs = await import_from_source(
    source_type="hubspot",
    connection={"access_token": "oauth_xyz..."},
    options={"object_type": "contacts", "limit": 500}
)

# Salesforce
docs = await import_from_source(
    source_type="salesforce",
    connection={
        "username": "user@company.com",
        "password": "pass123",
        "security_token": "token123",
        "domain": "login"
    },
    options={"query": "SELECT Id, Name FROM Contact"}
)

# Generic HTTP/REST
docs = await import_from_source(
    source_type="cms",
    connection={"url": "https://api.example.com/items"},
    options={"params": {"limit": 100}}
)
```

---

## Error Handling

### Validation Errors

```
400 Bad Request
- Unsupported file type
- File exceeds max size
- No files provided
- Invalid source configuration
```

### Quota Errors

```
403 Forbidden
- Import quota exceeded for organization
- Contact organization administrator for quota increase
```

### Not Found

```
404 Not Found
- Job not found
- Invalid job ID provided
```

### Server Errors

```
500 Internal Server Error
- Failed to create import job
- Database connection error
- Downstream service unavailable
```

### Retry Logic

- Failed item processing: automatic retry (up to 3 attempts)
- Transient errors: exponential backoff (1s, 2s, 4s)
- Permanent errors: logged and marked as failed
- Network timeouts: 30s per request

---

## Testing

### Unit Tests

```bash
# Run all tests
pytest tests/

# Run with coverage
pytest --cov=app tests/

# Run specific test
pytest tests/test_parsers.py::test_pdf_parsing
```

### Integration Tests

```bash
# Start services
docker-compose up -d

# Run integration tests
pytest -m integration tests/

# Test file upload
curl -X POST http://localhost:3025/api/v1/import/jobs/upload \
  -F "org_id=test-org" \
  -F "files=@test-file.csv"
```

### Load Testing

```bash
# Using Apache Bench
ab -n 1000 -c 10 http://localhost:3025/health

# Using wrk
wrk -t4 -c100 -d30s http://localhost:3025/health
```

---

## Performance

### Benchmarks

| Operation | Latency | Throughput | Notes |
|-----------|---------|-----------|-------|
| Health check | <10ms | >1000 req/s | Minimal overhead |
| File upload (CSV, 1MB) | 500ms | 100 files/min | Parser + validation |
| Notion sync (100 pages) | 30s | 3 pages/sec | API rate-limited |
| HubSpot import (1000 contacts) | 10s | 100 contacts/sec | Batch processing |

### Optimization Tips

1. **Batch Uploads** - Upload multiple files in single request
2. **Connector Rate Limits** - Respect external API limits (built-in)
3. **Job Polling** - Use SSE for real-time updates vs polling
4. **Database Tuning** - Add indexes for org_id, status, created_at
5. **Connection Pooling** - Keep NATS/PostgreSQL connections alive

---

## Troubleshooting

### High Memory Usage

```bash
# Check import job size
SELECT job_id, COUNT(*) as items FROM import_job_items GROUP BY job_id ORDER BY items DESC;

# Large file parsing
# - Stream file in chunks instead of loading entire content
# - Use chunked parsing for PDFs with >100 pages
```

### Slow Job Processing

```bash
# Check database performance
EXPLAIN ANALYZE SELECT * FROM import_jobs WHERE org_id = 'org_id' ORDER BY created_at DESC;

# Verify NATS connectivity
ps aux | grep nats

# Check network latency to downstream services
curl -w "@curl-format.txt" -o /dev/null -s https://document-service:3030/health
```

### Connection Failures

```bash
# Test database
psql $DATABASE_URL -c "SELECT 1;"

# Test NATS
nc -v localhost 4222

# Test downstream services
curl http://document-service:3030/health
curl http://org-service:3010/health
```

---

## Deployment

### Container Registry

```bash
# Build and push to registry
docker build -t registry.example.com/imports-core:1.0.0 .
docker push registry.example.com/imports-core:1.0.0
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: imports-core
spec:
  replicas: 3
  selector:
    matchLabels:
      app: imports-core
  template:
    metadata:
      labels:
        app: imports-core
    spec:
      containers:
      - name: imports-core
        image: registry.example.com/imports-core:1.0.0
        ports:
        - containerPort: 3025
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: imports-core-secrets
              key: database-url
        livenessProbe:
          httpGet:
            path: /health
            port: 3025
          initialDelaySeconds: 10
          periodSeconds: 30
        readinessProbe:
          httpGet:
            path: /health
            port: 3025
          initialDelaySeconds: 5
          periodSeconds: 10
```

### Environment-Specific Configuration

```bash
# Development
DOCUMENT_SERVICE_URL=http://localhost:3030
ORG_SERVICE_URL=http://localhost:3010
TEMPORAL_ENABLED=false

# Staging
DOCUMENT_SERVICE_URL=https://document-service.staging.internal
ORG_SERVICE_URL=https://org-service.staging.internal
TEMPORAL_ENABLED=true

# Production
DOCUMENT_SERVICE_URL=https://document-service.internal
ORG_SERVICE_URL=https://org-service.internal
TEMPORAL_ENABLED=true
```

---

## Contributing

### Code Style

```bash
# Format code
black app/

# Lint
flake8 app/
pylint app/

# Type checking
mypy app/
```

### Adding New File Parser

```python
# 1. Add function to app/parsers.py
def _parse_xlsx(content: bytes) -> str:
    # Implementation
    pass

# 2. Update main parse function
if extension == "xlsx":
    text = _parse_xlsx(content)

# 3. Add to allowed types
ALLOWED_FILE_TYPES = [..., "xlsx"]

# 4. Test
pytest tests/test_parsers.py::test_xlsx_parsing
```

### Adding New Connector

```python
# 1. Add function to app/connectors.py
async def import_from_google_drive(
    connection: dict[str, Any],
    options: dict[str, Any]
) -> list[ImportDocument]:
    # Implementation
    pass

# 2. Update import_from_source
if source_type_lower == "google_drive":
    return await import_from_google_drive(connection, options)

# 3. Update request schema
class SourceImportRequest(BaseModel):
    source_type: Literal[..., "google_drive"]

# 4. Test
pytest tests/test_connectors.py::test_google_drive_import
```

---

## Monitoring

### Logging

```python
import logging

logger = logging.getLogger(__name__)

logger.info("Import job %s started", job_id)
logger.warning("Connector rate limit reached: %s", source_type)
logger.error("Failed to parse file: %s", filename, exc_info=True)
```

### Metrics

```
# Prometheus metrics (future)
imports_jobs_total{source_type="upload"}
imports_files_processed_total{extension="pdf"}
imports_parsing_duration_seconds{file_type="docx"}
imports_connector_requests_total{source="notion"}
imports_connector_errors_total{source="hubspot"}
```

### Alerts

```yaml
# Alert rules (Prometheus)
- alert: ImportJobProcessingStalled
  expr: increase(imports_jobs_total{status="running"}[1h]) == 0
  for: 30m
  annotations:
    summary: "No import jobs processing for 30 minutes"

- alert: HighErrorRate
  expr: |
    (increase(imports_jobs_total{status="failed"}[1h]) /
     increase(imports_jobs_total[1h])) > 0.1
  for: 10m
  annotations:
    summary: "Import error rate >10%"
```

---

## Future Enhancements

### Phase 2 (Next Quarter)

- [ ] Incremental sync (delta imports)
- [ ] Bi-directional sync (write-back to sources)
- [ ] Custom extraction templates
- [ ] Multi-language support
- [ ] Advanced filtering/transformation

### Phase 3 (Q3)

- [ ] Vector database integration (Qdrant)
- [ ] Semantic deduplication
- [ ] Auto-categorization
- [ ] Custom metrics dashboard
- [ ] HIPAA/SOC2 compliance

### Phase 4 (Q4)

- [ ] GraphQL API
- [ ] WebSocket support
- [ ] Mobile SDK
- [ ] Enterprise admin panel
- [ ] Advanced audit logging

---

## Support

**Issues:** [GitHub Issues](https://github.com/triodelab/coresystem)  
**Slack Channel:** #ingestion-plane-dev  
**On-Call:** See rotation in PagerDuty  
**Documentation:** [ReadTheDocs](https://docs.triodelab.com/imports-core)

---

## License

MIT License - See LICENSE file

---

**Last Updated:** February 19, 2026  
**Maintainer:** Ingestion Plane Team  
**Version:** 1.0.0
