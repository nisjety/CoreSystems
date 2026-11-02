# SuperOffice → BigQuery Quick Start

## 🚀 Fast Setup (5 minutes)

### 1. Verify Configuration

```bash
# Check .env has required GCP settings
grep GCP_ .env

# Should show:
# GCP_PROJECT_ID=prj-dw-dev
# GCP_REGION=europe-north1
# GOOGLE_APPLICATION_CREDENTIALS=/app/gcp-credentials.json
# GCP_DATASET_ID=dw_1_staging_socrm
# GCP_BUCKET_NAME=aquatiq-dw-dev-storage
```

### 2. Start Service

```bash
docker-compose up -d socrm-service
docker-compose logs -f socrm-service
```

### 3. Authenticate with SuperOffice

```bash
# Check auth status
curl http://localhost:5005/api/auth/microsoft/status

# If not authenticated, authenticate via frontend:
# Open: http://localhost:3000/erp-connector/superoffice
```

### 4. Sync Data to BigQuery

```bash
# Sync all entities (companies, contacts, sales, projects)
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts", "sales", "projects"]}'

# Expected response:
# {
#   "success": true,
#   "tables_synced": 4,
#   "total_rows": 1234,
#   "results": { ... }
# }
```

### 5. Query BigQuery Data

```bash
# Check tables
curl http://localhost:5005/api/superoffice/bigquery/status

# Get company data
curl "http://localhost:5005/api/superoffice/bigquery/data?table=Company&limit=10"
```

## 📊 BigQuery Tables

| Table | Description | Sync Endpoint |
|-------|-------------|--------------|
| `Company` | Companies/Organizations | `/api/v1/Contact` |
| `Contact` | Persons/Contacts | `/api/v1/Person` |
| `Sale` | Sales Opportunities | `/api/v1/Sale` |
| `Project` | Projects | `/api/v1/Project` |

All tables in: `prj-dw-dev.dw_1_staging_socrm`

## 🔄 Common Operations

### Sync Only Companies

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies"]}'
```

### Sync Companies + Contacts

```bash
curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts"]}'
```

### Check Sync Status

```bash
curl http://localhost:5005/api/superoffice/bigquery/status | jq
```

## 📈 BigQuery SQL Examples

### Query Companies

```sql
SELECT 
    company_id,
    name,
    country,
    email,
    is_active
FROM `prj-dw-dev.dw_1_staging_socrm.Company`
WHERE is_active = TRUE
ORDER BY name
LIMIT 10;
```

### Companies with Contact Count

```sql
SELECT 
    c.name,
    c.country,
    COUNT(p.contact_id) as contact_count
FROM `prj-dw-dev.dw_1_staging_socrm.Company` c
LEFT JOIN `prj-dw-dev.dw_1_staging_socrm.Contact` p
    ON c.company_id = p.company_id
GROUP BY c.name, c.country
ORDER BY contact_count DESC
LIMIT 20;
```

### Sales Pipeline by Company

```sql
SELECT 
    c.name as company_name,
    COUNT(s.sale_id) as total_sales,
    SUM(s.amount_nok) as total_amount_nok,
    AVG(s.probability) as avg_probability
FROM `prj-dw-dev.dw_1_staging_socrm.Sale` s
JOIN `prj-dw-dev.dw_1_staging_socrm.Company` c
    ON s.company_id = c.company_id
WHERE s.status = 'Open'
GROUP BY c.name
ORDER BY total_amount_nok DESC
LIMIT 10;
```

## 🐛 Troubleshooting

### Not Authenticated

```bash
# Error: "Not authenticated"
# Solution: Authenticate via frontend or check tokens
curl http://localhost:5005/api/auth/microsoft/status
```

### GCP Not Configured

```bash
# Error: "GCP not configured"
# Solution: Check .env file
cat .env | grep GCP_
```

### Tables Don't Exist

Tables are created automatically on first sync. If issues:

```python
# Run in Python:
from apps.socrm_service.sync.bigquery_sync import ensure_tables_exist
ensure_tables_exist()
```

### Check Logs

```bash
docker-compose logs socrm-service | grep -i bigquery
```

## 📁 Key Files

- **Sync Module**: `apps/socrm_service/sync/bigquery_sync.py`
- **API Routes**: `apps/socrm_service/app.py` (lines 869-1000)
- **Full Docs**: `apps/socrm_service/BIGQUERY_INTEGRATION.md`
- **Implementation**: `apps/socrm_service/BIGQUERY_SETUP_COMPLETE.md`

## 🔗 Related Services

- **Visma**: Similar pattern at `apps/visma_service/bigquery_sync.py`
- **Xero**: Similar pattern at `apps/xero_service/bigquery_sync.py`
- **Shared GCP**: `shared/gcp_client.py`

## ⏰ Schedule Automatic Syncs

### Hourly Sync (Companies + Contacts)

```bash
# Add to crontab
0 * * * * curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts"]}'
```

### Daily Sync (All Entities)

```bash
# Add to crontab
0 1 * * * curl -X POST http://localhost:5005/api/superoffice/sync/gcp \
  -H "Content-Type: application/json" \
  -d '{"tables": ["companies", "contacts", "sales", "projects"]}'
```

## ✅ Verification Checklist

- [ ] `.env` has all GCP_* variables
- [ ] Service is running: `docker-compose ps socrm-service`
- [ ] Authenticated with SuperOffice
- [ ] Can sync data: `POST /api/superoffice/sync/gcp`
- [ ] Tables exist in BigQuery
- [ ] Can query data from BigQuery
- [ ] No errors in logs

## 📞 Need Help?

1. Check logs: `docker-compose logs -f socrm-service`
2. Review full documentation: [BIGQUERY_INTEGRATION.md](./BIGQUERY_INTEGRATION.md)
3. Test GCP connection: `python scripts/test_gcp_connection.py`
4. Contact development team

---

**Status**: ✅ Production Ready

**Last Updated**: 2026-01-05

**Version**: 1.0
