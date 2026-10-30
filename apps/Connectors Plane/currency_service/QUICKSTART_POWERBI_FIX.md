# Quick Start: Implementing Power BI View Fix (Minimal Code Changes)

## Step 1: Install Test Dependencies (1 minute)

```bash
# Create virtual environment (recommended)
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt
```

## Step 2: Run Tests - Verify They Fail (RED Phase) ✅

```bash
pytest test_bigquery_sync.py -v

# Expected output: Many FAILED tests (this is GOOD - it's the RED phase)
# Example:
# FAILED test_bigquery_sync.py::TestPowerBIViews::test_create_rate_view_function_exists
# FAILED test_bigquery_sync.py::TestPowerBIViews::test_rate_view_has_explicit_schema
```

## Step 3: Add Minimal Code to Pass Tests (GREEN Phase) ✅

Add these functions to `bigquery_sync.py` (after existing functions, before `if __name__`):

```python
# ============================================================================
# Power BI View Support
# ============================================================================

def get_rate_view_sql(project_id: str, dataset_id: str) -> str:
    """
    Generate SQL to create Power BI-safe view for Rate table.
    
    Uses explicit CAST operations to ensure Power BI correctly interprets types.
    """
    return f"""
    CREATE OR REPLACE VIEW `{project_id}.{dataset_id}.Rate_PowerBI` AS
    SELECT
        CAST(currency_code AS STRING) AS currency_code,
        CAST(quote_currency AS STRING) AS quote_currency,
        SAFE_CAST(rate AS FLOAT64) AS rate,
        CAST(date AS DATE) AS date,
        CAST(source AS STRING) AS source,
        CAST(updated_at AS TIMESTAMP) AS updated_at
    FROM `{project_id}.{dataset_id}.Rate`
    """


def get_historyrates_view_sql(project_id: str, dataset_id: str) -> str:
    """
    Generate SQL to create Power BI-safe view for Historyrates table.
    """
    return f"""
    CREATE OR REPLACE VIEW `{project_id}.{dataset_id}.Historyrates_PowerBI` AS
    SELECT
        CAST(currency_code AS STRING) AS currency_code,
        CAST(quote_currency AS STRING) AS quote_currency,
        SAFE_CAST(rate AS FLOAT64) AS rate,
        CAST(date AS DATE) AS date,
        CAST(source AS STRING) AS source,
        CAST(created_at AS TIMESTAMP) AS created_at
    FROM `{project_id}.{dataset_id}.Historyrates`
    WHERE date >= '2023-01-01'  -- Limit to recent history for performance
    """


def create_powerbi_views() -> bool:
    """
    Create Power BI-safe views for both Rate and Historyrates tables.
    
    These views use explicit type casting to ensure Power BI correctly
    interprets column types (especially the rate FLOAT64 column).
    
    Returns:
        True if views were created successfully
    """
    try:
        gcp = get_gcp_client()
        
        # Create Rate_PowerBI view
        rate_sql = get_rate_view_sql(gcp.project_id, gcp.dataset_id)
        gcp.bigquery.query(rate_sql).result()
        logger.info(f"Created view: {gcp.project_id}.{gcp.dataset_id}.Rate_PowerBI")
        
        # Create Historyrates_PowerBI view
        history_sql = get_historyrates_view_sql(gcp.project_id, gcp.dataset_id)
        gcp.bigquery.query(history_sql).result()
        logger.info(f"Created view: {gcp.project_id}.{gcp.dataset_id}.Historyrates_PowerBI")
        
        return True
        
    except Exception as e:
        logger.error(f"Failed to create Power BI views: {e}")
        return False


def query_rate_view(currency_codes: Optional[list[str]] = None, limit: int = 100) -> list[dict]:
    """
    Query the Power BI-safe Rate view.
    
    Args:
        currency_codes: Optional list of currency codes to filter
        limit: Maximum results to return
        
    Returns:
        List of rate dictionaries with correctly typed values
    """
    try:
        gcp = get_gcp_client()
        
        where_clause = ""
        if currency_codes:
            codes_str = "', '".join(currency_codes)
            where_clause = f"WHERE currency_code IN ('{codes_str}')"
        
        query = f"""
            SELECT 
                currency_code,
                quote_currency,
                rate,
                date,
                source,
                updated_at
            FROM `{gcp.project_id}.{gcp.dataset_id}.Rate_PowerBI`
            {where_clause}
            ORDER BY date DESC
            LIMIT {limit}
        """
        
        results = gcp.query_bigquery(query)
        
        rates = []
        for row in results:
            rates.append({
                "currency_code": row.currency_code,
                "quote_currency": row.quote_currency,
                "rate": row.rate,  # Should be float from view
                "date": row.date.isoformat() if hasattr(row.date, 'isoformat') else str(row.date),
                "source": row.source,
                "updated_at": row.updated_at.isoformat() if hasattr(row.updated_at, 'isoformat') else str(row.updated_at)
            })
        
        return rates
        
    except Exception as e:
        logger.error(f"Failed to query Rate_PowerBI view: {e}")
        return []


def verify_dataset_configuration() -> dict:
    """
    Verify that BigQuery dataset has all required tables and views.
    
    Checks:
    - Base tables exist (Rate, Historyrates)
    - Power BI views exist (Rate_PowerBI, Historyrates_PowerBI)
    
    Returns:
        dict with status and details
    """
    try:
        gcp = get_gcp_client()
        client = gcp.bigquery
        
        result = {
            "status": "ok",
            "tables_exist": True,
            "views_exist": True,
            "missing_tables": [],
            "missing_views": []
        }
        
        # Check base tables
        for table_id in ["Rate", "Historyrates"]:
            try:
                client.get_table(f"{gcp.project_id}.{gcp.dataset_id}.{table_id}")
            except Exception:
                result["tables_exist"] = False
                result["missing_tables"].append(table_id)
        
        # Check Power BI views
        for view_id in ["Rate_PowerBI", "Historyrates_PowerBI"]:
            try:
                client.get_table(f"{gcp.project_id}.{gcp.dataset_id}.{view_id}")
            except Exception:
                result["views_exist"] = False
                result["missing_views"].append(view_id)
        
        if not result["tables_exist"] or not result["views_exist"]:
            result["status"] = "missing_objects"
        
        return result
        
    except Exception as e:
        logger.error(f"Failed to verify dataset configuration: {e}")
        return {
            "status": "error",
            "error": str(e)
        }
```

