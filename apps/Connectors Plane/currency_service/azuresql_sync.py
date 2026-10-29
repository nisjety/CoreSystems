"""
Azure SQL synchronization for Currency Service

This module syncs current exchange rates from PostgreSQL to Azure SQL Server
for Power BI and other business intelligence tools.
"""

import os
import logging
from datetime import datetime
from typing import Tuple, Optional
import pyodbc

from models import get_session, ExchangeRate

logger = logging.getLogger(__name__)


def get_azure_sql_connection():
    """
    Create and return Azure SQL Server connection.
    
    Returns:
        pyodbc connection or None if connection fails
    """
    try:
        server = os.getenv("POWERBI_SQL_SERVER")
        database = os.getenv("POWERBI_SQL_DATABASE")
        username = os.getenv("POWERBI_SQL_USERNAME")
        password = os.getenv("POWERBI_SQL_PASSWORD")
        driver = os.getenv("POWERBI_SQL_DRIVER", "ODBC Driver 17 for SQL Server")
        
        if not all([server, database, username, password]):
            logger.error("Missing Azure SQL configuration in environment variables")
            return None
        
        connection_string = f"Driver={{{driver}}};Server=tcp:{server},1433;Database={database};Uid={username};Pwd={password};Encrypt=yes;TrustServerCertificate=no;Connection Timeout=30;"
        
        connection = pyodbc.connect(connection_string)
        logger.debug(f"Connected to Azure SQL: {server}/{database}")
        return connection
        
    except pyodbc.Error as e:
        logger.error(f"Failed to connect to Azure SQL: {str(e)}")
        return None
    except Exception as e:
        logger.error(f"Unexpected error connecting to Azure SQL: {str(e)}")
        return None


def ensure_azure_sql_tables_exist() -> bool:
    """
    Ensure required tables exist in Azure SQL for currency rates.
    Creates tables if they don't exist.
    
    Returns:
        True if tables exist or were created successfully
    """
    connection = get_azure_sql_connection()
    if not connection:
        return False
    
    try:
        cursor = connection.cursor()
        
        # Create Rate table (current rates only)
        rate_table_sql = """
        IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'Rate')
        BEGIN
            CREATE TABLE [dbo].[Rate] (
                [id] INT PRIMARY KEY IDENTITY(1,1),
                [date] DATETIME NOT NULL,
                [base] VARCHAR(3) NOT NULL,
                [quote] VARCHAR(3) NOT NULL,
                [value] DECIMAL(18, 8) NOT NULL,
                [src] VARCHAR(50) NOT NULL,
                [created_at] DATETIME DEFAULT GETUTCDATE(),
                [updated_at] DATETIME DEFAULT GETUTCDATE()
            );
            
            CREATE INDEX [idx_rate_base_quote_date] ON [dbo].[Rate] ([base], [quote], [date]);
            CREATE INDEX [idx_rate_date] ON [dbo].[Rate] ([date]);
            
            PRINT 'Created Rate table';
        END
        """
        
        cursor.execute(rate_table_sql)
        connection.commit()
        logger.info("Rate table ready in Azure SQL")
        
        cursor.close()
        return True
        
    except pyodbc.Error as e:
        logger.error(f"Failed to create Azure SQL tables: {str(e)}")
        return False
    except Exception as e:
        logger.error(f"Unexpected error creating Azure SQL tables: {str(e)}")
        return False
    finally:
        connection.close()


