# Currency Service

Production-ready microservice for real-time exchange rate aggregation and currency conversion. Part of the Aquatiq Integration Platform.

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Architecture](#architecture)
- [Quick Start](#quick-start)
- [API Reference](#api-reference)
- [Data Sources](#data-sources)
- [Configuration](#configuration)
- [Deployment](#deployment)
- [Cache Management](#cache-management)
- [Supported Currencies](#supported-currencies)
- [Development](#development)
- [Troubleshooting](#troubleshooting)

## Overview

The Currency Service provides:
- **Multi-source exchange rate aggregation** from Norges Bank and Abstract API
- **Real-time currency conversion** with NOK as pivot currency
- **Intelligent caching** with configurable TTL
- **Historical rate support** for time-series analysis
- **RESTful API** with comprehensive error handling

**Port**: 5004  
**Protocol**: HTTP REST  
**Base URL**: `http://localhost:5004`

## Features

### Core Capabilities

✅ **12 Supported Currencies**: USD, EUR, GBP, SEK, DKK, CAD, ISK, AUD, NZD, IDR, JPY, CLP  
✅ **Multi-source Aggregation**: Primary (Norges Bank) + Fallback (Abstract API)  
✅ **File-based Caching**: 1-hour TTL with automatic expiry  
✅ **Currency Conversion**: NOK pivot for accurate cross-currency rates  
✅ **Historical Rates**: Query rates for specific dates  
✅ **Health Monitoring**: Built-in health check endpoint  
✅ **Cache Management**: Clear all or expired entries only  

### Technical Highlights

- **Zero dependencies on other microservices**: Independent operation
- **CORS enabled**: Ready for frontend integration
- **Comprehensive logging**: Structured logs for debugging
- **Error handling**: Graceful degradation with detailed error messages
- **Docker support**: Containerized deployment with health checks

## Architecture

```
┌─────────────────────────────────────────┐
│         Currency Service (5004)         │
├─────────────────────────────────────────┤
│  ┌──────────────────────────────────┐  │
│  │      Flask REST API              │  │
│  │  - /health                       │  │
│  │  - /api/currency/rates           │  │
│  │  - /api/currency/convert         │  │
│  │  - /api/currency/sources         │  │
│  │  - /api/currency/cache/*         │  │
│  └──────────────────────────────────┘  │
│             ▼                           │
│  ┌──────────────────────────────────┐  │
│  │      Rate Cache (TTL: 1h)        │  │
│  │   File-based JSON storage        │  │
│  └──────────────────────────────────┘  │
│             ▼                           │
│  ┌──────────────────────────────────┐  │
│  │   Currency Converter             │  │
│  │   (NOK pivot conversion)         │  │
│  └──────────────────────────────────┘  │
│             ▼                           │
│  ┌─────────────┬──────────────────┐    │
│  │ Norges Bank │  Abstract API    │    │
│  │   Source    │    Source        │    │
│  │  (11 curr)  │    (CLP)         │    │
│  └─────────────┴──────────────────┘    │
└─────────────────────────────────────────┘
         ▼              ▼
    ┌────────┐    ┌──────────┐
    │Norges  │    │Abstract  │
    │Bank API│    │API       │
    └────────┘    └──────────┘
```

### Data Flow

1. **Client Request** → Flask API endpoint
2. **Cache Check** → Return cached data if valid
3. **Source Routing** → Determine appropriate data source(s)
4. **Data Fetching** → Retrieve rates from Norges Bank/Abstract API
5. **Data Processing** → Parse, normalize, merge results
6. **Cache Update** → Store with timestamp
7. **Response** → Return JSON with rates + metadata

## Quick Start

### Prerequisites

- Python 3.12+
- Docker & Docker Compose (for containerized deployment)
- Abstract API key (for CLP currency)

### Local Development

1. **Clone and navigate**:
```bash
cd apps/currency_service
```

2. **Create virtual environment**:
```bash
python -m venv venv
source venv/bin/activate  # macOS/Linux
```

3. **Install dependencies**:
```bash
pip install -r requirements.txt
```

4. **Configure environment**:
```bash
cp .env.example .env
# Edit .env and add your ABSTRACT_API_KEY
```

5. **Run service**:
```bash
python app.py
```

Service starts on `http://localhost:5004`

### Docker Deployment

1. **Build and run**:
```bash
docker-compose up -d
```

2. **Verify health**:
```bash
curl http://localhost:5004/health
```

3. **View logs**:
```bash
docker-compose logs -f currency_service
```

## API Reference

### Health Check

Check service status and data source availability.

**Endpoint**: `GET /health`

**Response**:
```json
{
  "status": "healthy",
  "service": "currency_service",
  "version": "1.0.0",
  "sources": {
    "norges_bank": "operational",
    "abstract_api": "operational"
  }
}
```

### Get Exchange Rates

Fetch current or historical exchange rates.

**Endpoint**: `GET /api/currency/rates`

**Query Parameters**:
- `base` (optional): Base currency code (default: NOK)
- `targets` (required): Comma-separated list of target currencies
- `date` (optional): Date in YYYY-MM-DD format for historical rates

**Example Request**:
```bash
curl "http://localhost:5004/api/currency/rates?base=NOK&targets=USD,EUR,GBP"
```

**Response**:
```json
{
  "base": "NOK",
  "date": "2024-01-15",
  "rates": {
    "USD": 0.0956,
    "EUR": 0.0875,
    "GBP": 0.0752
  },
  "metadata": {
    "source": "norges_bank",
    "cached": false,
    "cache_ttl": 3600
  }
}
```

**Historical Rates Example**:
```bash
curl "http://localhost:5004/api/currency/rates?targets=USD,EUR&date=2024-01-01"
```

### Convert Currency

Convert amount between currencies.

**Endpoint**: `GET /api/currency/convert`

**Query Parameters**:
- `from` (required): Source currency code
- `to` (required): Target currency code
- `amount` (required): Amount to convert (numeric)
- `date` (optional): Date for historical conversion

**Example Request**:
```bash
curl "http://localhost:5004/api/currency/convert?from=USD&to=EUR&amount=100"
```

**Response**:
```json
{
  "from_currency": "USD",
  "to_currency": "EUR",
  "amount": 100.0,
  "result": 91.52,
  "rate": 0.9152,
  "via": "NOK",
  "date": "2024-01-15"
}
```

**Direct Conversion (NOK involved)**:
```bash
curl "http://localhost:5004/api/currency/convert?from=NOK&to=USD&amount=1000"
```

### List Data Sources

Get information about available data sources.

**Endpoint**: `GET /api/currency/sources`

**Response**:
```json
{
  "sources": [
    {
      "name": "norges_bank",
      "description": "Norges Bank (Central Bank of Norway)",
      "url": "https://data.norges-bank.no/api/data/EXR",
      "currencies": ["USD", "EUR", "GBP", "SEK", "DKK", "CAD", "ISK", "AUD", "NZD", "IDR", "JPY"],
      "priority": "primary",
      "features": ["latest_rates", "historical_rates"]
    },
    {
      "name": "abstract_api",
      "description": "Abstract API Exchange Rates",
      "url": "https://api.abstractapi.com/v1/exchange-rates",
      "currencies": ["CLP"],
      "priority": "fallback",
      "features": ["latest_rates", "historical_rates"],
      "requires_api_key": true
    }
  ]
}
```

### Clear Cache

Remove all cached rate data.

**Endpoint**: `POST /api/currency/cache/clear`

**Response**:
```json
{
  "success": true,
  "message": "Cache cleared successfully"
}
```

### Clear Expired Cache

Remove only expired cache entries.

**Endpoint**: `POST /api/currency/cache/clear-expired`

**Response**:
```json
{
  "success": true,
  "message": "Expired cache entries cleared"
}
```

## Data Sources

### Norges Bank (Primary)

**Provider**: Central Bank of Norway  
**API**: SDMX CSV format  
**Update Frequency**: Daily  
**Currencies**: 11 (USD, EUR, GBP, SEK, DKK, CAD, ISK, AUD, NZD, IDR, JPY)  
**Quote Currency**: NOK  
**Special Handling**: JPY uses 100-unit multiplier

**Features**:
- Official central bank rates
- Historical data available
- No API key required
- High reliability

**Rate Format**:
- CSV with semicolon delimiter
- Unit multiplier field (0=1, 2=100)
- Time series data with dates

### Abstract API (Fallback)

**Provider**: Abstract API commercial service  
**API**: REST JSON  
**Update Frequency**: Near real-time  
**Currencies**: 1 primary (CLP) + 170+ available  
**Quote Currency**: Configurable (uses NOK)  
**Authentication**: API key required

**Features**:
- Covers currencies not in Norges Bank
- Rate inversion for NOK base
- Historical rate support
- Requires subscription

**Rate Inversion**:
```python
# Abstract returns NOK/CLP, we need CLP/NOK
adjusted_rate = 1 / api_rate
```

## Configuration

### Environment Variables

| Variable | Description | Default | Required |
|----------|-------------|---------|----------|
| `CURRENCY_PORT` | Service port | 5004 | No |
| `CURRENCY_HOST` | Bind address | 0.0.0.0 | No |
| `ABSTRACT_API_KEY` | Abstract API key | - | Yes* |
| `CURRENCY_CACHE_DIR` | Cache directory | ./cache/rates | No |
| `CACHE_TTL` | Cache TTL (seconds) | 3600 | No |
| `FLASK_ENV` | Flask environment | production | No |
| `FLASK_DEBUG` | Debug mode | false | No |
| `LOG_LEVEL` | Logging level | INFO | No |

*Required only for CLP currency

### Cache Settings

**Default TTL**: 3600 seconds (1 hour)  
**Storage**: File-based JSON  
**Location**: `./cache/rates/`  
**Format**: `rates_{base}_{targets}_{date}.json`

**Cache Structure**:
```json
{
  "cached_at": "2024-01-15T10:30:00Z",
  "data": {
    "base": "NOK",
    "rates": { ... }
  }
}
```

### Supported Currencies

| Code | Name | Source | Notes |
|------|------|--------|-------|
| USD | US Dollar | Norges Bank | - |
| EUR | Euro | Norges Bank | - |
| GBP | British Pound | Norges Bank | - |
| SEK | Swedish Krona | Norges Bank | - |
| DKK | Danish Krone | Norges Bank | - |
| CAD | Canadian Dollar | Norges Bank | - |
| ISK | Icelandic Króna | Norges Bank | - |
| AUD | Australian Dollar | Norges Bank | - |
| NZD | New Zealand Dollar | Norges Bank | - |
| IDR | Indonesian Rupiah | Norges Bank | - |
| JPY | Japanese Yen | Norges Bank | 100-unit multiplier |
| CLP | Chilean Peso | Abstract API | Requires API key |

## Deployment

### Docker Production Deployment

1. **Set environment variables**:
```bash
cp .env.example .env
nano .env  # Add ABSTRACT_API_KEY
```

2. **Build image**:
```bash
docker build -t currency-service:latest .
```

3. **Run container**:
```bash
docker run -d \
  --name currency_service \
  -p 5004:5004 \
  --env-file .env \
  -v $(pwd)/cache:/app/cache \
  --restart unless-stopped \
  currency-service:latest
```

4. **Verify deployment**:
```bash
docker logs currency_service
curl http://localhost:5004/health
```

### Docker Compose (Recommended)

```bash
docker-compose up -d
docker-compose ps
docker-compose logs -f currency_service
```

### Health Checks

**Docker Healthcheck**:
- Interval: 30s
- Timeout: 5s
- Retries: 3
- Start period: 40s

**Manual Check**:
```bash
curl -f http://localhost:5004/health || echo "Service unhealthy"
```

## Cache Management

### Cache Strategy

The service uses a **file-based cache** with automatic expiry:

- **Cache Hit**: Return cached data immediately
- **Cache Miss**: Fetch from source, cache, return
- **Cache Expired**: Fetch fresh data, update cache
- **Cache Key**: `rates_{base}_{sorted_targets}_{date}`

### Manual Cache Operations

**Clear all cache**:
```bash
curl -X POST http://localhost:5004/api/currency/cache/clear
```

**Clear expired only**:
```bash
curl -X POST http://localhost:5004/api/currency/cache/clear-expired
```

**View cache directory**:
```bash
ls -lh cache/rates/
```

### Cache Performance

| Operation | Cache Hit | Cache Miss |
|-----------|-----------|------------|
| Response Time | ~5-10ms | ~200-500ms |
| API Calls | 0 | 1-2 |
| Data Freshness | Up to 1h old | Real-time |

## Development

### Project Structure

```
currency_service/
├── app.py                  # Flask application entry point
├── requirements.txt        # Python dependencies
├── Dockerfile             # Container build instructions
├── docker-compose.yml     # Container orchestration
├── .env.example           # Environment template
├── README.md              # This file
├── config/
│   ├── __init__.py
│   └── settings.py        # Configuration dataclass
├── sources/
│   ├── __init__.py
│   ├── norges_bank.py     # Norges Bank API client
│   └── abstract_api.py    # Abstract API client
├── cache/
│   ├── __init__.py
│   └── rate_cache.py      # File-based cache manager
├── converters/
│   ├── __init__.py
│   └── currency_converter.py  # Currency conversion logic
└── cache/
    └── rates/             # Cache storage directory
```

### Running Tests

```bash
# Unit tests
pytest tests/

# Integration tests
pytest tests/ -m integration

# With coverage
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

# Type check
mypy .
```

## Troubleshooting

### Common Issues

#### Service Won't Start

**Symptom**: Service fails to start or exits immediately

**Solutions**:
1. Check port availability:
```bash
lsof -i :5004
```

2. Verify Python version:
```bash
python --version  # Should be 3.12+
```

3. Check environment variables:
```bash
cat .env
```

#### Cache Not Working

**Symptom**: Always fetching fresh data, never using cache

**Solutions**:
1. Verify cache directory exists:
```bash
mkdir -p cache/rates
```

2. Check permissions:
```bash
chmod 755 cache/rates
```

3. Clear corrupted cache:
```bash
rm -rf cache/rates/*
```

#### API Errors

**Symptom**: 500 errors or "Source unavailable"

**Solutions**:
1. **Norges Bank unreachable**:
   - Check internet connectivity
   - Verify DNS resolution: `nslookup data.norges-bank.no`
   - Check API status: https://data.norges-bank.no

2. **Abstract API errors**:
   - Verify API key in `.env`
   - Check quota limits
   - Test API key: `curl https://api.abstractapi.com/v1/exchange-rates?api_key=YOUR_KEY&base=USD`

3. **CSV parsing errors**:
   - Check logs for malformed CSV data
   - Verify Norges Bank API response format
   - Clear cache and retry

#### Rate Inversion Issues

**Symptom**: CLP rates seem incorrect or inverted

**Explanation**: Abstract API returns NOK/CLP, service inverts to CLP/NOK for consistency.

**Verification**:
```python
# If Abstract returns 0.0125 (NOK per CLP)
# Service returns 80.0 (CLP per NOK)
adjusted_rate = 1 / 0.0125  # = 80.0
```

#### Docker Issues

**Container exits immediately**:
```bash
docker logs currency_service  # Check error messages
docker-compose down && docker-compose up  # Restart
```

**Health check failing**:
```bash
docker exec currency_service curl http://localhost:5004/health
```

### Logging

**View logs in Docker**:
```bash
docker-compose logs -f currency_service
```

**Log levels**:
- `DEBUG`: Detailed source API calls, cache operations
- `INFO`: Request handling, rate fetching
- `WARNING`: Missing API keys, cache issues
- `ERROR`: API failures, parsing errors

**Enable debug logging**:
```bash
export LOG_LEVEL=DEBUG
python app.py
```

### Performance Tips

1. **Optimize Cache TTL**: Balance freshness vs API load
2. **Use Specific Targets**: Only request needed currencies
3. **Batch Conversions**: Use `/rates` + local calculation for multiple conversions
4. **Monitor Cache Hit Rate**: High hit rate = good performance

### Getting Help

**Check logs first**:
```bash
docker-compose logs currency_service | grep ERROR
```

**Test individual sources**:
```bash
python -c "from sources.norges_bank import NorgesBankSource; s = NorgesBankSource(); print(s.get_latest_rates(['USD']))"
```

**Verify configuration**:
```bash
python -c "from config.settings import currency_config; print(currency_config.SERVICE_PORT)"
```

---

## License

Part of the Aquatiq Integration Platform.

## Support

For issues, questions, or contributions, contact the Aquatiq development team.
