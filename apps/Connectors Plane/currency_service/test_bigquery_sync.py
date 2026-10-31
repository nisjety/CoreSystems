#!/usr/bin/env python3
"""
TDD tests for BigQuery sync and Power BI view integration.

Tests follow Red-Green-Refactor cycle:
1. Write failing test (RED)
2. Implement minimal code to pass (GREEN)  
3. Refactor and improve (IMPROVE)
"""

import os
import pytest
from datetime import date, datetime
from unittest.mock import Mock, patch, MagicMock
from google.cloud import bigquery

# Module under test
import bigquery_sync
from bigquery_sync import (
    ensure_tables_exist,
    sync_current_rates_to_bigquery,
    sync_historical_rates_to_bigquery,
    CURRENT_RATES_SCHEMA,
    HISTORICAL_RATES_SCHEMA,
)


# ============================================================================
# PHASE 1: Power BI View Creation Tests (Write these FIRST - should FAIL)
# ============================================================================

class TestPowerBIViews:
    """Test view creation for Power BI compatibility."""
    
    def test_create_rate_view_function_exists(self):
        """
        RED: This test should FAIL initially.
        Function doesn't exist yet.
        """
        from bigquery_sync import create_powerbi_views
        assert callable(create_powerbi_views)
    
    def test_rate_view_has_explicit_schema(self):
        """
        RED: Verify view defines explicit column types.
        Should FAIL - view doesn't exist yet.
        """
        # This will fail until we implement the view
        with patch('bigquery_sync.get_gcp_client') as mock_gcp:
            mock_client = Mock()
            mock_gcp.return_value.bigquery = mock_client
            mock_gcp.return_value.project_id = "test-project"
            mock_gcp.return_value.dataset_id = "test_dataset"
            
            from bigquery_sync import create_powerbi_views
            result = create_powerbi_views()
            
            # Should create views for both Rate and Historyrates
            assert result is True
            assert mock_client.query.called
    
    def test_rate_view_sql_casts_all_columns(self):
        """
        RED: Verify the view SQL explicitly casts every column.
        Should FAIL - need to implement view SQL generation.
        """
        from bigquery_sync import get_rate_view_sql
        
        sql = get_rate_view_sql("project", "dataset")
        
        # Must have explicit CAST for rate column (the problematic one)
        assert "CAST(rate AS FLOAT64)" in sql or "SAFE_CAST(rate AS FLOAT64)" in sql
        
        # Must have explicit casts for all columns
        assert "CAST(currency_code AS STRING)" in sql
        assert "CAST(date AS DATE)" in sql


# ============================================================================
# PHASE 2: Type Safety Tests (Write these SECOND - should FAIL)
# ============================================================================

class TestTypeSafety:
    """Test that views return correct primitive Python types."""
    
    def test_rate_column_returns_float_not_string(self):
        """
        RED: Verify rate column returns Python float, not string.
        This is the core issue we're fixing.
        """
        with patch('bigquery_sync.get_gcp_client') as mock_gcp:
            # Mock BigQuery result with typed data
            mock_row = Mock()
            mock_row.currency_code = "USD"
            mock_row.rate = 11.23  # Should be float, not "11.23"
            mock_row.date = date(2026, 3, 24)
            
            mock_gcp.return_value.query_bigquery.return_value = [mock_row]
            mock_gcp.return_value.project_id = "test-project"
            mock_gcp.return_value.dataset_id = "test_dataset"
            
            from bigquery_sync import query_rate_view
            
            rates = query_rate_view()
            
            assert len(rates) > 0
            # CRITICAL: rate must be float, not string
            assert isinstance(rates[0]["rate"], float)
            assert not isinstance(rates[0]["rate"], str)
    
    def test_null_rate_handling(self):
        """
        RED: Edge case - handle NULL rates safely.
        Should FAIL until we add NULL handling.
        """
        with patch('bigquery_sync.get_gcp_client') as mock_gcp:
            mock_row = Mock()
            mock_row.currency_code = "XXX"
            mock_row.rate = None  # NULL value
            mock_row.date = date(2026, 3, 24)
            
            mock_gcp.return_value.query_bigquery.return_value = [mock_row]
            mock_gcp.return_value.project_id = "test"
            mock_gcp.return_value.dataset_id = "test"
            
            from bigquery_sync import query_rate_view
            
            rates = query_rate_view()
            
            # Should handle NULL gracefully
            assert rates[0]["rate"] is None or isinstance(rates[0]["rate"], float)


# ============================================================================
# PHASE 3: Configuration & Dataset Management Tests
# ============================================================================