def sync_rates_to_azure_sql(quote_currency: str = "NOK") -> Tuple[bool, str, int]:
    """
    Sync current exchange rates from PostgreSQL to Azure SQL Rate table.
    
    This function:
    1. Reads the latest rate for each currency pair from PostgreSQL
    2. Clears the Azure SQL Rate table
    3. Inserts the latest rates
    
    Args:
        quote_currency: The quote currency (default: NOK)
    
    Returns:
        Tuple of (success: bool, message: str, records_synced: int)
    """
    pg_session = None
    azure_connection = None
    
    try:
        # Get PostgreSQL session
        pg_session = get_session()
        
        # Ensure Azure SQL tables exist
        if not ensure_azure_sql_tables_exist():
            return False, "Failed to ensure Azure SQL tables exist", 0
        
        # Get latest rate for each currency pair (where quote is NOK)
        from sqlalchemy import func
        
        # Find the latest timestamp per currency pair
        latest_ts_subquery = pg_session.query(
            ExchangeRate.from_currency,
            ExchangeRate.to_currency,
            func.max(ExchangeRate.timestamp).label('max_ts')
        ).filter(
            ExchangeRate.to_currency == quote_currency
        ).group_by(
            ExchangeRate.from_currency,
            ExchangeRate.to_currency
        ).subquery()
        
        # Get the latest rate records
        latest_rates = pg_session.query(ExchangeRate).join(
            latest_ts_subquery,
            (ExchangeRate.from_currency == latest_ts_subquery.c.from_currency) &
            (ExchangeRate.to_currency == latest_ts_subquery.c.to_currency) &
            (ExchangeRate.timestamp == latest_ts_subquery.c.max_ts)
        ).all()
        
        if not latest_rates:
            logger.warning("No rates found in PostgreSQL to sync")
            return False, "No rates found in PostgreSQL", 0
        
        logger.info(f"Found {len(latest_rates)} latest rates in PostgreSQL")
        
        # Connect to Azure SQL
        azure_connection = get_azure_sql_connection()
        if not azure_connection:
            return False, "Failed to connect to Azure SQL", 0
        
        cursor = azure_connection.cursor()
        
        # Clear existing rates (replace strategy for current rates)
        try:
            cursor.execute("TRUNCATE TABLE [dbo].[Rate]")
            azure_connection.commit()
            logger.info("Cleared Azure SQL Rate table")
        except Exception as e:
            logger.warning(f"Could not truncate Rate table: {str(e)}, will attempt to delete instead")
            try:
                cursor.execute("DELETE FROM [dbo].[Rate]")
                azure_connection.commit()
            except Exception as e2:
                logger.error(f"Failed to clear Rate table: {str(e2)}")
                return False, f"Failed to clear Rate table: {str(e2)}", 0
        
        # Insert latest rates
        now = datetime.utcnow()
        rows_inserted = 0
        
        for rate in latest_rates:
            try:
                insert_sql = """
                INSERT INTO [dbo].[Rate] ([date], [base], [quote], [value], [src], [created_at], [updated_at])
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """
                
                cursor.execute(insert_sql, (
                    rate.timestamp,
                    rate.from_currency,
                    rate.to_currency,
                    float(rate.rate),
                    "postgresql",
                    now,
                    now
                ))
                rows_inserted += 1
                
            except Exception as e:
                logger.error(f"Failed to insert rate {rate.from_currency}/{rate.to_currency}: {str(e)}")
                continue
        
        azure_connection.commit()
        logger.info(f"Inserted {rows_inserted} rates into Azure SQL Rate table")
        
        cursor.close()
        
        if rows_inserted > 0:
            message = f"Successfully synced {rows_inserted} current rates to Azure SQL"
            logger.info(message)
            return True, message, rows_inserted
        else:
            message = "No rates could be inserted into Azure SQL"
            logger.warning(message)
            return False, message, 0
        
    except Exception as e:
        error_msg = f"Azure SQL sync failed: {str(e)}"
        logger.error(error_msg, exc_info=True)
        return False, error_msg, 0
        
    finally:
        if pg_session:
            pg_session.close()
        if azure_connection:
            try:
                azure_connection.close()
            except:
                pass


def verify_azure_sql_sync() -> dict:
    """
    Verify that Azure SQL has been synced with current data.
    
    Returns:
        Dictionary with sync verification results
    """
    try:
        connection = get_azure_sql_connection()
        if not connection:
            return {"status": "error", "message": "Failed to connect to Azure SQL"}
        
        cursor = connection.cursor()
        
        # Check Rate table
        cursor.execute("""
        SELECT 
            COUNT(*) as total_records,
            MIN([date]) as min_date,
            MAX([date]) as max_date
        FROM [dbo].[Rate]
        """)
        
        result = cursor.fetchone()
        
        cursor.close()
        connection.close()
        
        return {
            "status": "success",
            "total_records": result[0] if result[0] else 0,
            "min_date": str(result[1]) if result[1] else None,
            "max_date": str(result[2]) if result[2] else None
        }
        
    except Exception as e:
        logger.error(f"Failed to verify Azure SQL sync: {str(e)}")
        return {"status": "error", "message": str(e)}
