# Currency Service - Complete Backend Implementation

## Overview
Complete microservice for currency exchange rate management with database persistence, scheduled synchronization, and full CRUD operations.

**Version:** 2.0.0  
**Port:** 5004  
**Stack:** Flask, PostgreSQL, SQLAlchemy, APScheduler, Redis

---

## 🎯 Features Implemented

### 1. **Full CRUD Operations**
Complete Create, Read, Update, Delete functionality for exchange rates with database persistence.

#### Endpoints:
- `GET /api/currency/rates` - List all rates (with filters)
- `GET /api/currency/rates/:id` - Get specific rate
- `POST /api/currency/rates` - Create new rate
- `PUT /api/currency/rates/:id` - Update rate
- `DELETE /api/currency/rates/:id` - Delete rate

### 2. **Database Integration**
- **PostgreSQL** persistence with SQLAlchemy ORM
- Three main tables:
  - `exchange_rates` - Currency exchange rates
  - `sync_logs` - Synchronization history
  - `service_logs` - Service activity logs
- Unique constraints on currency pairs
- Indexed queries for performance
- Automatic timestamp tracking

### 3. **Scheduled Cron Jobs**
- **APScheduler** for background task scheduling
- Hourly automatic rate synchronization (at minute 0)
- Initial sync on service startup
- Configurable schedule patterns
- Job status monitoring

### 4. **Multi-Source Data Aggregation**
- **Norges Bank** - Primary source for major currencies (11 currencies)
- **Abstract API** - Additional currencies (e.g., CLP)
- Automatic source selection
- Cross-rate calculation via NOK

### 5. **Comprehensive Monitoring**
- Service health checks
- Database connectivity monitoring
- External API status tracking
- Sync history with success/failure tracking
- Detailed logging (INFO, WARNING, ERROR, DEBUG)

### 6. **Rate Conversion**
- Direct conversion between currencies
- Cross-rate calculation via NOK
- Real-time rate lookup from database

---

## 📡 API Endpoints

### Health & Status

#### `GET /health`
Service health check with detailed status.

**Response:**
```json
{
  "status": "healthy",
  "service": "currency_service",
  "version": "2.0.0",
  "database": "healthy",
  "scheduler": "running",
  "sources": {
    "norges_bank": "available",
    "abstract_api": "available"
  },
  "timestamp": "2025-12-10T12:00:00Z"
}
```

#### `GET /api/currency/status`
Comprehensive service status including cron jobs, logs, and sync history.

**Response:**
```json
{
  "success": true,
  "scheduler_running": true,
  "cron_jobs": [{
    "name": "Hourly Currency Rate Sync",
    "schedule": "cron[minute='0']",
    "last_run": "2025-12-10T12:00:00Z",
    "next_run": "2025-12-10T13:00:00+00:00",
    "status": "active",
    "last_error": null
  }],
  "services": [{
    "service": "Norges Bank",
    "status": "healthy",
    "last_check": "2025-12-10T12:00:00Z",
    "response_time_ms": 100
  }],
  "sync_history": [...],
  "logs": [...]
}
```

### CRUD Operations

#### `GET /api/currency/rates`
List all exchange rates.

**Query Parameters:**
- `from_currency` - Filter by source currency (e.g., NOK)
- `source` - Filter by source (norgesbank, abstract, manual)
- `latest` - If true, return only latest rate for each pair

**Response:**
```json
{
  "success": true,
  "count": 11,
  "rates": [{
    "id": 1,
    "from_currency": "NOK",
    "to_currency": "USD",
    "rate": 0.092,
    "source": "norgesbank",
    "timestamp": "2025-12-10T12:00:00Z",
    "created_at": "2025-12-10T10:00:00Z",
    "updated_at": "2025-12-10T12:00:00Z"
  }],
  "timestamp": "2025-12-10T12:00:00Z"
}
```

#### `GET /api/currency/rates/:id`
Get specific exchange rate by ID.

**Response:**
```json
{
  "success": true,
  "rate": {
    "id": 1,
    "from_currency": "NOK",
    "to_currency": "USD",
    "rate": 0.092,
    "source": "norgesbank",
    "timestamp": "2025-12-10T12:00:00Z"
  }
}
```

#### `POST /api/currency/rates`
Create a new exchange rate.