class TestDatasetConfiguration:
    """Test dataset configuration and drift detection."""
    
    def test_verify_dataset_config_function_exists(self):
        """
        RED: Should FAIL - function doesn't exist.
        """
        from bigquery_sync import verify_dataset_configuration
        assert callable(verify_dataset_configuration)
    
    def test_detect_missing_views(self):
        """
        RED: Should detect when Power BI views are missing.
        """
        with patch('bigquery_sync.get_gcp_client') as mock_gcp:
            mock_client = Mock()
            mock_client.get_table.side_effect = Exception("Not found")
            mock_gcp.return_value.bigquery = mock_client
            mock_gcp.return_value.project_id = "test"
            mock_gcp.return_value.dataset_id = "test"
            
            from bigquery_sync import verify_dataset_configuration
            
            result = verify_dataset_configuration()
            
            # Should detect missing views
            assert result["views_exist"] is False
            assert "Rate_PowerBI" in result["missing_views"]


# ============================================================================
# PHASE 4: Existing Functionality Tests (Baseline)
# ============================================================================

class TestExistingSync:
    """Test existing sync functions to prevent regressions."""
    
    def test_current_rates_schema_has_float64_rate(self):
        """
        GREEN: This should PASS - verify base schema is correct.
        """
        rate_field = next(f for f in CURRENT_RATES_SCHEMA if f.name == "rate")
        assert rate_field.field_type == "FLOAT64"
        assert rate_field.mode == "REQUIRED"
    
    def test_sync_current_rates_with_valid_data(self):
        """
        GREEN: Test existing sync function with mocked BQ.
        """
        mock_rates = [
            {
                "currency_code": "USD",
                "rate": 11.23,
                "date": "2026-03-24"
            },
            {
                "currency_code": "EUR", 
                "rate": 12.45,
                "date": "2026-03-24"
            }
        ]
        
        with patch('bigquery_sync.ensure_tables_exist', return_value=True), \
             patch('bigquery_sync.get_gcp_client') as mock_gcp:
            
            mock_client = Mock()
            mock_gcp.return_value = mock_client
            mock_gcp.return_value.project_id = "test"
            mock_gcp.return_value.dataset_id = "test"
            mock_gcp.return_value.bigquery.query.return_value.result.return_value = None
            mock_gcp.return_value.insert_rows_to_bigquery = Mock()
            
            result = sync_current_rates_to_bigquery(mock_rates)
            
            assert result is True
            assert mock_gcp.return_value.insert_rows_to_bigquery.called
    
    def test_sync_rejects_empty_rates(self):
        """
        GREEN: Edge case - empty rates list.
        """
        result = sync_current_rates_to_bigquery([])
        assert result is False
    
    def test_sync_handles_invalid_rate_type(self):
        """
        RED: Edge case - rate as string should be converted to float.
        Should FAIL if not handled.
        """
        mock_rates = [
            {
                "currency_code": "USD",
                "rate": "11.23",  # String instead of float
                "date": "2026-03-24"
            }
        ]
        
        with patch('bigquery_sync.ensure_tables_exist', return_value=True), \
             patch('bigquery_sync.get_gcp_client') as mock_gcp:
            
            mock_client = Mock()
            mock_gcp.return_value = mock_client
            mock_gcp.return_value.project_id = "test"
            mock_gcp.return_value.dataset_id = "test"
            mock_gcp.return_value.bigquery.query.return_value.result.return_value = None
            
            # Should handle inserted rows
            inserted_rows = []
            def capture_rows(table, rows):
                inserted_rows.extend(rows)
            
            mock_gcp.return_value.insert_rows_to_bigquery = Mock(side_effect=capture_rows)
            
            result = sync_current_rates_to_bigquery(mock_rates)
            
            assert result is True
            # Verify rate was converted to float
            assert isinstance(inserted_rows[0]["rate"], float)
            assert inserted_rows[0]["rate"] == 11.23


# ============================================================================
# Test Fixtures & Utilities
# ============================================================================

@pytest.fixture
def sample_rates():
    """Fixture providing sample rate data for tests."""
    return [
        {"currency_code": "USD", "rate": 11.23, "date": "2026-03-24"},
        {"currency_code": "EUR", "rate": 12.45, "date": "2026-03-24"},
        {"currency_code": "GBP", "rate": 14.67, "date": "2026-03-24"},
    ]


@pytest.fixture
def mock_gcp_client():
    """Fixture providing a mocked GCP client."""
    with patch('bigquery_sync.get_gcp_client') as mock:
        client = Mock()
        client.project_id = "test-project"
        client.dataset_id = "test_dataset"
        client.bigquery = Mock()
        mock.return_value = client
        yield client


if __name__ == "__main__":
    print("=" * 70)
    print("TDD Test Suite for BigQuery Power BI Integration")
    print("=" * 70)
    print("\nRun with: pytest test_bigquery_sync.py -v")
    print("\nTest Coverage: pytest test_bigquery_sync.py --cov=bigquery_sync --cov-report=term-missing")
    print("\nExpected: Many tests will FAIL initially (RED phase)")
    print("Next step: Implement features to make tests pass (GREEN phase)")
    print("=" * 70)
