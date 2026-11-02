# SuperOffice CRM Integration Service

A microservice for integrating SuperOffice CRM with the Aquatiq platform. This service provides OAuth authentication, data synchronization, and API access to SuperOffice CRM resources.

## Features

- **OAuth 2.0 Authentication**: Secure authentication with SuperOffice CRM
- **Contact Management**: Sync and manage contacts from SuperOffice
- **Company Management**: Sync and manage companies from SuperOffice
- **Sales Tracking**: Access and sync sales data
- **Caching**: Built-in caching for improved performance
- **Multi-tenant Support**: Support for multiple organizations

## Architecture

```
socrm_service/
├── app.py                 # Main Flask application
├── config/                # Configuration management
│   └── settings.py
├── auth/                  # OAuth and authentication
│   └── oauth_handler.py
├── clients/               # SuperOffice API clients
│   └── socrm_client.py
├── sync/                  # Data synchronization
│   ├── contact_sync.py
│   ├── company_sync.py
│   └── sale_sync.py
├── cache/                 # Caching layer
│   └── cache_manager.py
└── utils/                 # Utilities
    ├── error_handler.py
    └── logger.py
```

## Prerequisites

- Python 3.12+
- Docker and Docker Compose
- PostgreSQL 15+
- Redis 7+
- SuperOffice CRM account with OAuth credentials

## Setup

### 1. SuperOffice Application Setup

1. Register your application at [SuperOffice Developer Portal](https://developer.superoffice.com/)
2. Create an OAuth application
3. Note down your `Client ID` and `Client Secret`
4. Configure the redirect URI: `http://localhost:5006/api/auth/callback`

### 2. Environment Configuration

Create a `.env` file in the service directory:

```env
# SuperOffice OAuth settings
SOCRM_CLIENT_ID=your_client_id
SOCRM_CLIENT_SECRET=your_client_secret
SOCRM_REDIRECT_URI=http://localhost:5006/api/auth/callback
SOCRM_TENANT=sod

# Service settings
SOCRM_HOST=0.0.0.0
SOCRM_PORT=5006

# Database
DATABASE_URL=postgresql://postgres:postgres@postgres:5432/aquatiq_db

# Cache settings
CACHE_DIR=/app/cache
CACHE_TTL=3600

# Logging
LOG_LEVEL=INFO
```

### 3. Installation

#### Using Docker (Recommended)

```bash
# Build and start the service
docker-compose up -d

# View logs
docker-compose logs -f socrm-service
```

#### Local Development

```bash
# Create virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt

# Run the service
python app.py
```

## API Endpoints

### Health Check

```http
GET /health
```

Returns service health status.

### Authentication

#### Initialize Authorization

```http
GET /api/auth/authorize?organization_id={org_id}
```

Returns the SuperOffice authorization URL.

#### OAuth Callback

```http
GET /api/auth/callback?code={code}&state={state}
```

Handles the OAuth callback from SuperOffice.

### Contacts

#### Get Contacts

```http
GET /api/contacts?organization_id={org_id}
```

Returns all contacts from SuperOffice CRM.

#### Sync Contacts

```http
POST /api/sync/contacts
Content-Type: application/json

{
  "organization_id": "org_id"
}
```

Triggers contact synchronization.

### Companies

#### Get Companies

```http
GET /api/companies?organization_id={org_id}
```

Returns all companies from SuperOffice CRM.

#### Sync Companies

```http
POST /api/sync/companies
Content-Type: application/json

{
  "organization_id": "org_id"
}
```

Triggers company synchronization.

### Sales

#### Get Sales

```http
GET /api/sales?organization_id={org_id}
```

Returns all sales from SuperOffice CRM.

#### Sync Sales

```http
POST /api/sync/sales
Content-Type: application/json

{
  "organization_id": "org_id"
}
```

Triggers sales synchronization.

## Usage Example

### 1. Authenticate with SuperOffice

```python
import requests

# Get authorization URL
response = requests.get(
    "http://localhost:5006/api/auth/authorize",
    params={"organization_id": "my_org_123"}
)
auth_url = response.json()["authorization_url"]

# User visits auth_url and grants permission
# SuperOffice redirects to callback URL with code
```

### 2. Get Contacts

```python
# Fetch contacts
response = requests.get(
    "http://localhost:5006/api/contacts",
    params={"organization_id": "my_org_123"}
)
contacts = response.json()["contacts"]
```

### 3. Sync Data

```python
# Trigger contact sync
response = requests.post(
    "http://localhost:5006/api/sync/contacts",
    json={"organization_id": "my_org_123"}
)
result = response.json()
```

## Configuration

### SuperOffice Tenant

The service supports different SuperOffice environments:
- `sod` - Development environment (default)
- `stage` - Staging environment
- `online` - Production environment

Set via `SOCRM_TENANT` environment variable.

### Caching

- **Cache Directory**: Configurable via `CACHE_DIR`
- **Cache TTL**: Default 3600 seconds (1 hour)
- **Redis**: Used for distributed caching in production

### Logging

Logging levels: DEBUG, INFO, WARNING, ERROR, CRITICAL

Set via `LOG_LEVEL` environment variable.

## Development

### Running Tests

```bash
# Install dev dependencies
pip install -r requirements-dev.txt

# Run tests
pytest

# Run with coverage
pytest --cov=. --cov-report=html
```

### Code Quality

```bash
# Format code
black .
isort .

# Lint code
ruff check .

# Type checking
mypy .
```

## Monitoring

### Health Check

The service provides a health check endpoint at `/health`:

```bash
curl http://localhost:5006/health
```

### Logs

View service logs:

```bash
# Docker
docker-compose logs -f socrm-service

# Local
tail -f logs/socrm_service.log
```

## Troubleshooting

### OAuth Issues

1. Verify `SOCRM_CLIENT_ID` and `SOCRM_CLIENT_SECRET` are correct
2. Ensure redirect URI matches SuperOffice app configuration
3. Check `SOCRM_TENANT` is set correctly

### Connection Issues

1. Verify database is accessible
2. Check Redis is running
3. Ensure firewall allows connections on port 5006

### Sync Issues

1. Check OAuth token is valid
2. Verify SuperOffice API permissions
3. Review logs for specific errors

## Security

- **OAuth Tokens**: Stored securely in PostgreSQL with encryption
- **HTTPS**: Use HTTPS in production
- **Secrets**: Never commit `.env` files to version control
- **Non-root User**: Container runs as non-root user

## License

Proprietary - Aquatiq Platform

## Support

For issues or questions, contact the Aquatiq development team.
