"""
Power BI Sync Service
Handles synchronization of currency data from PostgreSQL to SQL Server for Power BI
"""

import logging
from datetime import datetime, timedelta
from sqlalchemy.orm import Session
from typing import Tuple, Optional

from models import ExchangeRate, get_session as get_pg_session
from powerbi_models import Rate, PowerBISyncLog, get_powerbi_session

logger = logging.getLogger(__name__)


class PowerBISyncService:
    """Service for syncing data to Power BI SQL Server"""
    
    @staticmethod
    def sync_to_powerbi(sync_type: str = 'manual', days_back: int = 90) -> Tuple[bool, str, int]:
        """
        Sync currency data from PostgreSQL to SQL Server for Power BI
        
        Args:
            sync_type: Type of sync ('manual' or 'scheduled')
            days_back: Number of days of historical data to sync
            
        Returns:
            Tuple of (success: bool, message: str, records_synced: int)
        """
        pg_session = None
        powerbi_session = None
        sync_log = None
        start_time = datetime.utcnow()
        
        try:
            # Get database sessions
            pg_session = get_pg_session()
            powerbi_session = get_powerbi_session()
            
            # Create sync log
            sync_log = PowerBISyncLog(
                sync_type=sync_type,
                status='running',
                started_at=start_time
            )
            powerbi_session.add(sync_log)
            powerbi_session.commit()
            
            logger.info(f"Starting Power BI sync (type: {sync_type}, days_back: {days_back})")
            
            # Get cutoff date
            cutoff_date = datetime.utcnow() - timedelta(days=days_back)
            
            # Fetch rates from PostgreSQL
            pg_rates = pg_session.query(ExchangeRate).filter(
                ExchangeRate.timestamp >= cutoff_date
            ).all()
            
            logger.info(f"Fetched {len(pg_rates)} rates from PostgreSQL")
            
            if not pg_rates:
                message = "No rates found to sync"
                logger.warning(message)
                
                # Update sync log
                sync_log.status = 'success'
                sync_log.records_synced = 0
                sync_log.completed_at = datetime.utcnow()
                sync_log.duration_ms = int((datetime.utcnow() - start_time).total_seconds() * 1000)
                powerbi_session.commit()
                
                return True, message, 0
            
            # Sync rates to SQL Server
            records_synced = 0
            errors = []
            
            for pg_rate in pg_rates:
                try:
                    # Transform PostgreSQL format to Power BI format
                    # PostgreSQL: from_currency -> to_currency
                    # Power BI: base -> quote (where quote is always NOK for Norwegian views)
                    
                    # We store both directions for flexibility
                    # 1. Direct: USD -> NOK
                    if pg_rate.to_currency == 'NOK':
                        base = pg_rate.from_currency
                        quote = pg_rate.to_currency
                        value = pg_rate.rate
                    # 2. Inverse: NOK -> USD (calculate inverse)
                    elif pg_rate.from_currency == 'NOK':
                        base = pg_rate.to_currency
                        quote = pg_rate.from_currency
                        value = 1.0 / pg_rate.rate if pg_rate.rate != 0 else 0
                    else:
                        # For non-NOK pairs, skip for now (can be extended later)
                        continue
                    
                    # Check if rate already exists (upsert logic)
                    existing_rate = powerbi_session.query(Rate).filter(
                        Rate.base == base,
                        Rate.quote == quote,
                        Rate.date == pg_rate.timestamp,
                        Rate.src == pg_rate.source
                    ).first()
                    
                    if existing_rate:
                        # Update existing rate
                        existing_rate.value = value
                        existing_rate.updated_at = datetime.utcnow()
                    else:
                        # Insert new rate
                        new_rate = Rate(
                            date=pg_rate.timestamp,
                            base=base,
                            quote=quote,
                            value=value,
                            src=pg_rate.source
                        )
                        powerbi_session.add(new_rate)
                    
                    records_synced += 1
                    
                    # Commit in batches to avoid memory issues
                    if records_synced % 100 == 0:
                        powerbi_session.commit()
                        logger.debug(f"Synced {records_synced} records...")
                    
                except Exception as e:
                    error_msg = f"Error syncing rate {pg_rate.id}: {str(e)}"
                    logger.error(error_msg)
                    errors.append(error_msg)
                    # Continue with next rate
            
            # Final commit
            powerbi_session.commit()
            
            # Update sync log
            end_time = datetime.utcnow()
            duration_ms = int((end_time - start_time).total_seconds() * 1000)
            
            sync_log.status = 'success' if not errors else 'partial'
            sync_log.records_synced = records_synced
            sync_log.errors = '; '.join(errors[:5]) if errors else None  # Store first 5 errors
            sync_log.completed_at = end_time
            sync_log.duration_ms = duration_ms
            powerbi_session.commit()
            
            message = f"Successfully synced {records_synced} records to Power BI SQL Server"
            if errors:
                message += f" with {len(errors)} error(s)"
            
            logger.info(message)
            return True, message, records_synced
            
        except Exception as e:
            error_msg = f"Power BI sync failed: {str(e)}"
            logger.error(error_msg, exc_info=True)
            
            # Update sync log with error
            if sync_log and powerbi_session:
                try:
                    sync_log.status = 'failed'
                    sync_log.errors = error_msg[:2000]  # Truncate to fit column
                    sync_log.completed_at = datetime.utcnow()
                    sync_log.duration_ms = int((datetime.utcnow() - start_time).total_seconds() * 1000)
                    powerbi_session.commit()
                except Exception as log_error:
                    logger.error(f"Failed to update sync log: {log_error}")
            
            return False, error_msg, 0
            
        finally:
            # Close sessions
            if pg_session:
                pg_session.close()
            if powerbi_session:
                powerbi_session.close()
    
    @staticmethod
    def get_sync_status() -> dict:
        """Get the status of Power BI synchronization"""
        powerbi_session = None
        
        try:
            powerbi_session = get_powerbi_session()
            
            # Get latest sync log
            latest_sync = powerbi_session.query(PowerBISyncLog).order_by(
                PowerBISyncLog.started_at.desc()
            ).first()
            
            # Get total records in Power BI database
            total_records = powerbi_session.query(Rate).count()
            
            # Get date range
            from sqlalchemy import func
            date_stats = powerbi_session.query(
                func.min(Rate.date).label('min_date'),
                func.max(Rate.date).label('max_date'),
                func.count(func.distinct(Rate.base)).label('currency_count')
            ).first()
            
            return {
                'status': 'healthy',
                'total_records': total_records,
                'date_range': {
                    'earliest': date_stats.min_date.isoformat() if date_stats.min_date else None,
                    'latest': date_stats.max_date.isoformat() if date_stats.max_date else None
                },
                'currency_count': date_stats.currency_count or 0,
                'last_sync': latest_sync.to_dict() if latest_sync else None
            }
            
        except Exception as e:
            logger.error(f"Failed to get Power BI sync status: {e}")
            return {
                'status': 'error',
                'error': str(e),
                'total_records': 0,
                'date_range': {'earliest': None, 'latest': None},
                'currency_count': 0,
                'last_sync': None
            }
            
        finally:
            if powerbi_session:
                powerbi_session.close()
    
    @staticmethod
    def get_sync_history(limit: int = 10) -> list:
        """Get recent sync history"""
        powerbi_session = None
        
        try:
            powerbi_session = get_powerbi_session()
            
            sync_logs = powerbi_session.query(PowerBISyncLog).order_by(
                PowerBISyncLog.started_at.desc()
            ).limit(limit).all()
            
            return [log.to_dict() for log in sync_logs]
            
        except Exception as e:
            logger.error(f"Failed to get sync history: {e}")
            return []
            
        finally:
            if powerbi_session:
                powerbi_session.close()
