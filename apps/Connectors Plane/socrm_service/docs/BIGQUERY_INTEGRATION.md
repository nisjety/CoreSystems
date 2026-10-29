# SuperOffice BigQuery Integration

Complete guide for syncing SuperOffice CRM data to Google BigQuery for data warehousing and analytics.

## Overview

The SuperOffice BigQuery integration automatically syncs your CRM data to BigQuery tables, enabling:
- **Data warehousing**: Centralized storage of all CRM data
- **Analytics & reporting**: SQL-based analysis across all entities
- **Data integration**: Combine SuperOffice data with other sources
- **Historical tracking**: Maintain data history with partitioned tables

## Architecture

```
SuperOffice CRM (REST API)
         ↓
   SOCRM Service (Python/Flask)
         ↓
  BigQuery Sync Module
         ↓
Google BigQuery (dw_1_staging_socrm dataset)
```

## Supported Entities

| Entity | BigQuery Table | SuperOffice Endpoint | Description |
|--------|---------------|---------------------|-------------|
| Companies | `Company` | `/api/v1/Contact` | Organizations/companies |
| Contacts | `Contact` | `/api/v1/Person` | Individual persons |
| Sales | `Sale` | `/api/v1/Sale` | Sales opportunities |
| Projects | `Project` | `/api/v1/Project` | Projects |

## BigQuery Table Schemas

### Company Table

Stores organization/company data from SuperOffice Contact entities.

```sql
CREATE TABLE `prj-dw-dev.dw_1_staging_socrm.Company` (
  company_id STRING NOT NULL,           -- SuperOffice ContactId
  name STRING NOT NULL,                 -- Company name
  organization_number STRING,           -- Org number (OrgNr)
  country STRING,                       -- Country
  city STRING,                          -- City
  postal_code STRING,                   -- Postal/ZIP code
  category STRING,                      -- Company category
  business STRING,                      -- Business type
  email STRING,                         -- Primary email
  phone STRING,                         -- Primary phone
  website STRING,                       -- Website URL
  is_active BOOLEAN,                    -- Active status
  created_date TIMESTAMP,               -- Created timestamp
  updated_date TIMESTAMP,               -- Last updated
  synced_at TIMESTAMP NOT NULL,         -- Sync timestamp
  tenant_id STRING NOT NULL,            -- Organization tenant ID
  _metadata JSON                        -- Additional metadata
)
PARTITION BY DATE(synced_at);
```

### Contact Table

Stores person/contact data from SuperOffice Person entities.

```sql
CREATE TABLE `prj-dw-dev.dw_1_staging_socrm.Contact` (
  contact_id STRING NOT NULL,           -- SuperOffice PersonId
  first_name STRING,                    -- First name
  middle_name STRING,                   -- Middle name
  last_name STRING,                     -- Last name
  email STRING,                         -- Primary email
  phone STRING,                         -- Primary phone
  mobile STRING,                        -- Mobile phone
  direct_phone STRING,                  -- Direct phone
  company_id STRING,                    -- Related company ID
  company_name STRING,                  -- Related company name
  position STRING,                      -- Job position/title
  department STRING,                    -- Department
  is_active BOOLEAN,                    -- Active status
  created_date TIMESTAMP,               -- Created timestamp
  updated_date TIMESTAMP,               -- Last updated
  synced_at TIMESTAMP NOT NULL,         -- Sync timestamp
  tenant_id STRING NOT NULL,            -- Organization tenant ID
  _metadata JSON                        -- Additional metadata
)
PARTITION BY DATE(synced_at);
```

### Sale Table

Stores sales opportunity data from SuperOffice Sale entities.

```sql
CREATE TABLE `prj-dw-dev.dw_1_staging_socrm.Sale` (
  sale_id STRING NOT NULL,              -- SuperOffice SaleId
  title STRING NOT NULL,                -- Sale title/heading
  amount FLOAT64,                       -- Original amount
  currency STRING,                      -- Currency code
  amount_nok FLOAT64,                   -- Converted to NOK
  status STRING,                        -- Sale status
  stage STRING,                         -- Sale stage
  probability INTEGER,                  -- Win probability %
  company_id STRING,                    -- Related company ID
  company_name STRING,                  -- Related company name
  contact_id STRING,                    -- Related contact ID
  contact_name STRING,                  -- Related contact name
  owner_id STRING,                      -- Owner associate ID
  owner STRING,                         -- Owner name
  sale_date DATE,                       -- Sale date
  close_date DATE,                      -- Expected close date
  created_date TIMESTAMP,               -- Created timestamp
  updated_date TIMESTAMP,               -- Last updated
  synced_at TIMESTAMP NOT NULL,         -- Sync timestamp
  tenant_id STRING NOT NULL,            -- Organization tenant ID
  _metadata JSON                        -- Additional metadata
)
PARTITION BY DATE(synced_at);
```

