"""
BigQuery integration for Currency Service.

This module provides functions to sync exchange rate data to BigQuery
for data warehousing and analytics.

Two tables:
- Rate: Current/latest rates only (one per currency)
- Historyrates: All historical rates for past 3 years
"""

import os
import json
import logging
from datetime import date, datetime, timedelta
from typing import Optional
from google.cloud import bigquery

from shared.gcp_client import get_gcp_client

logger = logging.getLogger(__name__)

RATE_POWERBI_VIEW = "RatePowerBI"
HISTORY_POWERBI_VIEW = "HistoryratesPowerBI"


# BigQuery table schema for CURRENT rates (Rate table)
CURRENT_RATES_SCHEMA = [
    bigquery.SchemaField("currency_code", "STRING", mode="REQUIRED", description="ISO currency code"),
    bigquery.SchemaField("quote_currency", "STRING", mode="REQUIRED", description="Quote currency (usually NOK)"),
    bigquery.SchemaField("rate", "FLOAT64", mode="REQUIRED", description="Exchange rate"),
    bigquery.SchemaField("date", "DATE", mode="REQUIRED", description="Rate date"),
    bigquery.SchemaField("source", "STRING", mode="REQUIRED", description="Data source (norges_bank, fcs, etc)"),
    bigquery.SchemaField("updated_at", "TIMESTAMP", mode="REQUIRED", description="When this record was last updated"),
]

# BigQuery table schema for HISTORICAL rates (Historyrates table)
HISTORICAL_RATES_SCHEMA = [
    bigquery.SchemaField("currency_code", "STRING", mode="REQUIRED", description="ISO currency code"),
    bigquery.SchemaField("quote_currency", "STRING", mode="REQUIRED", description="Quote currency (usually NOK)"),
    bigquery.SchemaField("rate", "FLOAT64", mode="REQUIRED", description="Exchange rate"),
    bigquery.SchemaField("date", "DATE", mode="REQUIRED", description="Rate date"),
    bigquery.SchemaField("source", "STRING", mode="REQUIRED", description="Data source (norges_bank, fcs, etc)"),
    bigquery.SchemaField("created_at", "TIMESTAMP", mode="REQUIRED", description="When this record was created"),
]


def get_rate_powerbi_view_sql(project_id: str, dataset_id: str) -> str:
    """Return SQL for a Power BI-safe projection of the current rates table."""
    return f"""
        CREATE OR REPLACE VIEW `{project_id}.{dataset_id}.{RATE_POWERBI_VIEW}` AS
        SELECT
            CAST(currency_code AS STRING) AS currency_code,
            CAST(quote_currency AS STRING) AS quote_currency,
            SAFE_CAST(rate AS FLOAT64) AS rate,
            CAST(date AS DATE) AS date,
            CAST(source AS STRING) AS source,
            CAST(updated_at AS TIMESTAMP) AS updated_at
        FROM `{project_id}.{dataset_id}.Rate`
    """


def get_history_powerbi_view_sql(project_id: str, dataset_id: str) -> str:
    """Return SQL for a Power BI-safe projection of the historical rates table."""
    return f"""
        CREATE OR REPLACE VIEW `{project_id}.{dataset_id}.{HISTORY_POWERBI_VIEW}` AS
        SELECT
            CAST(currency_code AS STRING) AS currency_code,
            CAST(quote_currency AS STRING) AS quote_currency,
            SAFE_CAST(rate AS FLOAT64) AS rate,
            CAST(date AS DATE) AS date,
            CAST(source AS STRING) AS source,
            CAST(created_at AS TIMESTAMP) AS created_at
        FROM `{project_id}.{dataset_id}.Historyrates`
    """


