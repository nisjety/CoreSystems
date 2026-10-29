"""
Scheduled jobs for Currency Service
"""

import logging
from datetime import datetime
from apscheduler.schedulers.background import BackgroundScheduler
from apscheduler.triggers.cron import CronTrigger

from models import get_session
from crud import CurrencyRateService, SyncLogService, ServiceLogService
from sources import NorgesBankSource, FCSAPISource
from config import config

logger = logging.getLogger(__name__)

# Import Power BI sync (will be imported conditionally to avoid errors if SQL Server is not configured)
try:
    from powerbi_sync import PowerBISyncService
    POWERBI_AVAILABLE = True
except Exception as e:
    logger.warning(f"Power BI sync not available: {e}")
    POWERBI_AVAILABLE = False

# Import BigQuery sync (will be imported conditionally to avoid errors if GCP is not configured)
try:
    from bigquery_sync import sync_rates_to_bigquery, ensure_table_exists
    BIGQUERY_AVAILABLE = True
except Exception as e:
    logger.warning(f"BigQuery sync not available: {e}")
    BIGQUERY_AVAILABLE = False

# Import Azure SQL sync (will be imported conditionally to avoid errors if Azure SQL is not configured)
try:
    from azuresql_sync import sync_rates_to_azure_sql
    AZURE_SQL_AVAILABLE = True
except Exception as e:
    logger.warning(f"Azure SQL sync not available: {e}")
    AZURE_SQL_AVAILABLE = False