### Project Table

Stores project data from SuperOffice Project entities.

```sql
CREATE TABLE `prj-dw-dev.dw_1_staging_socrm.Project` (
  project_id STRING NOT NULL,           -- SuperOffice ProjectId
  name STRING NOT NULL,                 -- Project name
  description STRING,                   -- Project description
  type STRING,                          -- Project type
  status STRING,                        -- Project status
  company_id STRING,                    -- Related company ID
  company_name STRING,                  -- Related company name
  start_date DATE,                      -- Project start date
  end_date DATE,                        -- Project end date
  is_active BOOLEAN,                    -- Active status
  created_date TIMESTAMP,               -- Created timestamp
  updated_date TIMESTAMP,               -- Last updated
  synced_at TIMESTAMP NOT NULL,         -- Sync timestamp
  tenant_id STRING NOT NULL,            -- Organization tenant ID
  _metadata JSON                        -- Additional metadata
)
PARTITION BY DATE(synced_at);
```

## Configuration

### Environment Variables

Required configuration in `.env`:

```bash
# GCP Configuration
GCP_PROJECT_ID=prj-dw-dev
GCP_REGION=europe-north1
GOOGLE_APPLICATION_CREDENTIALS=/app/gcp-credentials.json
GCP_DATASET_ID=dw_1_staging_socrm
GCP_BUCKET_NAME=aquatiq-dw-dev-storage
```

### GCP Service Account

Your service account needs these permissions:
- `bigquery.datasets.get`
- `bigquery.tables.create`
- `bigquery.tables.get`
- `bigquery.tables.updateData`
- `bigquery.jobs.create`

## API Usage

### Sync All Data

Sync all entities to BigQuery:

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{
    "tables": ["companies", "contacts", "sales", "projects"],
    "tenant_id": "default"
  }'
```

Response:
```json
{
  "success": true,
  "tables_synced": 4,
  "total_rows": 1234,
  "results": {
    "companies": {
      "success": true,
      "table": "Company",
      "rows_synced": 450,
      "synced_at": "2026-01-05T12:00:00"
    },
    "contacts": {
      "success": true,
      "table": "Contact",
      "rows_synced": 678,
      "synced_at": "2026-01-05T12:00:01"
    },
    "sales": {
      "success": true,
      "table": "Sale",
      "rows_synced": 89,
      "synced_at": "2026-01-05T12:00:02"
    },
    "projects": {
      "success": true,
      "table": "Project",
      "rows_synced": 17,
      "synced_at": "2026-01-05T12:00:03"
    }
  },
  "timestamp": "2026-01-05T12:00:03"
}
```

### Sync Specific Entities

Sync only companies and contacts:

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{
    "tables": ["companies", "contacts"],
    "tenant_id": "default"
  }'
```

### Check BigQuery Status

Get GCP/BigQuery integration status:

```bash
curl http://localhost:5005/api/superoffice/bigquery/status
```

Response:
```json
{
  "configured": true,
  "project_id": "prj-dw-dev",
  "dataset_id": "dw_1_staging_socrm",
  "region": "europe-north1",
  "tables": {
    "Company": {
      "exists": true,
      "row_count": 450,
      "last_synced": "2026-01-05T12:00:00"
    },
    "Contact": {
      "exists": true,
      "row_count": 678,
      "last_synced": "2026-01-05T12:00:01"
    },
    "Sale": {
      "exists": true,
      "row_count": 89,
      "last_synced": "2026-01-05T12:00:02"
    },
    "Project": {
      "exists": true,
      "row_count": 17,
      "last_synced": "2026-01-05T12:00:03"
    }
  }
}
```

### Query BigQuery Data

Fetch data from BigQuery:

```bash
curl "http://localhost:5005/api/superoffice/bigquery/data?table=Company&limit=10"
```

## Python Code Examples

### Programmatic Sync