def ensure_powerbi_views_exist() -> bool:
    """Ensure Power BI-safe BigQuery views exist for both current and historical rates."""
    try:
        gcp = get_gcp_client()
        gcp.bigquery.query(get_rate_powerbi_view_sql(gcp.project_id, gcp.dataset_id)).result()
        gcp.bigquery.query(get_history_powerbi_view_sql(gcp.project_id, gcp.dataset_id)).result()
        logger.info(
            "Power BI BigQuery views ready: %s, %s",
            RATE_POWERBI_VIEW,
            HISTORY_POWERBI_VIEW,
        )
        return True
    except Exception as e:
        logger.error(f"Failed to ensure Power BI BigQuery views exist: {e}")
        return False


def ensure_tables_exist() -> bool:
    """
    Ensure both Rate and Historyrates tables exist in BigQuery.
    Creates the tables if they don't exist.
    
    Returns:
        True if both tables exist or were created successfully
    """
    try:
        gcp = get_gcp_client()
        
        # Create Rate table (current rates only)
        rate_table = gcp.create_bigquery_table(
            table_id="Rate",
            schema=CURRENT_RATES_SCHEMA,
            partitioning_field=None  # No partitioning, small table with latest rates only
        )
        logger.info(f"Rate table ready: {rate_table.full_table_id}")
        
        # Create Historyrates table (all historical rates)
        history_table = gcp.create_bigquery_table(
            table_id="Historyrates",
            schema=HISTORICAL_RATES_SCHEMA,
            partitioning_field="date"  # Partition by date for better query performance
        )
        logger.info(f"Historyrates table ready: {history_table.full_table_id}")

        if not ensure_powerbi_views_exist():
            return False
        
        return True
        
    except Exception as e:
        logger.error(f"Failed to ensure tables exist: {e}")
        return False


# Backward compatibility
def ensure_table_exists() -> bool:
    """Backward compatibility wrapper"""
    return ensure_tables_exist()


def sync_current_rates_to_bigquery(
    rates: list[dict],
    source: str = "currency_service",
    quote_currency: str = "NOK"
) -> bool:
    """
    Sync ONLY the latest/current exchange rates to Rate table.
    This table should only contain one rate per currency (the most recent).
    
    Uses MERGE strategy to update existing rates or insert new ones.
    
    Args:
        rates: List of LATEST rate dictionaries with keys: currency_code, rate, date
        source: Data source identifier (e.g., 'norges_bank', 'fcs', 'currency_service')
        quote_currency: The quote currency (default: NOK)
    
    Returns:
        True if sync was successful
    """
    if not rates:
        logger.warning("No current rates to sync to BigQuery Rate table")
        return False
    
    try:
        if not ensure_tables_exist():
            return False
        
        gcp = get_gcp_client()
        now = datetime.utcnow().isoformat()
        
        # Get only the latest rate per currency
        latest_rates = {}
        for rate in rates:
            currency = rate["currency_code"]
            rate_date = rate["date"]
            
            if currency not in latest_rates or rate_date > latest_rates[currency]["date"]:
                latest_rates[currency] = rate
        
        # Build rows to write
        rows = []
        for rate in latest_rates.values():
            rows.append({
                "currency_code": rate["currency_code"],
                "quote_currency": quote_currency,
                "rate": float(rate["rate"]),
                "date": rate["date"],
                "source": source,
                "updated_at": now
            })

        # Use a load job with WRITE_TRUNCATE to atomically replace the Rate table.
        # This avoids the streaming-buffer restriction that prevents DELETE/UPDATE DML
        # on rows recently inserted via the streaming API.
        table_ref = f"{gcp.project_id}.{gcp.dataset_id}.Rate"
        job_config = bigquery.LoadJobConfig(
            write_disposition=bigquery.WriteDisposition.WRITE_TRUNCATE,
            source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
            schema=CURRENT_RATES_SCHEMA,
        )
        load_job = gcp.bigquery.load_table_from_json(rows, table_ref, job_config=job_config)
        load_job.result()  # wait for job to complete
        logger.info("Rate table replaced via load job (WRITE_TRUNCATE)")
        
        logger.info(f"Successfully synced {len(rows)} current rates to BigQuery Rate table")
        return True
        
    except Exception as e:
        logger.error(f"Failed to sync current rates to BigQuery: {e}")
        return False