## Step 4: Re-run Tests - Verify They Pass (GREEN Phase) ✅

```bash
pytest test_bigquery_sync.py -v

# Expected: Phase 1-3 tests should PASS
# Some edge case tests may still fail - that's ok for now
```

## Step 5: Create Views in BigQuery (Manual Step)

Run this Python script to create the views:

```python
#!/usr/bin/env python3
"""Create Power BI views in BigQuery."""

from bigquery_sync import create_powerbi_views, verify_dataset_configuration

# Create views
print("Creating Power BI views...")
success = create_powerbi_views()

if success:
    print("✅ Views created successfully")
    
    # Verify configuration
    print("\nVerifying dataset configuration...")
    config = verify_dataset_configuration()
    print(f"Status: {config['status']}")
    print(f"Tables exist: {config['tables_exist']}")
    print(f"Views exist: {config['views_exist']}")
    
    if config['missing_views']:
        print(f"⚠️  Missing views: {config['missing_views']}")
else:
    print("❌ Failed to create views")
```

Save as `create_powerbi_views.py` and run:
```bash
python3 create_powerbi_views.py
```

## Step 6: Update Power BI Connection

In Power BI Desktop:

1. **Get Data** → **More** → **Google BigQuery**
2. Enter connection details:
   - Project: `prj-dw-dev`
   - Dataset: `dw_1_staging_currency`
3. **Select the NEW views**:
   - ✅ `Rate_PowerBI` (instead of `Rate`)
   - ✅ `Historyrates_PowerBI` (instead of `Historyrates`)
4. **Load** and verify:
   - `rate` column shows as **Decimal** (number type)
   - NOT as **Text** (string type)

## Step 7: Test Coverage

```bash
pytest test_bigquery_sync.py --cov=bigquery_sync --cov-report=term-missing

# Target: 80%+ coverage
```

## Troubleshooting

### "Permission denied" when creating views

**Problem**: Service account lacks permissions

**Solution**: Grant role in GCP Console:
```bash
gcloud projects add-iam-policy-binding prj-dw-dev \
    --member="serviceAccount:YOUR_SERVICE_ACCOUNT@prj-dw-dev.iam.gserviceaccount.com" \
    --role="roles/bigquery.dataEditor"
```

### "View already exists" error

**Problem**: Old view exists

**Solution**: The code uses `CREATE OR REPLACE VIEW` - should work automatically.
If not, manually drop:
```sql
DROP VIEW IF EXISTS `prj-dw-dev.dw_1_staging_currency.Rate_PowerBI`;
```

### Tests still show `rate` as string

**Problem**: Mock not properly configured

**Solution**: Ensure mock returns `float` type:
```python
mock_row.rate = 11.23  # float, not "11.23" string
```

### Power BI still shows string after view creation

**Problem**: Power BI cached old schema

**Solution**: In Power BI Desktop:
1. **Home** → **Transform Data**
2. **Data source settings**
3. **Clear Permissions**
4. **Refresh Preview**
5. **Close & Apply**

## Success Criteria ✅

- [ ] Tests pass: `pytest test_bigquery_sync.py -v`
- [ ] Coverage ≥ 80%: `pytest test_bigquery_sync.py --cov=bigquery_sync`
- [ ] Views exist in BigQuery: Verify in console
- [ ] Power BI shows `rate` as Decimal/Number (not Text)
- [ ] Power BI report calculates correctly (e.g., SUM, AVG on rate column)

## Time Estimate

- Step 1 (Install): 5 minutes
- Step 2 (Run tests): 1 minute
- Step 3 (Add code): 15 minutes
- Step 4 (Re-run tests): 1 minute
- Step 5 (Create views): 5 minutes
- Step 6 (Update Power BI): 10 minutes
- Step 7 (Coverage): 5 minutes

**Total**: ~40 minutes for complete implementation

## Next Steps After Implementation

1. **Deploy to Production**: Repeat steps 5-6 for production dataset
2. **Add Monitoring**: Set up alerts for view availability
3. **Update Documentation**: Document new views for Power BI users
4. **Refactor**: Clean up code, add more edge case tests
5. **CI/CD Integration**: Add test run to deployment pipeline
