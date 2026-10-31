# Moment Integration Service

[![Python](https://img.shields.io/badge/Python-3.12-3776AB?logo=python&logoColor=white)](https://www.python.org/)
[![Flask](https://img.shields.io/badge/Flask-3.0-000000?logo=flask&logoColor=white)](https://flask.palletsprojects.com/)
[![Docker](https://img.shields.io/badge/Docker-ready-2496ED?logo=docker&logoColor=white)](./docker-compose.yml)
[![BigQuery](https://img.shields.io/badge/BigQuery-sync-669DF6?logo=googlecloud&logoColor=white)](./bigquery_sync.py)

Self-contained Flask microservice for integrating with Moment time tracking and project management platform.

## Overview

This service provides a RESTful API to interact with Moment's API, supporting:

- ✅ Projects management
- ✅ Time records (create, read, update, delete)
- ✅ Customers
- ✅ User accounts
- ✅ Tags
- ✅ Invoices
- ✅ Complex reports (registered hours, time records)
- ✅ Batch operations (bulk create/update/delete)
- ✅ Includes concept for selective field retrieval
- ✅ Rate limiting (120 req/sec per IP, 30 req/sec per token)

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Running the Service](#running-the-service)
- [API Documentation](#api-documentation)
- [Docker Deployment](#docker-deployment)
- [Development](#development)
- [Troubleshooting](#troubleshooting)

## Prerequisites

- Python 3.12+
- Moment API key and company code
- Redis (optional, for caching/sync features)
- PostgreSQL (optional, for sync features)
- Docker & Docker Compose (for containerized deployment)

## Installation

### 1. Clone and Navigate

```bash
cd /path/to/apps/moment_service
```

### 2. Create Virtual Environment

```bash
python3 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate
```

### 3. Install Dependencies

```bash
pip install -r requirements.txt
```

### 4. Configure Environment

Copy the example environment file and update with your credentials:

```bash
cp .env.example .env
```

Edit `.env` and set:

```env
MOMENT_API_KEY=your_api_key_here
MOMENT_COMPANY_CODE=your_company_code_here
```

## Configuration

### Required Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `MOMENT_API_KEY` | Your Moment API key | `NFBvY0JxZThyRGc...` |
| `MOMENT_COMPANY_CODE` | Company code from Moment URL | `cloudware` |

### Optional Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MOMENT_ENVIRONMENT` | `prod` | Environment: `prod`, `sandbox`, or `all` |
| `MOMENT_BASE_URL` | `https://app.moment.team/api/1.0` | Moment API base URL |
| `MOMENT_PORT` | `5006` | Flask service port |
| `FLASK_SECRET_KEY` | Random | Flask secret key |
| `FLASK_DEBUG` | `false` | Enable Flask debug mode |
| `MOMENT_RATE_LIMIT_REQUESTS_PER_SECOND` | `30` | Rate limit per second |
| `MOMENT_RATE_LIMIT_REQUESTS_PER_MINUTE` | `120` | Rate limit per minute |
| `REDIS_URL` | `redis://localhost:6379/0` | Redis connection URL |
| `POSTGRES_URL` | See `.env.example` | PostgreSQL connection URL |
| `GCP_PROJECT_ID` | - | BigQuery project (enables BigQuery sync) |
| `GCP_DATASET_ID` | `moment_data` | BigQuery dataset the `moment_*` tables live in |
| `GOOGLE_APPLICATION_CREDENTIALS` | - | Path to a service account JSON key, relative to the project root (e.g. `app/gcp-credentials.json`) or absolute. Unset falls back to Application Default Credentials. |

Environment variables are loaded from `.env` automatically (via `python-dotenv`) whether you run `python app.py` directly or through Docker Compose (which bind-mounts `.env` into the container) - you don't need to `source`/export it yourself.

### Finding Your Company Code

Your company code is visible in the Moment URL when logged in:

```
https://app.moment.team/team/{company_code}/...
                              ^^^^^^^^^^^^^^
```

## Running the Service

### Development Mode

```bash
# Activate virtual environment
source venv/bin/activate

# Run the service
python app.py
```

The service will be available at `http://localhost:5006`

### Production Mode

Use Docker Compose (recommended):

```bash
docker-compose up -d
```

Or use Docker directly:

```bash
docker build -t moment_service .
docker run -p 5006:5006 --env-file .env moment_service
```

## API Documentation

### Base URL

```
http://localhost:5006/api
```

### Authentication

The service handles authentication with Moment API automatically using the configured API key (Basic Auth with username `apikey`).

### Health Check

```http
GET /health
```

Response:
```json
{
  "status": "healthy",
  "service": "moment_service",
  "version": "1.0.0",
  "timestamp": "2024-12-22T10:30:00.000Z"
}
```

### Projects

#### List Projects

```http
GET /api/projects?includes=base+customer
```

#### Get Project

```http
GET /api/projects/{project_id}?includes=base
```

#### Get Registered Hours Report

```http
GET /api/projects/registered-hours-report?fromDate=2024-01-01&toDate=2024-01-31&billableRateOption=registered
```

### Time Records

#### List Time Records

```http
GET /api/time-records?includes=base
```

#### Create Time Record

```http
POST /api/time-records
Content-Type: application/json

{
  "projectId": 123,
  "date": "2024-01-15",
  "hours": 8,
  "description": "Development work"
}
```

#### Update Time Record

```http
PATCH /api/time-records/{record_id}
Content-Type: application/json

{
  "hours": 6.5,
  "description": "Updated description"
}
```

#### Delete Time Record

```http
DELETE /api/time-records/{record_id}
```

#### Get Time Records Report

```http
GET /api/time-records/report?from=2024-01-01&groupBy=userAccount,week&projectId=123
```

### Customers

#### List Customers

```http
GET /api/customers?includes=base
```

#### Get Customer

```http
GET /api/customers/{customer_id}
```

### User Accounts

#### List User Accounts

```http
GET /api/user-accounts
```

#### Get User Account

```http
GET /api/user-accounts/{user_id}
```

### Tags

#### List Tags

```http
GET /api/tags
```

### Invoices

#### List Invoices

```http
GET /api/invoices
```

#### Get Invoice

```http
GET /api/invoices/{invoice_id}
```

### Batch Operations

Perform bulk create, update, and delete operations:

```http
POST /api/timeRecords/batch
Content-Type: application/json

{
  "create": [
    {"projectId": 123, "date": "2024-01-15", "hours": 8},
    {"projectId": 456, "date": "2024-01-15", "hours": 4}
  ],
  "update": {
    "789": {"hours": 7},
    "790": {"hours": 5}
  },
  "delete": [791, 792]
}
```

### Generic Entity Endpoints

#### List Any Entity Type

```http
GET /api/entities/{entity_type}?includes=base
```

Example:
```http
GET /api/entities/projects
GET /api/entities/timeRecords
GET /api/entities/customers
```

#### Get Single Entity

```http
GET /api/entities/{entity_type}/{entity_id}
```

### Includes Concept

Moment API uses an "includes" concept to optimize data retrieval. Specify which fields or parts to include:

```http
# Base information only
GET /api/projects?includes=base

# Multiple parts
GET /api/projects?includes=base+customer+metadata.overviewPage

# Nested fields
GET /api/projects/{id}?includes=customer.id+customer.name
```

> **Note:** `includes` tokens are a field *whitelist*, not "base fields plus extras". On projects/customers/userAccounts, `includes=base` returns a small fixed set of fields (e.g. 8-18) - dramatically fewer than the bare endpoint with no `includes` at all (which returns the full record: ~70-100+ fields). If you need the complete entity, omit `includes` entirely rather than assuming `base` is a subset of "full".

## BigQuery Sync

Fetches every page of a Moment entity (sorted by `id` for stable pagination) and **merges** the snapshot into BigQuery via an atomic `MERGE` statement per table:

- **New records** are inserted, stamped with `first_synced_at`
- **Existing records** are updated in place; `synced_at` becomes "last seen in Moment"
- **Records deleted in Moment are never removed** - they're soft-deleted: `is_deleted_in_moment = TRUE` with `deleted_detected_at` stamped. If a record reappears, the flags clear again.
- Duplicates are impossible: the merge keys on `id`, and the fetched snapshot is deduplicated before merging

Filter with `WHERE is_deleted_in_moment IS NOT TRUE` for Moment's current state; omit the filter for the full history including deleted records. Moment's own `created_date`/`last_modified_date` columns cover in-system change tracking.

A safety guard: if Moment suddenly returns **zero** rows for an entity that has data in BigQuery, the merge is skipped (nothing gets flagged deleted) and a warning is logged - a total wipe is far more likely an API hiccup than reality.

See `bigquery_sync.py` for the per-entity schemas and `sync/moment_sync.py` for the fetch-everything orchestrator.

Every synced table has a `raw_json` column holding the untouched API payload alongside the curated typed columns, so a mapping gap never means lost data - query `raw_json` with BigQuery's `JSON_VALUE`/`JSON_EXTRACT` for anything not surfaced as its own column.

### Sync everything

```http
POST /api/moment/sync/full
Content-Type: application/json

{}
```

Optionally restrict to specific entities:
```json
{ "entities": ["projects", "time_records", "invoices"] }
```

### Sync one entity

```http
POST /api/moment/sync/gcp
Content-Type: application/json

{ "entity_type": "time_records" }
```

Valid `entity_type`/`entities` values: `projects`, `customers`, `user_accounts`, `time_records`, `tags`, `invoices` (also loads a child `moment_invoice_lines` table), `offers`, `invoicing_currency_setups`, `project_membership_roles`, `expense_categories`, `custom_fields`, `departments`, `absences`.

> `offers` and `custom_fields` had zero rows in the tenant this was built against, so their BigQuery schema is currently just `id` + `raw_json` + audit fields - extend `OFFERS_SCHEMA`/`CUSTOM_FIELDS_SCHEMA` in `bigquery_sync.py` once real data is available to confirm field names.

### Check sync status

```http
GET /api/gcp/status
```

Returns, per entity, whether its table exists, its row count, and when it was last synced. This iterates every table with a live query, so it can take 15-30s.

### Query synced data back out

```http
GET /api/moment/bigquery/data?entity_type=time_records&limit=50&project_id=123
```

### Run a sync from the command line

```bash
python run_sync.py                          # all entities
python run_sync.py --entities time_records,invoices
python run_sync.py --list                   # valid entity names
```

Exit code is 0 only if every requested entity synced, so cron/CI can alert on failures.

### Scheduled sync (production VPS)

The Hostinger VPS runs the service as the `aquatiq-moment-service` container and a host
crontab triggers a full sync **every Sunday at 03:00**:

```cron
0 3 * * 0 docker exec aquatiq-moment-service python run_sync.py >> /var/log/moment-sync.log 2>&1
```

Deploying/updating the VPS is one script (see [deploy/deploy.sh](deploy/deploy.sh)):

```bash
bash deploy.sh              # pull latest, build, start container, install cron
bash deploy.sh --sync-now   # same + trigger one full sync immediately
```

The two secret files are gitignored and must be copied to the VPS once by hand:
`.env` (from `.env.production`) and `app/gcp-credentials.json`.

**Why reruns are safe (no duplicates, no gaps, no data loss):** every run fetches *all*
pages of every entity and applies one atomic `MERGE` per table keyed on `id` - inserts
new records, updates existing ones, and soft-flags (never deletes) records that
disappeared from Moment. A rerun converges on the same state, a failed run leaves the
table untouched, and running twice on the same day is harmless.

## Docker Deployment

### Build and Run

```bash
# Build the image
docker build -t moment_service .

# Run with docker-compose (includes Redis and PostgreSQL)
docker-compose up -d

# Check logs
docker-compose logs -f moment_service

# Stop services
docker-compose down
```

### Docker Compose Services

The `docker-compose.yml` includes:

- **moment_service**: Main Flask application (port 5006)
- **redis**: Redis for caching (port 6379)
- **postgres**: PostgreSQL for sync data (port 5432)

## Development

### Project Structure

```
moment_service/
├── app.py                 # Main Flask application
├── config/
│   ├── __init__.py
│   └── settings.py        # Configuration management
├── clients/
│   ├── __init__.py
│   └── moment_api_client.py  # Moment API client
├── sync/                  # Sync utilities (future)
├── utils/                 # Helper utilities (future)
├── requirements.txt       # Python dependencies
├── Dockerfile            # Docker image definition
├── docker-compose.yml    # Multi-container setup
├── .env                  # Environment variables (not committed)
├── .env.example          # Example environment file
└── README.md             # This file
```

### Adding New Endpoints

1. Add method to `MomentAPIClient` in `clients/moment_api_client.py`
2. Add Flask route in `app.py`
3. Test the endpoint

Example:

```python
# In clients/moment_api_client.py
def get_offers(self, includes=None, params=None):
    return self.get_list("offers", includes=includes, params=params)

# In app.py
@app.route("/api/offers", methods=["GET"])
def get_offers():
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    offers = moment_client.get_offers(includes=includes, params=params or None)
    return jsonify(offers)
```

### Rate Limiting

Moment API enforces:
- **120 requests/second per IP**
- **30 requests/second per token**

The service automatically handles rate limit errors (HTTP 429) and returns appropriate responses.

## Troubleshooting

### Authentication Error (401)

**Problem**: Getting 401 Unauthorized errors.

**Solution**:
- Verify your API key in `.env`
- Check that `MOMENT_COMPANY_CODE` matches your Moment database
- Ensure API Access integration is enabled in Moment
- Confirm API key hasn't expired (max 2 years validity)

### Rate Limit Exceeded (429)

**Problem**: Getting 429 Too Many Requests errors.

**Solution**:
- Reduce request frequency
- Implement exponential backoff
- Use batch operations for bulk updates
- Consider caching frequently accessed data

### Connection Errors

**Problem**: Cannot connect to Moment API.

**Solution**:
- Check internet connectivity
- Verify `MOMENT_BASE_URL` is correct
- Check firewall/proxy settings
- Ensure Moment service is operational

### Missing Company Code

**Problem**: `ValueError: MOMENT_COMPANY_CODE is required`

**Solution**:
- Set `MOMENT_COMPANY_CODE` in `.env`
- Find it in your Moment URL: `https://app.moment.team/team/{company_code}/...`

### Docker Issues

**Problem**: Container won't start or health check fails.

**Solution**:
```bash
# Check logs
docker-compose logs moment_service

# Restart services
docker-compose restart

# Rebuild image
docker-compose build --no-cache
docker-compose up -d
```

## API Rate Limits

Moment enforces the following rate limits:

| Limit Type | Value |
|------------|-------|
| Per IP address | 120 requests/second |
| Per API token | 30 requests/second |

When rate limit is exceeded, you'll receive HTTP 429 with `Retry-After` header.

## Support & Documentation

- **Moment API Docs**: https://app.moment.team/api/1.0/docs/
- **Moment Support**: Contact via Moment application
- **Service Issues**: Check logs with `docker-compose logs -f`

## License

Internal use only - Aquatiq Integration Platform

## Changelog

### Version 1.0.0 (2024-12-22)

- Initial release
- Full CRUD operations for projects, time records, customers, users, tags, invoices
- Batch operations support
- Includes concept implementation
- Rate limiting handling
- Docker deployment support
- Comprehensive error handling