# Earliest date we store in the historical table
HISTORY_START_DATE = date(2023, 1, 1)


def sync_historical_rates_to_bigquery(
    rates: list[dict],
    source: str = "currency_service",
    quote_currency: str = "NOK",
) -> bool:
    """
    APPEND new exchange rate records to the Historyrates table.

    Records already present (matched by currency_code + date) are skipped so this
    function is safe to call repeatedly without creating duplicates.
    History starts from HISTORY_START_DATE (2023-01-01).

    Args:
        rates: List of rate dicts with keys: currency_code, rate, date
        source: Data source identifier
        quote_currency: Quote currency (default: NOK)

    Returns:
        True if sync was successful
    """
    if not rates:
        logger.warning("No historical rates to sync to BigQuery Historyrates table")
        return False

    try:
        if not ensure_tables_exist():
            return False

        gcp = get_gcp_client()
        now = datetime.utcnow().isoformat()

        # Build candidate rows, keeping only dates from 2023-01-01 onward
        rows = []
        for rate in rates:
            rate_date_str = rate["date"]
            if isinstance(rate_date_str, str):
                try:
                    rate_date = datetime.fromisoformat(rate_date_str).date()
                except ValueError:
                    rate_date = datetime.strptime(rate_date_str, "%Y-%m-%d").date()
            else:
                rate_date = rate_date_str

            if rate_date >= HISTORY_START_DATE:
                rows.append({
                    "currency_code": rate["currency_code"],
                    "quote_currency": quote_currency,
                    "rate": float(rate["rate"]),
                    "date": rate_date.isoformat(),
                    "source": source,
                    "created_at": now,
                })

        if not rows:
            logger.info("No historical rates from 2023+ to sync")
            return True

        # Find the date range we are trying to insert
        dates = [r["date"] for r in rows]
        min_date, max_date = min(dates), max(dates)

        # Query which (currency_code, date) pairs already exist in that range
        # The table is partitioned by date so this is efficient
        existing_query = f"""
            SELECT currency_code, CAST(date AS STRING) AS date_str
            FROM `{gcp.project_id}.{gcp.dataset_id}.Historyrates`
            WHERE date BETWEEN @min_date AND @max_date
        """
        try:
            job_config = bigquery.QueryJobConfig(
                query_parameters=[
                    bigquery.ScalarQueryParameter("min_date", "DATE", min_date),
                    bigquery.ScalarQueryParameter("max_date", "DATE", max_date),
                ]
            )
            existing_keys = {
                (row.currency_code, row.date_str)
                for row in gcp.bigquery.query(existing_query, job_config=job_config).result()
            }
        except Exception:
            existing_keys = set()

        # Only insert truly new records (APPEND – never delete)
        new_rows = [
            r for r in rows
            if (r["currency_code"], r["date"]) not in existing_keys
        ]

        if not new_rows:
            logger.info("No new historical rates to append – already up to date")
            return True

        gcp.insert_rows_to_bigquery("Historyrates", new_rows)
        logger.info(f"Appended {len(new_rows)} new historical rates to BigQuery Historyrates table")
        return True

    except Exception as e:
        logger.error(f"Failed to sync historical rates to BigQuery: {e}")
        return False


def sync_rates_to_bigquery(
    rates: list[dict],
    source: str = "norges_bank",
    quote_currency: str = "NOK"
) -> bool:
    """
    Main sync function - syncs to BOTH Rate and Historyrates tables.
    
    Args:
        rates: List of rate dictionaries with keys: currency_code, rate, date
        source: Data source identifier (e.g., 'norges_bank', 'fcs', 'abstract')
        quote_currency: The quote currency (default: NOK)
    
    Returns:
        True if both syncs were successful
    """
    if not rates:
        logger.warning("No rates to sync to BigQuery")
        return False
    
    # Sync to both tables
    current_success = sync_current_rates_to_bigquery(rates, source, quote_currency)
    historical_success = sync_historical_rates_to_bigquery(rates, source, quote_currency)

    return current_success and historical_success