```python
from apps.socrm_service.sync.bigquery_sync import (
    sync_all_to_bigquery,
    ensure_tables_exist
)

# Ensure tables exist
if not ensure_tables_exist():
    print("Failed to create tables")
    exit(1)

# Prepare data
sync_data = {
    "companies": [
        {
            "ContactId": 123,
            "Name": "Aquatiq AS",
            "OrgNr": "123456789",
            "Country": {"Value": "Norway"},
            "City": "Oslo",
            "Emails": [{"Value": "contact@aquatiq.com"}],
            "Active": True
        }
    ],
    "contacts": [
        {
            "PersonId": 456,
            "Firstname": "John",
            "Lastname": "Doe",
            "Emails": [{"Value": "john@aquatiq.com"}],
            "Contact": {"ContactId": 123, "Name": "Aquatiq AS"}
        }
    ]
}

# Sync to BigQuery
result = sync_all_to_bigquery(sync_data, tenant_id="default")
print(f"Synced {result['total_rows']} rows to {result['tables_synced']} tables")
```

### Query BigQuery with Python

```python
from shared.gcp_client import get_gcp_client

gcp = get_gcp_client()

# Query companies
query = """
    SELECT 
        company_id,
        name,
        country,
        email,
        synced_at
    FROM `prj-dw-dev.dw_1_staging_socrm.Company`
    WHERE is_active = TRUE
    ORDER BY synced_at DESC
    LIMIT 10
"""

results = gcp.query_bigquery(query)
for row in results:
    print(f"{row.name} ({row.country})")
```

### Complex Analytics Query

```python
# Get sales pipeline by company
query = """
    SELECT 
        c.name as company_name,
        c.country,
        COUNT(s.sale_id) as total_sales,
        SUM(s.amount_nok) as total_amount_nok,
        AVG(s.probability) as avg_probability
    FROM `prj-dw-dev.dw_1_staging_socrm.Sale` s
    JOIN `prj-dw-dev.dw_1_staging_socrm.Company` c
        ON s.company_id = c.company_id
    WHERE s.status = 'Open'
        AND DATE(s.synced_at) = CURRENT_DATE()
    GROUP BY c.name, c.country
    ORDER BY total_amount_nok DESC
    LIMIT 20
"""

results = gcp.query_bigquery(query)
for row in results:
    print(f"{row.company_name}: {row.total_sales} sales, {row.total_amount_nok:,.0f} NOK")
```

## Data Transformation

The sync module handles SuperOffice's unique data structure:

### Nested Object Extraction

SuperOffice uses nested objects for list values:

```python
# Input from SuperOffice
{
  "Country": {"Value": "Norway"},
  "Category": {"Value": "Customer"}
}

# Transformed to BigQuery
{
  "country": "Norway",
  "category": "Customer"
}
```

### Email/Phone Arrays

```python
# Input
{
  "Emails": [
    {"Value": "primary@example.com"},
    {"Value": "secondary@example.com"}
  ]
}

# Transformed (first email only)
{
  "email": "primary@example.com"
}
```

### Relationship Handling

```python
# Contact with company relation
{
  "PersonId": 456,
  "Contact": {
    "ContactId": 123,
    "Name": "Aquatiq AS"
  }
}

# Transformed
{
  "contact_id": "456",
  "company_id": "123",
  "company_name": "Aquatiq AS"
}
```

## Monitoring & Troubleshooting

### Check Sync Status

```bash
# View recent syncs
curl http://localhost:5005/api/superoffice/bigquery/status
```

### View Logs

```bash
# Docker logs
docker-compose logs socrm-service | grep -i bigquery

# Or follow logs
docker-compose logs -f socrm-service
```

### Common Issues

#### 1. Authentication Errors

```
Error: Not authenticated
```

**Solution**: Ensure you're authenticated with SuperOffice:
```bash
curl http://localhost:5005/api/auth/microsoft/status
```

#### 2. GCP Configuration Missing

```
Error: GCP not configured
```

**Solution**: Check `.env` file has all required GCP variables:
```bash
echo $GCP_PROJECT_ID
echo $GCP_DATASET_ID
```

#### 3. Permission Denied

```
Error: 403 User does not have bigquery.tables.create permission
```

**Solution**: Update GCP service account permissions or use a different service account.

#### 4. Table Already Exists

This is normal - tables are created once and reused. The sync will continue normally.