**Request Body:**
```json
{
  "from_currency": "BTC",
  "to_currency": "USD",
  "rate": 43000.50,
  "source": "manual"
}
```

**Response:**
```json
{
  "success": true,
  "rate": {
    "id": 14,
    "from_currency": "BTC",
    "to_currency": "USD",
    "rate": 43000.50,
    "source": "manual",
    "timestamp": "2025-12-10T12:00:00Z"
  }
}
```

#### `PUT /api/currency/rates/:id`
Update an existing exchange rate.

**Request Body:**
```json
{
  "rate": 43500.00
}
```

**Response:**
```json
{
  "success": true,
  "rate": {
    "id": 14,
    "rate": 43500.00,
    "updated_at": "2025-12-10T12:05:00Z"
  }
}
```

#### `DELETE /api/currency/rates/:id`
Delete an exchange rate.

**Response:**
```json
{
  "success": true,
  "message": "Rate deleted successfully"
}
```

### Conversion

#### `GET /api/currency/convert`
Convert amount between currencies.

**Query Parameters:**
- `from` - Source currency (required)
- `to` - Target currency (required)
- `amount` - Amount to convert (required)

**Response:**
```json
{
  "from": "USD",
  "to": "NOK",
  "amount": 100,
  "result": 1087.50,
  "rate": 10.875,
  "timestamp": "2025-12-10T12:00:00Z"
}
```

### Synchronization

#### `POST /api/currency/sync`
Manually trigger rate synchronization from external sources.

**Response:**
```json
{
  "success": true,
  "updated_count": 11,
  "status": "success",
  "errors": null,
  "timestamp": "2025-12-10T12:00:00Z"
}
```

### Logging

#### `GET /api/currency/logs`
Get service logs with optional filtering.

**Query Parameters:**
- `level` - Filter by level (INFO, WARNING, ERROR, DEBUG)
- `limit` - Maximum number of logs (default: 100)

**Response:**
```json
{
  "success": true,
  "count": 50,
  "logs": [{
    "id": 1,
    "level": "INFO",
    "message": "Synced 11 rates from Norges Bank",
    "details": null,
    "timestamp": "2025-12-10T12:00:00Z"
  }]
}
```

---

## 🗄️ Database Schema

### exchange_rates
```sql
CREATE TABLE exchange_rates (
    id SERIAL PRIMARY KEY,
    from_currency VARCHAR(3) NOT NULL,
    to_currency VARCHAR(3) NOT NULL,
    rate FLOAT NOT NULL,
    source VARCHAR(50) NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(from_currency, to_currency, source)
);

CREATE INDEX idx_currency_pair ON exchange_rates(from_currency, to_currency);
CREATE INDEX idx_timestamp ON exchange_rates(timestamp);
```

### sync_logs
```sql
CREATE TABLE sync_logs (
    id SERIAL PRIMARY KEY,
    sync_type VARCHAR(50) NOT NULL,  -- 'manual', 'scheduled', 'api'
    status VARCHAR(20) NOT NULL,      -- 'success', 'partial', 'failed'
    rates_updated INTEGER DEFAULT 0,
    errors VARCHAR(1000),
    started_at TIMESTAMP NOT NULL,
    completed_at TIMESTAMP,
    duration_ms INTEGER
);

CREATE INDEX idx_started_at ON sync_logs(started_at);
CREATE INDEX idx_status ON sync_logs(status);
```

### service_logs
```sql
CREATE TABLE service_logs (
    id SERIAL PRIMARY KEY,
    level VARCHAR(10) NOT NULL,       -- 'INFO', 'WARNING', 'ERROR', 'DEBUG'
    message VARCHAR(500) NOT NULL,
    details VARCHAR(2000),
    timestamp TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_timestamp ON service_logs(timestamp);
```

---

## ⚙️ Configuration

### Environment Variables
```env
# Database
DATABASE_URL=postgresql://user:pass@host:5432/dbname

# Service Settings
CURRENCY_PORT=5004
CURRENCY_HOST=0.0.0.0
FLASK_ENV=production
LOG_LEVEL=INFO

# External APIs
ABSTRACT_API_KEY=your_api_key_here
NB_BASES=USD,EUR,GBP,SEK,DKK,CAD,ISK,AUD,NZD,IDR,CLP
NB_QUOTE=NOK

# Cache
CURRENCY_CACHE_DIR=/app/cache/rates
CACHE_TTL=3600

# Redis (optional)
REDIS_URL=redis://:password@host:6379
```

