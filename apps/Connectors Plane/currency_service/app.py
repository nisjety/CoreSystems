"""
Currency Integration Microservice

Complete backend with:
- Full CRUD operations for exchange rates
- Database persistence (PostgreSQL)
- Scheduled cron jobs for automatic rate synchronization
- Multiple data sources (Norges Bank, FCS API)
- Comprehensive logging and monitoring

Port: 5004
"""

import logging
import os
from datetime import datetime, timedelta, timezone
from typing import Optional

from flask import Flask, jsonify, request
from flask_cors import CORS

from cache import RateCache
from config import config
from converters import CurrencyConverter
from sources import FCSAPISource, NorgesBankSource
from models import init_db, get_session, ExchangeRate, SyncLog, ServiceLog
from crud import CurrencyRateService, SyncLogService, ServiceLogService
from scheduler import CurrencyScheduler

# Configure logging
logging.basicConfig(
    level=getattr(logging, config.LOG_LEVEL),
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)

# Initialize Flask app
app = Flask(__name__)
app.secret_key = os.getenv("FLASK_SECRET_KEY", os.urandom(24).hex())

CORS(app, resources={
    r"/api/*": {
        "origins": ["http://localhost:3000", "http://localhost:3001", "http://localhost:8080"],
        "supports_credentials": True,
        "allow_headers": ["Content-Type", "Authorization"],
        "methods": ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
    }
})

# Initialize components
rate_cache = RateCache()
converter = CurrencyConverter(rate_cache)
norges_bank = NorgesBankSource()
fcs_api = FCSAPISource()

# Initialize database
try:
    Session = init_db()
    logger.info("Database initialized successfully")
except Exception as e:
    logger.error(f"Failed to initialize database: {e}")
    Session = None

# Initialize scheduler
scheduler = CurrencyScheduler(norges_bank, fcs_api)


# =================================================================
# HEALTH CHECK
# =================================================================


@app.route("/health", methods=["GET"])
def health():
    """Health check endpoint."""
    db_status = "healthy"
    try:
        from sqlalchemy import text
        session = get_session()
        session.execute(text("SELECT 1"))
        session.close()
    except Exception as e:
        db_status = f"unhealthy: {str(e)}"
    
    return jsonify({
        "status": "healthy",
        "service": "currency_service",
        "version": "2.0.0",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "database": db_status,
        "scheduler": "running" if scheduler.is_running else "stopped",
        "sources": {
            "norges_bank": "available",
            "fcs_api": "available" if fcs_api.api_key else "disabled"
        }
    })


# =================================================================
# CRUD ENDPOINTS FOR EXCHANGE RATES
# =================================================================