#### 5. Data Fetch Errors

```json
{
  "success": true,
  "partial_success": true,
  "fetch_errors": ["Failed to fetch projects: 404 Not Found"]
}
```

**Solution**: Some entities might not be available. This is normal if you haven't configured all entity types in SuperOffice.

## Best Practices

### 1. Regular Syncs

Schedule regular syncs (e.g., hourly or daily) via cron or orchestrator:

```bash
# Example: Sync every hour
0 * * * * curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts", "sales"]}'
```

### 2. Incremental Sync

For large datasets, consider implementing incremental sync based on `UpdatedDate`:

```python
# Fetch only updated records
last_sync = "2026-01-04T00:00:00"
companies = client.get(f"/api/v1/Contact?$filter=UpdatedDate gt '{last_sync}'")
```

### 3. Partition Cleanup

BigQuery partitions by `synced_at`. Clean up old partitions to save costs:

```sql
-- Delete partitions older than 90 days
DELETE FROM `prj-dw-dev.dw_1_staging_socrm.Company`
WHERE DATE(synced_at) < DATE_SUB(CURRENT_DATE(), INTERVAL 90 DAY);
```

### 4. Monitor Costs

BigQuery charges for:
- Storage (partitioned tables are cheaper)
- Query processing (use `LIMIT` and partitioning)

Check costs in GCP Console → BigQuery → Queries

### 5. Data Quality

Validate synced data:

```sql
-- Check for duplicates
SELECT company_id, COUNT(*) as count
FROM `prj-dw-dev.dw_1_staging_socrm.Company`
WHERE DATE(synced_at) = CURRENT_DATE()
GROUP BY company_id
HAVING COUNT(*) > 1;
```

## Integration with Other Services

### Visma & Xero Data

Combine SuperOffice data with Visma/Xero for complete business insights:

```sql
-- Companies in both SuperOffice and Visma
SELECT 
    so.name as superoffice_name,
    so.email as superoffice_email,
    v.name as visma_name,
    v.email as visma_email
FROM `prj-dw-dev.dw_1_staging_socrm.Company` so
LEFT JOIN `prj-dw-dev.dw_1_staging_visma.Customer` v
    ON LOWER(so.email) = LOWER(v.email)
WHERE DATE(so.synced_at) = CURRENT_DATE()
    AND v.email IS NOT NULL;
```

### Data Warehouse Patterns

Use SuperOffice as the master CRM data source:

```sql
-- Create unified customer view
CREATE OR REPLACE VIEW `prj-dw-dev.analytics.unified_customers` AS
SELECT 
    COALESCE(so.company_id, v.customer_id) as unified_id,
    COALESCE(so.name, v.name) as name,
    COALESCE(so.email, v.email) as email,
    so.country as crm_country,
    v.currency_code as accounting_currency,
    so.synced_at as crm_updated,
    v.synced_at as accounting_updated
FROM `prj-dw-dev.dw_1_staging_socrm.Company` so
FULL OUTER JOIN `prj-dw-dev.dw_1_staging_visma.Customer` v
    ON LOWER(so.email) = LOWER(v.email);
```

## Migration from Old System

If migrating from the old class-based sync:

### Old Code (Deprecated)

```python
# DON'T USE THIS
from apps.socrm_service.sync.bigquery_sync import SuperOfficeBigQuerySync

bq_sync = SuperOfficeBigQuerySync(project_id, dataset_id)
result = bq_sync.sync_companies(companies, tenant_id)
```

### New Code (Recommended)

```python
# USE THIS INSTEAD
from apps.socrm_service.sync.bigquery_sync import (
    sync_companies_to_bigquery,
    ensure_tables_exist
)

ensure_tables_exist()
result = sync_companies_to_bigquery(companies, tenant_id)
```

## Support

For issues or questions:
1. Check logs: `docker-compose logs socrm-service`
2. Verify configuration: `.env` file
3. Test BigQuery access: `apps/socrm_service/scripts/test_gcp_connection.py`
4. Review this documentation
5. Contact the development team

## Related Documentation

- [SuperOffice API Integration](./SUPEROFFICE_API_INTEGRATION.md)
- [GCP Integration Guide](../../docs/GCP_INTEGRATION.md)
- [Shared GCP Client](../../shared/gcp_client.py)
- [BigQuery Sync Module](./sync/bigquery_sync.py)