---

## 🔄 Cron Job Schedule

### Automatic Synchronization
- **Frequency:** Every hour at minute 0
- **Schedule:** `0 * * * *` (cron format)
- **Sources:** Norges Bank + Abstract API
- **Action:** Upsert all currency rates
- **Logging:** Full sync history and errors

### Initial Sync
- Runs immediately on service startup
- Ensures fresh data is available
- Same behavior as hourly sync

---

## 📊 Monitoring & Observability

### Metrics Tracked
1. **Service Health**
   - Database connectivity
   - Scheduler status
   - External API availability

2. **Sync Performance**
   - Last sync time
   - Next scheduled sync
   - Rates updated count
   - Success/failure status
   - Error messages

3. **Activity Logs**
   - All CRUD operations
   - Sync events
   - Errors and warnings
   - API calls

### Log Levels
- **INFO:** Normal operations (syncs, CRUD operations)
- **WARNING:** Non-critical issues (API timeouts)
- **ERROR:** Failures (sync errors, database errors)
- **DEBUG:** Detailed debugging information

---

## 🧪 Testing Examples

### Create Manual Rate
```bash
curl -X POST http://localhost:5004/api/currency/rates \
  -H "Content-Type: application/json" \
  -d '{"from_currency": "BTC", "to_currency": "USD", "rate": 43000.50, "source": "manual"}'
```

### Get Latest Rates
```bash
curl http://localhost:5004/api/currency/rates?latest=true
```

### Update Rate
```bash
curl -X PUT http://localhost:5004/api/currency/rates/14 \
  -H "Content-Type: application/json" \
  -d '{"rate": 43500.00}'
```

### Convert Currency
```bash
curl "http://localhost:5004/api/currency/convert?from=USD&to=NOK&amount=100"
```

### Manual Sync
```bash
curl -X POST http://localhost:5004/api/currency/sync
```

### Check Status
```bash
curl http://localhost:5004/api/currency/status | jq '.'
```

---

## 🚀 Deployment

### Docker Compose Configuration
```yaml
currency_service:
  build: ./apps/currency_service
  container_name: currency_service
  ports:
    - "5004:5004"
  environment:
    - DATABASE_URL=postgresql://user:pass@postgres:5432/dbname
    - REDIS_URL=redis://:password@redis:6379
  networks:
    - aquatiq-network
  depends_on:
    - postgres
    - redis
  restart: unless-stopped
```

### Build & Run
```bash
# Build
docker-compose build currency_service

# Start
docker-compose up -d currency_service

# Check logs
docker logs currency_service -f

# Test
curl http://localhost:5004/health
```

---

## 📈 Performance Considerations

1. **Database Indexes:** Optimized queries on currency pairs and timestamps
2. **Connection Pooling:** SQLAlchemy pool for efficient database connections
3. **Cache Layer:** Optional Redis caching for frequently accessed rates
4. **Unique Constraints:** Prevents duplicate rates per source
5. **Scheduled Sync:** Hourly updates minimize API calls
6. **Log Cleanup:** Automatic removal of old logs (keeps last 1000)

---

## 🔒 Security

1. **Input Validation:** All inputs validated and sanitized
2. **SQL Injection Protection:** SQLAlchemy ORM prevents injection
3. **CORS Configuration:** Controlled access from frontend origins
4. **Error Handling:** Graceful error messages without exposing internals
5. **Secret Management:** Environment variables for sensitive data

---

## 🎉 Summary

✅ **Complete CRUD** - Full Create, Read, Update, Delete operations  
✅ **Database Persistence** - PostgreSQL with SQLAlchemy ORM  
✅ **Scheduled Jobs** - APScheduler for automatic hourly syncs  
✅ **Multi-Source** - Norges Bank + Abstract API aggregation  
✅ **Monitoring** - Comprehensive status, logs, and sync history  
✅ **Conversion** - Real-time currency conversion with cross-rates  
✅ **Production Ready** - Docker, health checks, error handling  

**Status:** ✅ All features implemented and tested  
**Last Updated:** December 10, 2025