@app.route("/api/currency/rates", methods=["GET"])
def list_rates():
    """
    List all exchange rates with optional filters.
    
    Query params:
    - from_currency: Filter by source currency
    - source: Filter by source (norgesbank, abstract, manual)
    - latest: If true, return only latest rate for each pair
    """
    try:
        session = get_session()
        from_currency = request.args.get('from_currency', '').upper()
        source = request.args.get('source', '')
        latest = request.args.get('latest', 'false').lower() == 'true'
        
        if latest:
            rates = CurrencyRateService.get_latest_rates(session)
        else:
            rates = CurrencyRateService.get_all_rates(
                session,
                from_currency=from_currency if from_currency else None,
                source=source if source else None
            )
        
        session.close()
        
        return jsonify({
            "success": True,
            "rates": [rate.to_dict() for rate in rates],
            "count": len(rates),
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        logger.error(f"Failed to list rates: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates/<int:rate_id>", methods=["GET"])
def get_rate(rate_id: int):
    """Get a specific exchange rate by ID."""
    try:
        session = get_session()
        rate = CurrencyRateService.get_rate(session, rate_id)
        
        if not rate:
            session.close()
            return jsonify({"error": "Rate not found"}), 404
        
        rate_dict = rate.to_dict()
        session.close()
        
        return jsonify({
            "success": True,
            "rate": rate_dict
        })
        
    except Exception as e:
        logger.error(f"Failed to get rate: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates", methods=["POST"])
def create_rate():
    """
    Create a new exchange rate.
    
    Body:
    {
        "from_currency": "USD",
        "to_currency": "NOK",
        "rate": 10.5,
        "source": "manual"
    }
    """
    try:
        data = request.get_json()
        from_currency = data.get('from_currency', '').upper()
        to_currency = data.get('to_currency', '').upper()
        rate = float(data.get('rate', 0))
        source = data.get('source', 'manual')
        
        if not all([from_currency, to_currency, rate]):
            return jsonify({"error": "Missing required fields"}), 400
        
        session = get_session()
        new_rate = CurrencyRateService.create_rate(
            session, from_currency, to_currency, rate, source
        )
        
        # Convert to dict before closing session
        rate_dict = new_rate.to_dict()
        
        # Log the action
        ServiceLogService.add_log(
            session, 'INFO',
            f"Created rate: {from_currency} -> {to_currency} = {rate}"
        )
        
        session.close()
        
        return jsonify({
            "success": True,
            "rate": rate_dict
        }), 201
        
    except Exception as e:
        logger.error(f"Failed to create rate: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates/<int:rate_id>", methods=["PUT"])
def update_rate(rate_id: int):
    """
    Update an existing exchange rate.
    
    Body:
    {
        "rate": 10.75
    }
    """
    try:
        data = request.get_json()
        rate = float(data.get('rate', 0))
        
        if not rate:
            return jsonify({"error": "Rate value is required"}), 400
        
        session = get_session()
        updated_rate = CurrencyRateService.update_rate(session, rate_id, rate)
        
        if not updated_rate:
            session.close()
            return jsonify({"error": "Rate not found"}), 404
        
        rate_dict = updated_rate.to_dict()
        
        # Log the action
        ServiceLogService.add_log(
            session, 'INFO',
            f"Updated rate {rate_id}: {updated_rate.from_currency} -> {updated_rate.to_currency} = {rate}"
        )
        
        session.close()
        
        return jsonify({
            "success": True,
            "rate": rate_dict
        })
        
    except Exception as e:
        logger.error(f"Failed to update rate: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates/<int:rate_id>", methods=["DELETE"])
def delete_rate(rate_id: int):
    """Delete an exchange rate."""
    try:
        session = get_session()
        
        # Get rate info before deleting for logging
        rate = CurrencyRateService.get_rate(session, rate_id)
        if not rate:
            session.close()
            return jsonify({"error": "Rate not found"}), 404
        
        rate_info = f"{rate.from_currency} -> {rate.to_currency}"
        
        # Delete the rate
        success = CurrencyRateService.delete_rate(session, rate_id)
        
        if success:
            # Log the action
            ServiceLogService.add_log(
                session, 'INFO',
                f"Deleted rate {rate_id}: {rate_info}"
            )
        
        session.close()
        
        return jsonify({
            "success": success,
            "message": "Rate deleted successfully" if success else "Failed to delete rate"
        })
        
    except Exception as e:
        logger.error(f"Failed to delete rate: {e}")
        return jsonify({"error": str(e)}), 500


# =================================================================
# CONVERSION ENDPOINT
# =================================================================


@app.route("/api/currency/convert", methods=["GET"])
def convert_currency():
    """
    Convert amount between currencies.
    
    Query params:
    - from: Source currency (required)
    - to: Target currency (required)
    - amount: Amount to convert (required)
    """
    try:
        from_currency = request.args.get('from', '').upper()
        to_currency = request.args.get('to', '').upper()
        amount_str = request.args.get('amount', '')
        
        if not all([from_currency, to_currency, amount_str]):
            return jsonify({
                "error": "Missing required parameters",
                "required": ["from", "to", "amount"]
            }), 400
        
        try:
            amount = float(amount_str)
        except ValueError:
            return jsonify({"error": "Invalid amount value"}), 400
        
        # Get rates from database
        session = get_session()
        
        # Try direct conversion
        direct_rate = CurrencyRateService.get_rate_by_pair(
            session, from_currency, to_currency
        )
        
        if direct_rate:
            result_amount = amount * direct_rate.rate
            session.close()
            
            return jsonify({
                "from": from_currency,
                "to": to_currency,
                "amount": amount,
                "result": result_amount,
                "rate": direct_rate.rate,
                "timestamp": direct_rate.timestamp.isoformat()
            })
        
        # Try inverse rate (if we have TO→FROM, we can calculate FROM→TO)
        inverse_rate = CurrencyRateService.get_rate_by_pair(session, to_currency, from_currency)
        if inverse_rate:
            result_amount = amount / inverse_rate.rate
            calculated_rate = 1 / inverse_rate.rate
            session.close()
            
            return jsonify({
                "from": from_currency,
                "to": to_currency,
                "amount": amount,
                "result": result_amount,
                "rate": calculated_rate,
                "method": "inverse_rate",
                "timestamp": inverse_rate.timestamp.isoformat()
            })
        
        # Try cross-rate via NOK
        # Check if from_currency is NOK (NOK → currency)
        if from_currency == 'NOK':
            # We need currency/NOK rate to calculate NOK/currency
            currency_to_nok = CurrencyRateService.get_rate_by_pair(session, to_currency, 'NOK')
            if currency_to_nok:
                # 1 NOK = 1/rate currency
                result_amount = amount / currency_to_nok.rate
                calculated_rate = 1 / currency_to_nok.rate
                session.close()
                
                return jsonify({
                    "from": from_currency,
                    "to": to_currency,
                    "amount": amount,
                    "result": result_amount,
                    "rate": calculated_rate,
                    "method": "nok_to_currency",
                    "timestamp": currency_to_nok.timestamp.isoformat()
                })
        
        # Check if to_currency is NOK (currency → NOK)
        if to_currency == 'NOK':
            currency_to_nok = CurrencyRateService.get_rate_by_pair(session, from_currency, 'NOK')
            if currency_to_nok:
                result_amount = amount * currency_to_nok.rate
                session.close()
                
                return jsonify({
                    "from": from_currency,
                    "to": to_currency,
                    "amount": amount,
                    "result": result_amount,
                    "rate": currency_to_nok.rate,
                    "method": "currency_to_nok",
                    "timestamp": currency_to_nok.timestamp.isoformat()
                })
        
        # Try cross-rate for non-NOK pairs (e.g., USD → EUR)
        from_to_nok = CurrencyRateService.get_rate_by_pair(session, from_currency, 'NOK')
        to_to_nok = CurrencyRateService.get_rate_by_pair(session, to_currency, 'NOK')
        
        if from_to_nok and to_to_nok:
            # Convert: amount → NOK → target
            # 1 USD = 10 NOK, 1 EUR = 11 NOK
            # 1 USD = 10/11 EUR
            cross_rate = from_to_nok.rate / to_to_nok.rate
            result_amount = amount * cross_rate
            
            session.close()
            
            return jsonify({
                "from": from_currency,
                "to": to_currency,
                "amount": amount,
                "result": result_amount,
                "rate": cross_rate,
                "method": "cross_rate_via_NOK",
                "timestamp": datetime.utcnow().isoformat()
            })
        
        session.close()
        return jsonify({"error": "Exchange rate not available"}), 404
        
    except Exception as e:
        logger.error(f"Conversion failed: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/search", methods=["GET"])
def search_currency():
    """
    Search for a currency by querying APIs directly (not database).
    Checks Norges Bank first, then FCS API.
    
    Query params:
        symbol: Currency code to search (e.g., JPY, BRL, GBP)
    
    Returns:
        {
            "symbol": "JPY",
            "rate": 0.064764,
            "source": "norgesbank",
            "found": true
        }
    """
    try:
        symbol = request.args.get('symbol', '').upper().strip()
        
        if not symbol or len(symbol) < 2:
            return jsonify({"error": "Please provide a valid currency symbol"}), 400
        
        logger.info(f"Searching for currency: {symbol}")
        
        # First, try Norges Bank
        try:
            logger.info(f"Checking Norges Bank for {symbol}")
            nb_rates = norges_bank.get_latest_rates(currencies=[symbol])
            
            if nb_rates and len(nb_rates) > 0:
                rate = nb_rates[0]
                logger.info(f"Found {symbol} in Norges Bank: {rate['value']}")
                return jsonify({
                    "symbol": symbol,
                    "rate": rate['value'],
                    "source": "norgesbank",
                    "found": True,
                    "pair": f"{symbol}/NOK"
                })
        except Exception as e:
            logger.warning(f"Norges Bank search failed for {symbol}: {e}")
        
        # If not found in Norges Bank, try FCS API
        if fcs_api.api_key:
            try:
                logger.info(f"Checking FCS API for {symbol}")
                fcs_rates = fcs_api.get_latest_rates(base='NOK', targets=[symbol])
                
                if fcs_rates and len(fcs_rates) > 0:
                    rate = fcs_rates[0]
                    logger.info(f"Found {symbol} in FCS API: {rate['value']}")
                    return jsonify({
                        "symbol": symbol,
                        "rate": rate['value'],
                        "source": "fcs",
                        "found": True,
                        "pair": f"{symbol}/NOK"
                    })
            except Exception as e:
                logger.warning(f"FCS API search failed for {symbol}: {e}")
        
        # Currency not found in any source
        logger.info(f"Currency {symbol} not found in any source")
        return jsonify({
            "symbol": symbol,
            "found": False,
            "error": "Currency not found in Norges Bank or FCS API"
        }), 404
        
    except Exception as e:
        logger.error(f"Search failed: {e}")
        return jsonify({"error": str(e)}), 500


# =================================================================
# SYNC & SCHEDULING ENDPOINTS
# =================================================================


@app.route("/api/currency/sync", methods=["POST"])
def manual_sync():
    """Manually trigger rate synchronization."""
    try:
        session = get_session()
        sync_log = SyncLogService.create_log(session, 'manual')
        sync_log_id = sync_log.id  # Store the ID before closing session
        
        ServiceLogService.add_log(session, 'INFO', "Manual sync triggered")
        session.close()
        
        rates_updated = 0
        errors = []
        
        # Sync from Norges Bank
        try:
            session = get_session()
            nb_rates = norges_bank.get_latest_rates(currencies=config.NORGES_BANK_CURRENCIES)
            
            for rate in nb_rates:
                from_curr = rate['base'] if rate['quote'] == 'NOK' else 'NOK'
                to_curr = rate['quote'] if rate['quote'] == 'NOK' else rate['base']
                
                CurrencyRateService.upsert_rate(
                    session, from_curr, to_curr, rate['value'], 'norgesbank'
                )
                rates_updated += 1
            
            ServiceLogService.add_log(
                session, 'INFO', f"Synced {len(nb_rates)} rates from Norges Bank"
            )
            session.close()
            
        except Exception as e:
            error_msg = f"Norges Bank sync failed: {str(e)}"
            errors.append(error_msg)
            logger.error(error_msg)
            session = get_session()
            ServiceLogService.add_log(session, 'ERROR', error_msg)
            session.close()
        
        # Sync from FCS API
        if fcs_api.api_key:
            try:
                session = get_session()
                fcs_rates = fcs_api.get_latest_rates(
                    base='NOK', targets=config.FCS_CURRENCIES
                )
                
                for rate in fcs_rates:
                    from_curr = rate['base'] if rate['quote'] == 'NOK' else 'NOK'
                    to_curr = rate['quote'] if rate['quote'] == 'NOK' else rate['base']
                    
                    CurrencyRateService.upsert_rate(
                        session, from_curr, to_curr, rate['value'], 'fcs'
                    )
                    rates_updated += 1
                
                ServiceLogService.add_log(
                    session, 'INFO', f"Synced {len(fcs_rates)} rates from FCS API"
                )
                session.close()
                
            except Exception as e:
                error_msg = f"FCS API sync failed: {str(e)}"
                errors.append(error_msg)
                logger.error(error_msg)
                session = get_session()
                ServiceLogService.add_log(session, 'ERROR', error_msg)
                session.close()
        
        # Complete sync log
        session = get_session()
        status = 'success' if not errors else ('partial' if rates_updated > 0 else 'failed')
        SyncLogService.complete_log(
            session, sync_log_id, status, rates_updated,
            '; '.join(errors) if errors else None
        )
        
        ServiceLogService.add_log(
            session, 'INFO', f"Sync completed: {rates_updated} rates updated"
        )
        session.close()
        
        return jsonify({
            "success": True,
            "updated_count": rates_updated,
            "status": status,
            "errors": errors if errors else None,
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        logger.error(f"Manual sync failed: {e}")
        return jsonify({"error": str(e)}), 500


# =================================================================
# STATUS & MONITORING ENDPOINTS
# =================================================================


@app.route("/api/currency/status", methods=["GET"])
def get_service_status():
    """Get comprehensive service status."""
    try:
        session = get_session()
        
        # Get recent sync logs
        recent_syncs = SyncLogService.get_recent_logs(session, limit=10)
        last_sync = SyncLogService.get_last_successful_sync(session)
        
        # Get recent service logs
        recent_logs = ServiceLogService.get_recent_logs(session, limit=50)
        
        session.close()
        
        # Check service health
        services = []
        
        # Norges Bank status
        try:
            nb_rates = norges_bank.get_latest_rates(currencies=["USD"])
            services.append({
                "service": "Norges Bank",
                "status": "healthy",
                "last_check": datetime.utcnow().isoformat(),
                "response_time_ms": 100
            })
        except Exception as e:
            services.append({
                "service": "Norges Bank",
                "status": "down",
                "last_check": datetime.utcnow().isoformat(),
                "error": str(e)
            })
        
        # FCS API status
        if fcs_api.api_key:
            try:
                fcs_rates = fcs_api.get_latest_rates(base="NOK", targets=["CLP"])
                services.append({
                    "service": "FCS API",
                    "status": "healthy",
                    "last_check": datetime.utcnow().isoformat(),
                    "response_time_ms": 150
                })
            except Exception as e:
                services.append({
                    "service": "FCS API",
                    "status": "down",
                    "last_check": datetime.utcnow().isoformat(),
                    "error": str(e)
                })
        
        # Build cron jobs status
        jobs = scheduler.get_jobs()
        cron_jobs = []
        
        for job in jobs:
            next_run = job.next_run_time.isoformat() if job.next_run_time else None
            
            cron_jobs.append({
                "name": job.name,
                "schedule": str(job.trigger) if hasattr(job, 'trigger') else "On demand",
                "last_run": last_sync.completed_at.isoformat() if last_sync else None,
                "next_run": next_run,
                "status": "active" if scheduler.is_running else "idle",
                "last_error": last_sync.errors if last_sync and last_sync.errors else None
            })
        
        # Sync history
        sync_history = [log.to_dict() for log in recent_syncs]
        
        return jsonify({
            "success": True,
            "cron_jobs": cron_jobs,
            "services": services,
            "logs": [log.to_dict() for log in recent_logs],
            "sync_history": sync_history,
            "scheduler_running": scheduler.is_running,
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        logger.error(f"Failed to get service status: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/logs", methods=["GET"])
def get_logs():
    """Get service logs with optional filtering."""
    try:
        level = request.args.get('level', '').upper()
        limit = int(request.args.get('limit', 100))
        
        session = get_session()
        logs = ServiceLogService.get_recent_logs(
            session,
            limit=limit,
            level=level if level else None
        )
        session.close()
        
        return jsonify({
            "success": True,
            "logs": [log.to_dict() for log in logs],
            "count": len(logs)
        })
        
    except Exception as e:
        logger.error(f"Failed to get logs: {e}")
        return jsonify({"error": str(e)}), 500


# =================================================================
# SOURCES & CACHE MANAGEMENT
# =================================================================


@app.route("/api/currency/sources", methods=["GET"])
def get_sources():
    """List available currency data sources."""
    return jsonify({
        "sources": [
            {
                "id": "norgesbank",
                "name": "Norges Bank",
                "description": "Norwegian central bank exchange rates",
                "url": "https://data.norges-bank.no",
                "currencies": config.NORGES_BANK_CURRENCIES,
                "status": "active",
                "priority": 1
            },
            {
                "id": "fcs",
                "name": "FCS API",
                "description": "Forex Currency System API - 2000+ currency pairs",
                "url": "https://fcsapi.com",
                "currencies": config.FCS_CURRENCIES,
                "status": "active" if fcs_api.api_key else "disabled",
                "priority": 2
            }
        ],
        "total_currencies": len(set(
            config.NORGES_BANK_CURRENCIES + 
            config.FCS_CURRENCIES
        ))
    })


@app.route("/api/currency/cache/clear", methods=["POST"])
def clear_cache():
    """Clear rate cache."""
    count = rate_cache.clear()
    logger.info(f"Cache cleared: {count} entries removed")
    
    return jsonify({
        "success": True,
        "entries_removed": count,
        "timestamp": datetime.utcnow().isoformat()
    })


# =================================================================
# ERROR HANDLERS
# =================================================================


@app.route("/api/gcp/status", methods=["GET"])
def gcp_status():
    """
    Get GCP integration status.
    
    Returns BigQuery and Cloud Storage connection status,
    available tables, and last sync information.
    """
    try:
        # Try to import GCP client
        try:
            from shared.gcp_client import get_gcp_client
        except ImportError as ie:
            logger.warning(f"GCP client module not available: {ie}")
            return jsonify({
                "connected": False,
                "error": "GCP integration not configured",
                "project_id": os.getenv("GCP_PROJECT_ID"),
                "dataset": os.getenv("GCP_DATASET_ID")
            }), 503
        
        gcp = get_gcp_client()
        
        # Get project and dataset info
        project_id = os.getenv("GCP_PROJECT_ID")
        dataset_id = os.getenv("GCP_DATASET_ID")
        bucket_name = os.getenv("GCP_BUCKET_NAME")
        region = os.getenv("GCP_REGION")
        
        # Check BigQuery table
        bigquery_tables = {}
        try:
            table_ref = f"{project_id}.{dataset_id}.exchange_rates"
            bq_client = gcp.bigquery
            table = bq_client.get_table(table_ref)
            bigquery_tables["currency"] = True
            
            # Get row count and last update
            query = f"""
                SELECT 
                    COUNT(*) as total_rows,
                    MAX(created_at) as last_update
                FROM `{table_ref}`
            """
            results = gcp.query_bigquery(query)
            for row in results:
                bigquery_tables["total_rows"] = row.total_rows
                bigquery_tables["last_update"] = str(row.last_update) if row.last_update else None
        except Exception as e:
            logger.warning(f"Currency table not found: {e}")
            bigquery_tables["currency"] = False
        
        # Get last sync time from database
        last_sync = {}
        try:
            session = get_session()
            last_sync_log = session.query(SyncLog)\
                .filter(SyncLog.destination == "bigquery")\
                .order_by(SyncLog.sync_time.desc())\
                .first()
            
            if last_sync_log:
                last_sync["currency"] = last_sync_log.sync_time.isoformat()
            
            session.close()
        except Exception as e:
            logger.warning(f"Could not get last sync time: {e}")
        
        return jsonify({
            "connected": True,
            "project_id": project_id,
            "dataset": dataset_id,
            "bucket": bucket_name,
            "region": region,
            "bigquery_tables": bigquery_tables,
            "last_sync": last_sync
        })
        
    except Exception as e:
        import traceback
        logger.error(f"Error getting GCP status: {e}")
        logger.error(f"Traceback: {traceback.format_exc()}")
        return jsonify({
            "connected": False,
            "error": str(e),
            "project_id": os.getenv("GCP_PROJECT_ID"),
            "dataset": os.getenv("GCP_DATASET_ID"),
            "bucket": os.getenv("GCP_BUCKET_NAME"),
            "region": os.getenv("GCP_REGION")
        }), 200  # Return 200 even on error so frontend can show status


@app.route("/api/currency/bigquery/data", methods=["GET"])
def get_bigquery_data():
    """
    Query currency data from BigQuery.
    
    Returns exchange rate data stored in BigQuery data warehouse.
    """
    try:
        # Try to import BigQuery sync module
        try:
            from bigquery_sync import get_latest_rates_from_bigquery
        except ImportError as ie:
            logger.warning(f"BigQuery sync module not available: {ie}")
            return jsonify({
                "success": False,
                "error": "BigQuery integration not configured",
                "records": [],
                "count": 0
            }), 503
        
        # Get query parameters
        currency_codes = request.args.get('currencies', '').split(',') if request.args.get('currencies') else None
        limit = int(request.args.get('limit', 100))
        
        # Query BigQuery
        rates = get_latest_rates_from_bigquery(currency_codes=currency_codes, limit=limit)
        
        return jsonify({
            "success": True,
            "records": rates,
            "count": len(rates),
            "total": len(rates)
        })
        
    except Exception as e:
        logger.error(f"Error querying BigQuery data: {e}")
        return jsonify({
            "success": False,
            "error": str(e),
            "records": [],
            "count": 0
        }), 500


@app.route("/api/currency/sync/gcp", methods=["POST"])
def sync_to_gcp():
    """
    Sync exchange rates to BigQuery (GCP).
    Syncs to both Rate (current) and Historyrates (past 3 years) tables.
    """
    try:
        # Try to import BigQuery sync module
        try:
            from bigquery_sync import (
                sync_current_rates_to_bigquery, 
                sync_historical_rates_to_bigquery,
                ensure_tables_exist
            )
        except ImportError as ie:
            logger.warning(f"BigQuery sync module not available: {ie}")
            return jsonify({
                "success": False,
                "error": "BigQuery integration not configured"
            }), 503
        
        # Ensure tables exist
        if not ensure_tables_exist():
            return jsonify({
                "success": False,
                "error": "Failed to ensure BigQuery tables exist"
            }), 500
        
        # Get rates from database
        session = get_session()
        
        from datetime import timedelta
        from sqlalchemy import func
        
        # 1. Get LATEST rates for Rate table (one per currency, NOK pairs only)
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
                "date": rate.timestamp.date().isoformat()
            })

        # 2. Get ALL rates for past 3 years for Historyrates table (NOK pairs only)
        cutoff_date = datetime.now() - timedelta(days=365 * 3)
        historical_rates_query = session.query(ExchangeRate).filter(
            ExchangeRate.to_currency == 'NOK',
            ExchangeRate.timestamp >= cutoff_date
        ).order_by(ExchangeRate.timestamp.desc()).all()

        historical_rates = []
        for rate in historical_rates_query:
            historical_rates.append({
                "currency_code": rate.from_currency,
                "rate": rate.rate,
                "date": rate.timestamp.date().isoformat()
            })
        
        session.close()
        
        # Sync to both tables
        current_success = sync_current_rates_to_bigquery(
            current_rates, 
            source="currency_service", 
            quote_currency="NOK"
        )
        
        historical_success = sync_historical_rates_to_bigquery(
            historical_rates,
            source="currency_service",
            quote_currency="NOK",
        )
        
        if current_success and historical_success:
            return jsonify({
                "success": True,
                "message": f"Successfully synced to BigQuery",
                "current_rates": len(current_rates),
                "historical_rates": len(historical_rates),
                "tables": {
                    "Rate": f"{len(current_rates)} latest rates",
                    "Historyrates": f"{len(historical_rates)} rates (past 3 years)"
                }
            })
        else:
            errors = []
            if not current_success:
                errors.append("Rate table sync failed")
            if not historical_success:
                errors.append("Historyrates table sync failed")
            
            return jsonify({
                "success": False,
                "error": "; ".join(errors),
                "partial_success": current_success or historical_success
            }), 500
            
    except Exception as e:
        logger.error(f"Error syncing to GCP: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


# =================================================================
# POWER BI INTEGRATION ENDPOINTS
# =================================================================


@app.route("/api/currency/powerbi/status", methods=["GET"])
def powerbi_status():
    """
    Get Power BI sync status and database information.
    
    Returns:
    - Total records in Power BI database
    - Last sync information
    - Date range of available data
    - Currency count
    """
    try:
        from powerbi_sync import PowerBISyncService
        
        status = PowerBISyncService.get_sync_status()
        
        return jsonify({
            "success": True,
            "powerbi": status
        })
        
    except ImportError:
        return jsonify({
            "success": False,
            "error": "Power BI integration not configured"
        }), 503
    except Exception as e:
        logger.error(f"Error getting Power BI status: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


@app.route("/api/currency/powerbi/sync", methods=["POST"])
def powerbi_sync():
    """
    Manually trigger Power BI sync.
    
    Body (optional):
    {
        "days_back": 90  // Number of days to sync (default: 90)
    }
    """
    try:
        from powerbi_sync import PowerBISyncService
        
        data = request.get_json() or {}
        days_back = int(data.get('days_back', 90))
        
        success, message, records_synced = PowerBISyncService.sync_to_powerbi(
            sync_type='manual',
            days_back=days_back
        )
        
        if success:
            return jsonify({
                "success": True,
                "message": message,
                "records_synced": records_synced
            })
        else:
            return jsonify({
                "success": False,
                "error": message
            }), 500
            
    except ImportError:
        return jsonify({
            "success": False,
            "error": "Power BI integration not configured"
        }), 503
    except Exception as e:
        logger.error(f"Error syncing to Power BI: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


@app.route("/api/currency/powerbi/history", methods=["GET"])
def powerbi_history():
    """
    Get Power BI sync history.
    
    Query params:
    - limit: Number of recent syncs to return (default: 10)
    """
    try:
        from powerbi_sync import PowerBISyncService
        
        limit = int(request.args.get('limit', 10))
        history = PowerBISyncService.get_sync_history(limit=limit)
        
        return jsonify({
            "success": True,
            "history": history,
            "count": len(history)
        })
        
    except ImportError:
        return jsonify({
            "success": False,
            "error": "Power BI integration not configured"
        }), 503
    except Exception as e:
        logger.error(f"Error getting Power BI history: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


@app.route("/api/currency/powerbi/connection", methods=["GET"])
def powerbi_connection():
    """
    Get Power BI connection information.
    
    Returns connection string details for connecting Power BI to SQL Server.
    """
    try:
        from powerbi_models import get_powerbi_connection_string
        
        connection_info = get_powerbi_connection_string()
        
        return jsonify({
            "success": True,
            "connection": connection_info,
            "available_views": [
                "DataSammendrag",
                "Valutakurser",
                "SisteKurser",
                "HistoricalRate"
            ],
            "instructions": {
                "step1": "Open Power BI Desktop",
                "step2": "Click 'Get Data' -> 'SQL Server'",
                "step3": f"Server: {connection_info['server']}",
                "step4": f"Database: {connection_info['database']}",
                "step5": "Authentication: SQL Server (use provided credentials)",
                "step6": "Select one of the available views",
                "step7": "Load the data and create your visualizations"
            }
        })
        
    except ImportError:
        return jsonify({
            "success": False,
            "error": "Power BI integration not configured"
        }), 503
    except Exception as e:
        logger.error(f"Error getting Power BI connection info: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


@app.errorhandler(404)
def not_found(error):
    """Handle 404 errors."""
    return jsonify({"error": "Endpoint not found"}), 404


@app.errorhandler(500)
def internal_error(error):
    """Handle 500 errors."""
    logger.error(f"Internal server error: {error}")
    return jsonify({"error": "Internal server error"}), 500


# =================================================================
# MAIN
# =================================================================


if __name__ == "__main__":
    logger.info(f"Starting Currency service on port {config.port}")
    logger.info(f"Supported currencies: {len(set(config.NORGES_BANK_CURRENCIES + config.ABSTRACT_CURRENCIES))}")
    
    # Validate configuration
    config.validate()
    
    # Start scheduler
    scheduler.start()
    logger.info("Scheduler started successfully")
    
    # Run Flask app
    try:
        app.run(
            host=config.host,
            port=config.port,
            debug=config.DEBUG,
        )
    finally:
        scheduler.stop()
        logger.info("Currency service stopped")
