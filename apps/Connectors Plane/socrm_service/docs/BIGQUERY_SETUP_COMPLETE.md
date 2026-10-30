# SuperOffice BigQuery Integration - Implementation Summary

## ✅ Completed Tasks

### 1. BigQuery Sync Module (`apps/socrm_service/sync/bigquery_sync.py`)

**Refactored from class-based to function-based architecture** (following Visma/Xero pattern):

- ✅ Uses shared `gcp_client` for BigQuery operations
- ✅ Function-based API: `sync_all_to_bigquery()`, `ensure_tables_exist()`
- ✅ Proper data transformation for SuperOffice's nested object format
- ✅ Helper functions for extracting emails, phones, relations
- ✅ Comprehensive error handling and logging

### 2. BigQuery Table Schemas

**Four entity types with proper partitioning:**

| Table | Partition Field | Description |
|-------|----------------|-------------|
| `Company` | `synced_at` | Companies (SuperOffice Contact entities) |
| `Contact` | `synced_at` | Persons (SuperOffice Person entities) |
| `Sale` | `synced_at` | Sales opportunities |
| `Project` | `synced_at` | Projects |

**Schema highlights:**
- All tables include `tenant_id` for multi-tenant support
- Partitioned by `synced_at` for performance
- Include both original and transformed fields
- `_metadata` JSON field for extensibility

### 3. API Endpoint Updates (`apps/socrm_service/app.py`)

**`POST /api/superoffice/sync/gcp`** - Enhanced with:
- ✅ Real data fetching from SuperOffice API (not sample data)
- ✅ Support for all four entity types
- ✅ Proper error handling with partial success support
- ✅ Authentication check before sync
- ✅ GCP configuration validation

**Existing endpoints:**
- ✅ `GET /api/superoffice/bigquery/status` - Check BigQuery status
- ✅ `GET /api/superoffice/bigquery/data` - Query BigQuery data

### 4. Data Transformation

**Handles SuperOffice's unique data structure:**

- **Nested objects**: `{"Value": "Norway"}` → `"Norway"`
- **Email/phone arrays**: Takes first item from arrays
- **Relationships**: Extracts company/contact relations with IDs and names
- **Active status**: Handles various boolean/int formats
- **Date parsing**: Converts to ISO format for BigQuery

### 5. Documentation

**Created comprehensive guide:** `BIGQUERY_INTEGRATION.md`

Includes:
- Complete table schemas (SQL DDL)
- API usage examples
- Python code examples
- Query patterns for analytics
- Troubleshooting guide
- Best practices
- Migration guide from old system

## 🏗️ Architecture

```
SuperOffice CRM (REST API)
         ↓
  GET /api/v1/Contact (Companies)
  GET /api/v1/Person (Contacts)
  GET /api/v1/Sale (Sales)
  GET /api/v1/Project (Projects)
         ↓
   SOCRM Service (Flask)
   POST /api/superoffice/sync/gcp
         ↓
  BigQuery Sync Module
  - ensure_tables_exist()
  - sync_all_to_bigquery()
         ↓
   Shared GCP Client
         ↓
Google BigQuery
  prj-dw-dev.dw_1_staging_socrm
  - Company (partitioned)
  - Contact (partitioned)
  - Sale (partitioned)
  - Project (partitioned)
```

## 📝 Configuration

**Required environment variables** (already in `.env`):

```bash
GCP_PROJECT_ID=prj-dw-dev
GCP_REGION=europe-north1
GOOGLE_APPLICATION_CREDENTIALS=/app/gcp-credentials.json
GCP_DATASET_ID=dw_1_staging_socrm
GCP_BUCKET_NAME=aquatiq-dw-dev-storage
```

## 🚀 Usage

### Sync All Data

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{
    "tables": ["companies", "contacts", "sales", "projects"],
    "tenant_id": "default"
  }'
```

### Sync Specific Entities

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts"]}'
```

### Check Status

```bash
curl http://localhost:5005/api/superoffice/bigquery/status
```

### Query Data

```bash
curl "http://localhost:5005/api/superoffice/bigquery/data?table=Company&limit=10"
```

## 🔄 Similar to Visma/Xero Integration

**Follows the same patterns:**

1. **Shared GCP Client**: Uses `shared.gcp_client.get_gcp_client()`
2. **Function-based API**: No class instantiation needed
3. **Table Management**: `ensure_tables_exist()` creates tables if needed
4. **Partitioned Tables**: All tables partitioned by sync timestamp
5. **Error Handling**: Comprehensive with partial success support

**Key differences:**
- SuperOffice data structure (nested objects, PascalCase)
- Entity naming (Contact=Company, Person=Contact)
- Additional helper functions for data extraction

## 📊 Data Examples

### Company Record

```python
# SuperOffice format
{
  "ContactId": 123,
  "Name": "Aquatiq AS",
  "OrgNr": "123456789",
  "Country": {"Value": "Norway"},
  "Emails": [{"Value": "contact@aquatiq.com"}]
}

# BigQuery format
{
  "company_id": "123",
  "name": "Aquatiq AS",
  "organization_number": "123456789",
  "country": "Norway",
  "email": "contact@aquatiq.com",
  "synced_at": "2026-01-05T12:00:00",
  "tenant_id": "default"
}
```

