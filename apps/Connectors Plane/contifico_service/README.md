# Contifico ERP Service

Integration service for Contifico ERP system with automatic Spanish-to-English translation.

## Overview

This service provides a REST API interface to Contifico ERP, with built-in translation of Spanish field names and values to English for easier integration with international systems.

## Features

- **Complete API Coverage**: All Contifico API endpoints (Inventory, Transactions, Persons, Accounting, Banking)
- **Automatic Translation**: Spanish field names and values automatically translated to English
- **Rate Limiting**: Built-in rate limiting to prevent API quota exhaustion
- **Retry Logic**: Automatic retries with exponential backoff for failed requests
- **BigQuery Sync**: Automatic synchronization to Google BigQuery
- **Health Monitoring**: Built-in health check endpoints
- **Docker Support**: Complete containerization with Docker and docker-compose

## API Documentation

Contifico API documentation: http://contifico.github.io/

### Test Credentials

For testing purposes, use:

- **API Key**: `FrguR1kDpFHaXHLQwplZ2CwTX3p8p9XHVTnukL98V5U`
- **API Token**: `02914770-4a13-45f0-bfe3-c2e4666cdbcf`
- **Web Access**: www.contifico.com -> Contifico Online
  - Usuario: `APIPRUEBA`
  - Contraseña: `APIPRUEBA`

## Installation

### Local Development

```bash
# Install dependencies
pip install -r requirements.txt

# Copy environment file
cp .env.example .env

# Edit .env with your credentials
nano .env

# Run the service
python app.py
```

### Docker

```bash
# Build and run with docker-compose
docker-compose up -d

# View logs
docker-compose logs -f contifico-service

# Stop services
docker-compose down
```

## Configuration

### Environment Variables

See `.env.example` for all available configuration options.

Key variables:
- `CONTIFICO_API_KEY`: Your Contifico API key
- `CONTIFICO_API_TOKEN`: Your Contifico API token
- `CONTIFICO_API_BASE_URL`: API base URL (default: https://api.contifico.com/sistema/api/v1)
- `DATABASE_URL`: PostgreSQL connection string
- `GCP_PROJECT_ID`: Google Cloud project ID for BigQuery sync

## API Endpoints

### Health Check

```bash
GET /health
```

### Inventory

```bash
# Categories
GET /api/inventory/categories
GET /api/inventory/categories/{id}

# Warehouses
GET /api/inventory/warehouses
GET /api/inventory/warehouses/{id}

# Products
GET /api/inventory/products
POST /api/inventory/products
GET /api/inventory/products/{id}
PATCH /api/inventory/products/{id}
GET /api/inventory/products/{id}/stock

# Inventory Movements
GET /api/inventory/movements
POST /api/inventory/movements

# Brands
GET /api/inventory/brands
```

### Transactions

```bash
# Documents
GET /api/transactions/documents
POST /api/transactions/documents
PUT /api/transactions/documents
GET /api/transactions/documents/{id}
PUT /api/transactions/documents/{id}/sri

# Collections
GET /api/transactions/documents/{id}/collections
POST /api/transactions/documents/{id}/collections

# Transactions
GET /api/transactions
GET /api/transactions/{id}
```

### Persons (Customers/Suppliers)

```bash
GET /api/persons
POST /api/persons
PUT /api/persons
GET /api/persons/{id}
```

### Accounting

```bash
GET /api/accounting/cost-centers
GET /api/accounting/accounts
POST /api/accounting/journal-entries
GET /api/accounting/journal-entries/{id}
```

### Banking

```bash
GET /api/bank/accounts
GET /api/bank/movements
```

## Translation Feature

The service automatically translates Spanish field names to English:

### Example Translation

**Original Contifico Response (Spanish):**
```json
{
  "id": 123,
  "nombre": "Producto Ejemplo",
  "descripcion": "Descripción del producto",
  "precio": 99.99,
  "cantidad": 10,
  "categoria": "Electrónica",
  "estado": "activo"
}
```

**Translated Response (English):**
```json
{
  "id": 123,
  "name": "Producto Ejemplo",
  "description": "Descripción del producto",
  "price": 99.99,
  "quantity": 10,
  "category": "Electrónica",
  "status": "active"
}
```

### Disabling Translation

To receive responses in original Spanish:

```python
from clients.contifico_client import ContificoClient

# Initialize client without translation
client = ContificoClient(translate_responses=False)
```

## Usage Examples

### Python Client

```python
from clients.contifico_client import ContificoClient

# Initialize client
client = ContificoClient()

# Get all products (automatically translated to English)
products = client.get_products()

# Get specific product
product = client.get_product(product_id=123)

# Create new product
new_product = client.create_product({
    "name": "New Product",
    "description": "Product description",
    "price": 99.99,
    "category_id": 1
})

# Get product stock
stock = client.get_product_stock(product_id=123)
```

### HTTP Requests

```bash
# Get all products
curl -X GET http://localhost:5006/api/inventory/products

# Create a document
curl -X POST http://localhost:5006/api/transactions/documents \
  -H "Content-Type: application/json" \
  -d '{
    "type": "invoice",
    "customer_id": 123,
    "items": [...]
  }'

# Get bank movements
curl -X GET http://localhost:5006/api/bank/movements
```

## Error Handling

The service provides detailed error responses:

```json
{
  "error": "contifico_api_error",
  "message": "Unauthorized: Invalid API credentials"
}
```

Common error codes:
- `401`: Invalid credentials
- `403`: Access denied
- `404`: Resource not found
- `429`: Rate limit exceeded
- `500`: Internal server error

## Rate Limiting

Default limits (configurable in `.env`):
- 60 requests per minute
- 5000 requests per day

## Development

### Running Tests

```bash
pytest
pytest --cov=. --cov-report=html
```

### Code Quality

```bash
# Format code
black .

# Sort imports
isort .

# Lint
ruff check .

# Type checking
mypy .
```

## Production Deployment

1. Update `.env.production` with production credentials
2. Build production Docker image
3. Deploy to your infrastructure

```bash
# Build production image
docker build -t contifico-service:latest .

# Run in production
docker run -d \
  --name aquatiq-contifico-service \
  --env-file .env.production \
  -p 8000:8000 \
  contifico-service:latest
```

## Support

For Contifico API support, contact Contifico customer support.

For service issues, create an issue in the repository.

## License

Proprietary - Aquatiq Integration Platform
