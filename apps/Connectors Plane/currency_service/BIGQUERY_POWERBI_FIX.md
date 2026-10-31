# BigQuery Power BI Integration Fix - TDD Implementation

## Problem Statement

Power BI preview shows the `rate` column from BigQuery table `prj-dw-dev.dw_1_staging_currency.Rate` as a raw BigQuery REST payload string, despite the column being correctly defined as `FLOAT64` in BigQuery schema.

## Root Cause

Power BI's BigQuery connector occasionally misinterprets schema types, especially with certain BigQuery API responses. This is a known issue with Power BI + BigQuery integration.

## Solution

Create Power BI-safe **SQL views** with:
1. Explicit type casting for all columns (CAST/SAFE_CAST)
2. Clear column descriptions and metadata
3. Consistent naming convention (e.g., `Rate_PowerBI`, `Historyrates_PowerBI`)

---

## TDD Implementation Plan

### Phase 1: Power BI View Creation ⭐ START HERE
**Goal**: Create views that Power BI can reliably consume

**Tests to Write First** (in `test_bigquery_sync.py`):
- ✅ `test_create_rate_view_function_exists()` - Function exists
- ✅ `test_rate_view_has_explicit_schema()` - View creation SQL is called
- ✅ `test_rate_view_sql_casts_all_columns()` - SQL contains explicit CASTs

**Implementation** (in `bigquery_sync.py`):
```python
def get_rate_view_sql(project_id: str, dataset_id: str) -> str:
    """Generate SQL for Rate_PowerBI view."""
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

def create_powerbi_views() -> bool:
    """Create Power BI-safe views for Rate and Historyrates tables."""
    # Implementation to create views
```

**Run Tests** (should FAIL):
```bash
pytest test_bigquery_sync.py::TestPowerBIViews -v
```

**Expected**: All Phase 1 tests fail (RED) ❌

**Implement**: Add functions to make tests pass (GREEN) ✅

---

### Phase 2: Type Safety Verification
**Goal**: Ensure views return correct Python primitive types

**Tests**:
- ✅ `test_rate_column_returns_float_not_string()` - Rate is float, not string
- ✅ `test_null_rate_handling()` - NULL values handled gracefully

**Implementation**:
```python
def query_rate_view(limit: int = 100) -> list[dict]:
    """Query the Power BI view and return typed results."""
    # Query Rate_PowerBI view
    # Verify types in results
```

**Run Tests**: 
```bash
pytest test_bigquery_sync.py::TestTypeSafety -v
```

---

### Phase 3: Dataset Configuration Management
**Goal**: Detect and fix config drift between dev/prod environments

**Tests**:
- ✅ `test_verify_dataset_config_function_exists()` - Function exists
- ✅ `test_detect_missing_views()` - Detects when views are missing

**Implementation**:
```python
def verify_dataset_configuration() -> dict:
    """
    Verify BigQuery dataset configuration is correct.
    
    Checks:
    - Tables exist (Rate, Historyrates)
    - Views exist (Rate_PowerBI, Historyrates_PowerBI)
    - Schema matches expected
    
    Returns:
        dict with status and any issues found
    """
```

---

### Phase 4: Refactor & Coverage
**Goal**: Achieve 80%+ test coverage

**Run Full Test Suite**:
```bash
pytest test_bigquery_sync.py -v --cov=bigquery_sync --cov-report=term-missing
```

**Coverage Target**: 80%+ branches, functions, lines, statements

---

## Setup & Running Tests

### Install Test Dependencies
```bash
# Option 1: Using virtual environment (recommended)
python3 -m venv venv
source venv/bin/activate
pip install pytest pytest-cov

# Option 2: Using Homebrew (macOS)
brew install pytest

# Option 3: System-wide (use with caution)
pip3 install --break-system-packages pytest pytest-cov
```

### Run Tests
```bash
# Run all tests
pytest test_bigquery_sync.py -v

# Run specific test class
pytest test_bigquery_sync.py::TestPowerBIViews -v

# Run with coverage report
pytest test_bigquery_sync.py --cov=bigquery_sync --cov-report=html

# Run single test
pytest test_bigquery_sync.py::TestPowerBIViews::test_create_rate_view_function_exists -v
```

---

## Risks & Edge Cases

### 🔴 CRITICAL Risks

1. **IAM Permissions**
   - **Risk**: Service account may lack `bigquery.tables.create` permission for views
   - **Test**: `test_rate_view_has_explicit_schema()` will fail with permission error
   - **Mitigation**: Grant `BigQuery Data Editor` or `BigQuery Admin` role
   - **Production Impact**: View creation will fail silently if not tested

2. **NULL Rate Values**
   - **Risk**: NULL rates cause Power BI to fail or misinterpret data
   - **Test**: `test_null_rate_handling()`
   - **Mitigation**: Use `SAFE_CAST` and `COALESCE(rate, 0.0)` in view SQL
   - **Edge Case**: Division by zero in Power BI calculations

3. **Dataset Drift (Dev vs Prod)**
   - **Risk**: Production dataset missing views or has wrong schema
   - **Test**: `test_detect_missing_views()`
   - **Mitigation**: Run `verify_dataset_configuration()` in CI/CD
   - **Impact**: Power BI reports fail in production but work in dev

### 🟡 HIGH Risks