### Contact Record

```python
# SuperOffice format
{
  "PersonId": 456,
  "Firstname": "John",
  "Lastname": "Doe",
  "Contact": {
    "ContactId": 123,
    "Name": "Aquatiq AS"
  }
}

# BigQuery format
{
  "contact_id": "456",
  "first_name": "John",
  "last_name": "Doe",
  "company_id": "123",
  "company_name": "Aquatiq AS",
  "synced_at": "2026-01-05T12:00:00",
  "tenant_id": "default"
}
```

## 🧪 Testing

### Manual Test

```python
# In Python shell or script
from apps.socrm_service.sync.bigquery_sync import ensure_tables_exist

# Test table creation
if ensure_tables_exist():
    print("✅ All tables created successfully")
else:
    print("❌ Failed to create tables")
```

### Integration Test

```bash
# 1. Ensure service is running
docker-compose up -d socrm-service

# 2. Check authentication
curl http://localhost:5005/api/auth/microsoft/status

# 3. Sync data
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies"]}'

# 4. Verify in BigQuery
# Go to GCP Console → BigQuery → dw_1_staging_socrm
```

## 📈 Analytics Queries

### Top Companies by Sales

```sql
SELECT 
    c.name as company_name,
    COUNT(s.sale_id) as total_sales,
    SUM(s.amount_nok) as total_amount_nok
FROM `prj-dw-dev.dw_1_staging_socrm.Sale` s
JOIN `prj-dw-dev.dw_1_staging_socrm.Company` c
    ON s.company_id = c.company_id
WHERE s.status = 'Open'
GROUP BY c.name
ORDER BY total_amount_nok DESC
LIMIT 10;
```

### Contact Distribution by Company

```sql
SELECT 
    c.name as company_name,
    COUNT(p.contact_id) as contact_count
FROM `prj-dw-dev.dw_1_staging_socrm.Company` c
LEFT JOIN `prj-dw-dev.dw_1_staging_socrm.Contact` p
    ON c.company_id = p.company_id
GROUP BY c.name
ORDER BY contact_count DESC
LIMIT 20;
```

## ⚙️ Implementation Details

### Key Files Modified/Created

1. **`apps/socrm_service/sync/bigquery_sync.py`**
   - Refactored from class-based to function-based
   - Added helper functions for data extraction
   - Enhanced error handling

2. **`apps/socrm_service/app.py`**
   - Updated `/api/superoffice/sync/gcp` endpoint
   - Added real SuperOffice API data fetching
   - Enhanced error reporting

3. **`apps/socrm_service/BIGQUERY_INTEGRATION.md`**
   - Comprehensive documentation
   - Usage examples
   - Troubleshooting guide

4. **`apps/socrm_service/BIGQUERY_SETUP_COMPLETE.md`** (this file)
   - Implementation summary
   - Quick reference

### Helper Functions

```python
# Data extraction helpers
_extract_value()              # Extract from {"Value": "..."}
_extract_first_email()        # Get first email from array
_extract_first_phone()        # Get first phone from array
_extract_company_relation()   # Extract company ID and name
_extract_person_relation()    # Extract person ID and name
_parse_active_status()        # Parse boolean/int active status
_parse_timestamp()            # Convert to ISO format
_parse_date()                 # Convert to date format
```

## 🎯 Next Steps

### Optional Enhancements

1. **Scheduled Sync**: Set up cron job or Cloud Scheduler for automatic syncs
2. **Incremental Sync**: Implement delta sync using `UpdatedDate` filter
3. **More Entities**: Add Appointments, Documents, Activities
4. **Data Validation**: Add schema validation before sync
5. **Monitoring**: Set up alerts for sync failures
6. **Cost Optimization**: Implement partition lifecycle management

### Recommended Schedule

```bash
# Hourly sync for companies and contacts
0 * * * * curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -d '{"tables": ["companies", "contacts"]}'

# Daily sync for sales and projects
0 1 * * * curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -d '{"tables": ["sales", "projects"]}'
```

## 📚 Related Documentation

- [SuperOffice API Integration](./SUPEROFFICE_API_INTEGRATION.md) - Complete API guide
- [BigQuery Integration Guide](./BIGQUERY_INTEGRATION.md) - Detailed BigQuery docs
- [GCP Setup](../../docs/GCP_INTEGRATION.md) - General GCP configuration
- [Shared GCP Client](../../shared/gcp_client.py) - Shared utilities

## ✨ Summary

The SuperOffice BigQuery integration is now **production-ready** with:

- ✅ Four entity types syncing to BigQuery
- ✅ Proper data transformation for SuperOffice format
- ✅ Partitioned tables for performance
- ✅ Comprehensive error handling
- ✅ Multi-tenant support
- ✅ Following Visma/Xero patterns
- ✅ Full documentation

**Ready to sync SuperOffice CRM data to BigQuery!** 🚀