def get_latest_rates_from_bigquery(
    currency_codes: Optional[list[str]] = None,
    limit: int = 100
) -> list[dict]:
    """
    Retrieve latest/current exchange rates from Rate table.
    
    Args:
        currency_codes: Optional list of currency codes to filter by
        limit: Maximum number of results to return
        
    Returns:
        List of current rate dictionaries
    """
    try:
        gcp = get_gcp_client()
        
        # Build query
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
            FROM `{gcp.project_id}.{gcp.dataset_id}.Rate`
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
                "rate": row.rate,
                "date": row.date.isoformat() if hasattr(row.date, 'isoformat') else str(row.date),
                "source": row.source,
                "updated_at": row.updated_at.isoformat() if hasattr(row.updated_at, 'isoformat') else str(row.updated_at)
            })
        
        logger.info(f"Retrieved {len(rates)} current rates from BigQuery Rate table")
        return rates
        
    except Exception as e:
        logger.error(f"Failed to get current rates from BigQuery: {e}")
        return []


def get_historical_rates(
    currency_code: str,
    start_date: str,
    end_date: str,
    source: Optional[str] = None
) -> list[dict]:
    """
    Get historical exchange rates for a specific currency from Historyrates table.
    
    Args:
        currency_code: ISO currency code (e.g., 'USD')
        start_date: Start date (YYYY-MM-DD)
        end_date: End date (YYYY-MM-DD)
        source: Optional data source filter
        
    Returns:
        List of historical rates ordered by date
    """
    try:
        gcp = get_gcp_client()
        
        source_clause = f"AND source = '{source}'" if source else ""
        
        query = f"""
            SELECT 
                currency_code,
                quote_currency,
                rate,
                date,
                source,
                created_at
            FROM `{gcp.project_id}.{gcp.dataset_id}.Historyrates`
            WHERE currency_code = '{currency_code}'
              AND date BETWEEN '{start_date}' AND '{end_date}'
              {source_clause}
            ORDER BY date ASC
        """
        
        results = gcp.query_bigquery(query)
        
        rates = []
        for row in results:
            rates.append({
                "currency_code": row.currency_code,
                "quote_currency": row.quote_currency,
                "rate": row.rate,
                "date": row.date.isoformat() if hasattr(row.date, 'isoformat') else str(row.date),
                "source": row.source,
                "created_at": row.created_at.isoformat() if hasattr(row.created_at, 'isoformat') else str(row.created_at)
            })
        
        logger.info(f"Retrieved {len(rates)} historical rates for {currency_code} from Historyrates table")
        return rates
        
    except Exception as e:
        logger.error(f"Failed to get historical rates: {e}")
        return []


def backup_rates_to_storage(rates: list[dict], filename: str) -> Optional[str]:
    """
    Backup exchange rates to Google Cloud Storage.
    
    Args:
        rates: List of rate dictionaries
        filename: Filename for the backup (e.g., 'rates-2024-12-19.json')
        
    Returns:
        GCS URL of the backup file, or None if failed
        
    Example:
        ```python
        rates = get_all_rates()
        url = backup_rates_to_storage(rates, "backup-2024-12-19.json")
        print(f"Backup saved to: {url}")
        ```
    """
    try:
        import json
        from datetime import datetime
        
        gcp = get_gcp_client()
        bucket_name = os.getenv("GCP_BUCKET_NAME")
        
        # Prepare backup data
        backup_data = {
            "timestamp": datetime.utcnow().isoformat(),
            "count": len(rates),
            "rates": rates
        }
        
        # Upload to Cloud Storage
        destination = f"currency/backups/{filename}"
        url = gcp.upload_to_storage(
            bucket_name=bucket_name,
            destination_blob_name=destination,
            data=json.dumps(backup_data, indent=2).encode(),
            content_type="application/json"
        )
        
        logger.info(f"Backed up {len(rates)} rates to: {url}")
        return url
        
    except Exception as e:
        logger.error(f"Failed to backup rates to storage: {e}")
        return None