4. **View Name Conflicts**
   - **Risk**: View `Rate_PowerBI` may already exist with different definition
   - **Mitigation**: Use `CREATE OR REPLACE VIEW`
   - **Test**: Verify `CREATE OR REPLACE` in `test_rate_view_sql_casts_all_columns()`

5. **Schema Evolution**
   - **Risk**: Adding columns to base table breaks view
   - **Mitigation**: 
     - Add `SELECT * EXCEPT()` pattern for extensibility
     - Version views (`Rate_PowerBI_v1`, `Rate_PowerBI_v2`)
   - **Test**: Add test for `test_view_survives_column_addition()`

6. **Rate Type Coercion**
   - **Risk**: String rates like "11.23" inserted into FLOAT64 column
   - **Test**: `test_sync_handles_invalid_rate_type()`
   - **Mitigation**: Explicit `float()` conversion in sync functions
   - **Edge Cases**: "N/A", "INVALID", empty string

### 🟢 MEDIUM Risks

7. **Large Result Sets**
   - **Risk**: Querying full Historyrates table (3+ years) overwhelms Power BI
   - **Mitigation**: Add date range filters to view definition
   - **Test**: Add `test_view_limits_date_range()`
   - **Edge Case**: 10K+ currency pairs × 1000+ days = millions of rows

8. **BigQuery API Rate Limits**
   - **Risk**: Frequent view creation during testing hits quota limits
   - **Mitigation**: Mock BigQuery client in tests, use sandbox project for integration tests
   - **Test**: Ensure all unit tests use mocks

9. **Timezone Issues**
   - **Risk**: `updated_at` timestamp in different timezone than `date`
   - **Mitigation**: Always use UTC (`TIMESTAMP(updated_at, "UTC")`)
   - **Edge Case**: Rates dated tomorrow in Power BI due to timezone shift

10. **Currency Code Case Sensitivity**
    - **Risk**: "usd" vs "USD" creates duplicates
    - **Mitigation**: `UPPER(currency_code)` in view
    - **Test**: `test_currency_code_case_normalization()`

### 🔵 LOW Risks (Monitor)

11. **View Query Performance**
    - **Risk**: View adds latency vs direct table query
    - **Mitigation**: Views should be simple SELECT with CAST (no joins/aggregations)
    - **Monitor**: Add query execution time logging

12. **Power BI Cache Staleness**
    - **Risk**: Power BI caches old schema even after view fix
    - **Mitigation**: Clear Power BI cache, refresh data source
    - **User Action**: Power BI users must "Refresh" data source

---

## Production Deployment Checklist

Before deploying to production:

- [ ] All tests pass (`pytest test_bigquery_sync.py -v`)
- [ ] Coverage ≥ 80% (`pytest --cov=bigquery_sync`)
- [ ] Views created in dev environment
- [ ] Verify views in dev return correct types (run query in BigQuery console)
- [ ] Power BI connects successfully to views in dev
- [ ] Run `verify_dataset_configuration()` against prod dataset
- [ ] IAM permissions validated (service account can create views)
- [ ] Rate limits checked (BigQuery quota dashboard)
- [ ] Rollback plan documented (drop views, revert to direct table access)
- [ ] Alert Power BI users to refresh data sources after deployment

---

## Monitoring & Alerts

Add monitoring for:
1. **View Existence**: Daily check that `Rate_PowerBI` and `Historyrates_PowerBI` exist
2. **Row Count Match**: `Rate_PowerBI` row count ≈ `Rate` row count (allow ±10%)
3. **NULL Rate**: Alert if >5% of rows have NULL rate
4. **Last Update Time**: Alert if `updated_at` older than 24 hours
5. **Power BI Query Failures**: Monitor BigQuery audit logs for failed queries from Power BI

---

## Rollback Plan

If Power BI integration breaks after deployment:

1. **Immediate**: Direct Power BI to query base tables (`Rate`, `Historyrates`) instead of views
2. **Drop Views** (if corrupted):
   ```sql
   DROP VIEW IF EXISTS `prj-dw-dev.dw_1_staging_currency.Rate_PowerBI`;
   DROP VIEW IF EXISTS `prj-dw-dev.dw_1_staging_currency.Historyrates_PowerBI`;
   ```
3. **Recreate Views**: Run `create_powerbi_views()` function
4. **Verify**: Query views directly in BigQuery console
5. **Alert**: Notify Power BI users to refresh data sources

---

## Next Steps

1. ✅ **Review this document** - Ensure plan makes sense
2. 🔴 **Install pytest** - `pip install pytest pytest-cov` (in venv recommended)
3. 🔴 **Run tests** - `pytest test_bigquery_sync.py -v` (expect failures - RED phase)
4. 🟡 **Implement Phase 1** - Add view creation functions to `bigquery_sync.py`
5. 🟢 **Re-run tests** - Verify Phase 1 tests pass (GREEN phase)
6. 🔵 **Continue Phases 2-4** - Repeat Red-Green-Refactor cycle
7. ✅ **Deploy to production** - Follow deployment checklist

---

## References

- [BigQuery Views Documentation](https://cloud.google.com/bigquery/docs/views)
- [Power BI BigQuery Connector](https://learn.microsoft.com/en-us/power-bi/connect-data/desktop-connect-bigquery)
- [BigQuery IAM Roles](https://cloud.google.com/bigquery/docs/access-control)
- Currency Service BigQuery Sync: `bigquery_sync.py`
- Test Suite: `test_bigquery_sync.py`