class CurrencyScheduler:
    """Scheduler for currency rate synchronization"""
    
    def __init__(self, norges_bank: NorgesBankSource, fcs_api: FCSAPISource):
        self.scheduler = BackgroundScheduler()
        self.norges_bank = norges_bank
        self.fcs_api = fcs_api
        self.is_running = False
        
    def sync_rates(self):
        """Synchronize rates from all sources"""
        session = get_session()
        sync_log = None
        
        try:
            # Create sync log
            sync_log = SyncLogService.create_log(session, 'scheduled')
            logger.info("Starting scheduled rate synchronization")
            
            rates_updated = 0
            errors = []
            
            # Sync from Norges Bank
            try:
                nb_rates = self.norges_bank.get_latest_rates(
                    currencies=config.NORGES_BANK_CURRENCIES
                )
                
                for rate in nb_rates:
                    from_curr = rate['base'] if rate['quote'] == 'NOK' else 'NOK'
                    to_curr = rate['quote'] if rate['quote'] == 'NOK' else rate['base']
                    
                    CurrencyRateService.upsert_rate(
                        session,
                        from_curr,
                        to_curr,
                        rate['value'],
                        'norgesbank'
                    )
                    rates_updated += 1
                
                logger.info(f"Synced {len(nb_rates)} rates from Norges Bank")
                ServiceLogService.add_log(
                    session, 'INFO',
                    f"Synced {len(nb_rates)} rates from Norges Bank"
                )
                
            except Exception as e:
                error_msg = f"Norges Bank sync failed: {str(e)}"
                errors.append(error_msg)
                logger.error(error_msg)
                ServiceLogService.add_log(session, 'ERROR', error_msg)
            
            # Sync from FCS API
            if self.fcs_api.api_key:
                try:
                    fcs_rates = self.fcs_api.get_latest_rates(
                        base='NOK',
                        targets=config.FCS_CURRENCIES
                    )
                    
                    for rate in fcs_rates:
                        from_curr = rate['base'] if rate['quote'] == 'NOK' else 'NOK'
                        to_curr = rate['quote'] if rate['quote'] == 'NOK' else rate['base']
                        
                        CurrencyRateService.upsert_rate(
                            session,
                            from_curr,
                            to_curr,
                            rate['value'],
                            'fcs'
                        )
                        rates_updated += 1
                    
                    logger.info(f"Synced {len(fcs_rates)} rates from FCS API")
                    ServiceLogService.add_log(
                        session, 'INFO',
                        f"Synced {len(fcs_rates)} rates from FCS API"
                    )
                    
                except Exception as e:
                    error_msg = f"FCS API sync failed: {str(e)}"
                    errors.append(error_msg)
                    logger.error(error_msg)
                    ServiceLogService.add_log(session, 'ERROR', error_msg)
            
            # Complete sync log
            status = 'success' if not errors else ('partial' if rates_updated > 0 else 'failed')
            SyncLogService.complete_log(
                session,
                sync_log.id,
                status,
                rates_updated,
                '; '.join(errors) if errors else None
            )
            
            logger.info(f"Sync completed: {rates_updated} rates updated, status: {status}")
            ServiceLogService.add_log(
                session, 'INFO',
                f"Sync completed: {rates_updated} rates updated"
            )
            
        except Exception as e:
            logger.error(f"Sync failed: {str(e)}")
            if sync_log:
                SyncLogService.complete_log(
                    session, sync_log.id, 'failed', 0, str(e)
                )
            ServiceLogService.add_log(session, 'ERROR', f"Sync failed: {str(e)}")
        finally:
            session.close()
    
    def sync_to_powerbi(self):
        """Synchronize data to Power BI SQL Server"""
        if not POWERBI_AVAILABLE:
            logger.warning("Power BI sync not available - skipping")
            return
        
        try:
            logger.info("Starting Power BI sync")
            success, message, records_synced = PowerBISyncService.sync_to_powerbi(
                sync_type='scheduled',
                days_back=90  # Sync last 90 days
            )
            
            if success:
                logger.info(f"Power BI sync completed: {message}")
            else:
                logger.error(f"Power BI sync failed: {message}")
                
        except Exception as e:
            logger.error(f"Power BI sync error: {str(e)}", exc_info=True)
    
    def sync_to_bigquery(self):
        """Synchronize data to BigQuery (GCP) - both current and historical rates"""
        if not BIGQUERY_AVAILABLE:
            logger.warning("BigQuery sync not available - skipping")
            return

        session = get_session()
        try:
            logger.info("Starting BigQuery sync")

            # Ensure tables exist
            if not ensure_table_exists():
                logger.error("Failed to ensure BigQuery tables exist")
                return

            from models import ExchangeRate
            from sqlalchemy import func
            from bigquery_sync import sync_current_rates_to_bigquery, sync_historical_rates_to_bigquery

            # 1. Get LATEST rate per currency pair for the Rate (current) table.
            #    ExchangeRate stores from_currency / to_currency / timestamp.
            #    We only care about pairs where NOK is the quote (to) currency.
            latest_ts_subquery = session.query(
                ExchangeRate.from_currency,
                ExchangeRate.to_currency,
                func.max(ExchangeRate.timestamp).label('max_ts')
            ).filter(
                ExchangeRate.to_currency == 'NOK'
            ).group_by(
                ExchangeRate.from_currency,
                ExchangeRate.to_currency,
            ).subquery()

            latest_rates_query = session.query(ExchangeRate).join(
                latest_ts_subquery,
                (ExchangeRate.from_currency == latest_ts_subquery.c.from_currency) &
                (ExchangeRate.to_currency == latest_ts_subquery.c.to_currency) &
                (ExchangeRate.timestamp == latest_ts_subquery.c.max_ts)
            ).all()

            current_rates = []
            for rate in latest_rates_query:
                current_rates.append({
                    "currency_code": rate.from_currency,
                    "rate": rate.rate,
                    "date": rate.timestamp.date().isoformat(),
                })

            # 2. Get all rates from 2023-01-01 onward for the Historyrates table.
            history_start = datetime(2023, 1, 1)
            historical_rates_query = session.query(ExchangeRate).filter(
                ExchangeRate.to_currency == 'NOK',
                ExchangeRate.timestamp >= history_start,
            ).order_by(ExchangeRate.timestamp.desc()).all()

            historical_rates = []
            for rate in historical_rates_query:
                historical_rates.append({
                    "currency_code": rate.from_currency,
                    "rate": rate.rate,
                    "date": rate.timestamp.date().isoformat(),
                })

            # Sync current rates → Rate table (full overwrite daily)
            current_success = sync_current_rates_to_bigquery(
                current_rates,
                source="currency_service",
                quote_currency="NOK",
            )

            # Sync historical rates → Historyrates table (append only)
            historical_success = sync_historical_rates_to_bigquery(
                historical_rates,
                source="currency_service",
                quote_currency="NOK",
            )
            
            if current_success and historical_success:
                logger.info(f"BigQuery sync completed: {len(current_rates)} current rates, {len(historical_rates)} historical rates")
                ServiceLogService.add_log(
                    session, 'INFO',
                    f"BigQuery sync completed: {len(current_rates)} current rates, {len(historical_rates)} historical rates"
                )
            else:
                error_msg = []
                if not current_success:
                    error_msg.append("Rate table sync failed")
                if not historical_success:
                    error_msg.append("Historyrates table sync failed")
                logger.error(f"BigQuery sync errors: {'; '.join(error_msg)}")
                ServiceLogService.add_log(
                    session, 'ERROR',
                    f"BigQuery sync errors: {'; '.join(error_msg)}"
                )
            
            session.commit()
                
        except Exception as e:
            logger.error(f"BigQuery sync error: {str(e)}", exc_info=True)
            ServiceLogService.add_log(
                session, 'ERROR',
                f"BigQuery sync error: {str(e)}"
            )
            session.commit()
        finally:
            session.close()

    def sync_to_azure_sql(self):
        """Synchronize data to Azure SQL Database for Power BI"""
        if not AZURE_SQL_AVAILABLE:
            logger.warning("Azure SQL sync not available - skipping")
            return
        
        session = get_session()
        try:
            logger.info("Starting Azure SQL sync")
            success, message, records_synced = sync_rates_to_azure_sql(
                quote_currency="NOK"
            )
            
            if success:
                logger.info(f"Azure SQL sync completed: {message}")
                ServiceLogService.add_log(
                    session, 'INFO',
                    f"Azure SQL sync completed: {records_synced} records synced"
                )
            else:
                logger.error(f"Azure SQL sync failed: {message}")
                ServiceLogService.add_log(
                    session, 'ERROR',
                    f"Azure SQL sync failed: {message}"
                )
            
            session.commit()
                
        except Exception as e:
            logger.error(f"Azure SQL sync error: {str(e)}", exc_info=True)
            ServiceLogService.add_log(
                session, 'ERROR',
                f"Azure SQL sync error: {str(e)}"
            )
            session.commit()
        finally:
            session.close()

    def backfill_bigquery_history(self):
        """
        One-time (idempotent) backfill of historical rates into BigQuery.

        Fetches all available Norges Bank rates from 2023-01-01 to today in
        6-month chunks and appends them to the Historyrates table.  Records
        already present are skipped automatically by sync_historical_rates_to_bigquery.
        """
        if not BIGQUERY_AVAILABLE:
            logger.warning("BigQuery not available – skipping history backfill")
            return

        logger.info("Starting BigQuery history backfill from 2023-01-01")

        from datetime import date, timedelta as td
        from bigquery_sync import sync_historical_rates_to_bigquery

        start = date(2023, 1, 1)
        today = date.today()
        chunk_days = 180  # 6-month chunks to stay within API limits

        chunk_start = start
        total_synced = 0

        while chunk_start <= today:
            chunk_end = min(chunk_start + td(days=chunk_days - 1), today)
            logger.info(f"Backfill chunk: {chunk_start} → {chunk_end}")

            try:
                nb_rates = self.norges_bank.get_historical_rates(
                    start_date=chunk_start,
                    end_date=chunk_end,
                )

                # Convert NorgesBank response to BigQuery-ready dicts
                # NorgesBank returns: {date, base, quote (always NOK), value, src}
                bq_rates = [
                    {
                        "currency_code": r["base"],
                        "rate": r["value"],
                        "date": r["date"],
                    }
                    for r in nb_rates
                    if r.get("quote") == "NOK"
                ]

                if bq_rates:
                    ok = sync_historical_rates_to_bigquery(
                        bq_rates,
                        source="norges_bank",
                        quote_currency="NOK",
                    )
                    if ok:
                        total_synced += len(bq_rates)
                    else:
                        logger.warning(f"Chunk {chunk_start}→{chunk_end} sync returned False")

            except Exception as exc:
                logger.error(f"Backfill chunk {chunk_start}→{chunk_end} failed: {exc}")

            chunk_start = chunk_end + td(days=1)

        logger.info(f"BigQuery history backfill complete – {total_synced} rows processed")

    def start(self):
        """Start the scheduler"""
        if not self.is_running:
            # Schedule PostgreSQL sync Mon-Fri at 16:10 (after Norges Bank's 16:00 update)
            self.scheduler.add_job(
                self.sync_rates,
                CronTrigger(day_of_week='mon-fri', hour=16, minute=10),
                id='daily_sync',
                name='Daily Currency Rate Sync (Mon-Fri 16:10)',
                replace_existing=True
            )
            
            # Schedule BigQuery sync Mon-Fri at 16:30 (after PostgreSQL sync)
            if BIGQUERY_AVAILABLE:
                self.scheduler.add_job(
                    self.sync_to_bigquery,
                    CronTrigger(day_of_week='mon-fri', hour=16, minute=30),
                    id='bigquery_sync',
                    name='BigQuery Sync (Mon-Fri 16:30)',
                    replace_existing=True
                )
                logger.info("BigQuery sync scheduled for Mon-Fri at 16:30")
            else:
                logger.warning("BigQuery sync NOT scheduled - BIGQUERY_AVAILABLE is False")
            
            # Schedule Azure SQL sync Mon-Fri at 16:35 (after BigQuery sync)
            if AZURE_SQL_AVAILABLE:
                self.scheduler.add_job(
                    self.sync_to_azure_sql,
                    CronTrigger(day_of_week='mon-fri', hour=16, minute=35),
                    id='azure_sql_sync',
                    name='Azure SQL Sync (Mon-Fri 16:35)',
                    replace_existing=True
                )
                logger.info("Azure SQL sync scheduled for Mon-Fri at 16:35")
            else:
                logger.warning("Azure SQL sync NOT scheduled - AZURE_SQL_AVAILABLE is False")
            
            # Schedule Power BI sync Mon-Fri at 17:00 (after PostgreSQL sync)
            if POWERBI_AVAILABLE:
                self.scheduler.add_job(
                    self.sync_to_powerbi,
                    CronTrigger(day_of_week='mon-fri', hour=17, minute=0),
                    id='powerbi_sync',
                    name='Power BI Sync (Mon-Fri 17:00)',
                    replace_existing=True
                )
                logger.info("Power BI sync scheduled for Mon-Fri at 17:00")
            else:
                logger.warning("Power BI sync NOT scheduled - POWERBI_AVAILABLE is False")
            
            # Run initial PostgreSQL sync immediately
            self.scheduler.add_job(
                self.sync_rates,
                'date',
                run_date=datetime.now(),
                id='initial_sync',
                name='Initial Currency Rate Sync'
            )

            # Run BigQuery history backfill 30 s after startup (idempotent – skips
            # records already present, so safe to run on every restart)
            if BIGQUERY_AVAILABLE:
                from datetime import timedelta
                self.scheduler.add_job(
                    self.backfill_bigquery_history,
                    'date',
                    run_date=datetime.now() + timedelta(seconds=30),
                    id='bigquery_backfill',
                    name='BigQuery History Backfill (2023-01-01 → today)'
                )
                logger.info("BigQuery history backfill scheduled to run in 30 s")

            self.scheduler.start()
            self.is_running = True
            logger.info("Currency scheduler started")
    
    def stop(self):
        """Stop the scheduler"""
        if self.is_running:
            self.scheduler.shutdown()
            self.is_running = False
            logger.info("Currency scheduler stopped")
    
    def get_jobs(self):
        """Get all scheduled jobs"""
        return self.scheduler.get_jobs()
    
    def get_next_run_time(self, job_id: str):
        """Get next run time for a job"""
        job = self.scheduler.get_job(job_id)
        return job.next_run_time if job else None
