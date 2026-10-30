"""
Currency Integration Microservice

Aggregates currency exchange rates from multiple sources:
- Norges Bank (Norwegian central bank) - Primary source for major currencies
- Abstract API (Commercial) - For currencies not available from central banks (e.g., CLP)

Port: 5004
"""

import logging
import os
from datetime import datetime, timedelta
from typing import Optional, List, Dict

from flask import Flask, jsonify, request
from flask_cors import CORS

from cache import RateCache
from config import config
from converters import CurrencyConverter
from sources import AbstractAPISource, NorgesBankSource

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
        "methods": ["GET", "POST", "OPTIONS"]
    }
})

# Initialize components
rate_cache = RateCache()
converter = CurrencyConverter(rate_cache)
norges_bank = NorgesBankSource()
abstract_api = AbstractAPISource()

# Storage for manual rates and logs
manual_rates: Dict[str, Dict] = {}
log_entries: List[Dict] = []
cron_status = {
    "last_run": None,
    "next_run": None,
    "status": "idle",
    "last_error": None
}


def add_log(level: str, message: str, details=None):
    """Add entry to in-memory log."""
    log_entries.append({
        "timestamp": datetime.utcnow().isoformat(),
        "level": level,
        "message": message,
        "details": details
    })
    # Keep only last 100 logs
    if len(log_entries) > 100:
        log_entries.pop(0)
    
    # Also log to system logger
    log_func = getattr(logger, level.lower(), logger.info)
    log_func(message)


# =================================================================
# HEALTH CHECK
# =================================================================


@app.route("/health", methods=["GET"])
def health():
    """Health check endpoint."""
    return jsonify({
        "status": "healthy",
        "service": "currency_service",
        "version": "1.0.0",
        "timestamp": datetime.utcnow().isoformat(),
        "sources": {
            "norges_bank": "available",
            "abstract_api": "available" if abstract_api.api_key else "disabled"
        }
    })


# =================================================================
# RATES ENDPOINTS
# =================================================================


@app.route("/api/currency/convert", methods=["GET"])
def convert_currency():
    """
    Convert amount between currencies.
    
    Query params:
    - from: Source currency (required)
    - to: Target currency (required)
    - amount: Amount to convert (required)
    - date: Date for rate (YYYY-MM-DD, default: latest)
    """
    from_currency = request.args.get('from', '').upper()
    to_currency = request.args.get('to', '').upper()
    amount_str = request.args.get('amount', '')
    date_str = request.args.get('date')
    
    # Validate parameters
    if not all([from_currency, to_currency, amount_str]):
        return jsonify({
            "error": "Missing required parameters",
            "required": ["from", "to", "amount"]
        }), 400
    
    try:
        amount = float(amount_str)
    except ValueError:
        return jsonify({
            "error": "Invalid amount value"
        }), 400
    
    try:
        # Get rates for both currencies
        currencies = [from_currency, to_currency]
        
        # Fetch rates
        all_rates = []
        
        # From Norges Bank
        nb_currencies = [c for c in currencies if c in config.NORGES_BANK_CURRENCIES]
        if nb_currencies:
            nb_rates = norges_bank.get_latest_rates(currencies=nb_currencies)
            all_rates.extend(nb_rates)
        
        # From Abstract API
        abstract_currencies = [c for c in currencies if c in config.ABSTRACT_CURRENCIES]
        if abstract_currencies:
            abstract_rates = abstract_api.get_latest_rates(base="NOK", targets=abstract_currencies)
            all_rates.extend(abstract_rates)
        
        # Build rates dictionary
        rates = {}
        for rate in all_rates:
            currency = rate['base'] if rate['quote'] == 'NOK' else rate['quote']
            rates[currency] = rate['value']
        
        # Perform conversion
        result = converter.convert(amount, from_currency, to_currency, rates)
        result["date"] = date_str or datetime.now().date().isoformat()
        result["timestamp"] = datetime.utcnow().isoformat()
        
        return jsonify(result)
        
    except ValueError as e:
        return jsonify({
            "error": str(e)
        }), 400
    except Exception as e:
        logger.error(f"Conversion failed: {e}")
        return jsonify({
            "error": "Currency conversion failed",
            "detail": str(e)
        }), 500


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
                "id": "abstract",
                "name": "Abstract API",
                "description": "Commercial exchange rate API",
                "url": "https://www.abstractapi.com/exchange-rate-api",
                "currencies": config.ABSTRACT_CURRENCIES,
                "status": "active" if abstract_api.api_key else "disabled",
                "priority": 2
            }
        ],
        "total_currencies": len(set(
            config.NORGES_BANK_CURRENCIES + 
            config.ABSTRACT_CURRENCIES
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


@app.route("/api/currency/cache/clear-expired", methods=["POST"])
def clear_expired_cache():
    """Remove expired cache entries."""
    count = rate_cache.clear_expired()
    
    return jsonify({
        "success": True,
        "expired_removed": count,
        "timestamp": datetime.utcnow().isoformat()
    })


# =================================================================
# RATE MANAGEMENT ENDPOINTS
# =================================================================


@app.route("/api/currency/rates", methods=["POST"])
def add_manual_rate():
    """
    Add a manual exchange rate.
    
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
        
        # Store manual rate
        rate_key = f"{from_currency}_{to_currency}"
        manual_rates[rate_key] = {
            "from_currency": from_currency,
            "to_currency": to_currency,
            "rate": rate,
            "source": source,
            "timestamp": datetime.utcnow().isoformat()
        }
        
        add_log("INFO", f"Added manual rate: {from_currency} -> {to_currency} = {rate}")
        
        return jsonify({
            "success": True,
            "rate": manual_rates[rate_key]
        })
        
    except Exception as e:
        add_log("ERROR", f"Failed to add manual rate: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates", methods=["PUT"])
def update_manual_rate():
    """Update an existing manual rate."""
    try:
        data = request.get_json()
        from_currency = data.get('from_currency', '').upper()
        to_currency = data.get('to_currency', '').upper()
        rate = float(data.get('rate', 0))
        
        rate_key = f"{from_currency}_{to_currency}"
        
        if rate_key not in manual_rates:
            return jsonify({"error": "Rate not found"}), 404
        
        manual_rates[rate_key].update({
            "rate": rate,
            "timestamp": datetime.utcnow().isoformat()
        })
        
        add_log("INFO", f"Updated manual rate: {from_currency} -> {to_currency} = {rate}")
        
        return jsonify({
            "success": True,
            "rate": manual_rates[rate_key]
        })
        
    except Exception as e:
        add_log("ERROR", f"Failed to update manual rate: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/rates", methods=["GET"])
def list_all_rates():
    """
    List all exchange rates including manual and fetched rates.
    This overrides the existing get_rates function to include manual rates.
    """
    try:
        # Fetch rates from sources
        all_rates = []
        
        # Norges Bank rates
        try:
            nb_rates = norges_bank.get_latest_rates(currencies=config.NORGES_BANK_CURRENCIES)
            for rate in nb_rates:
                currency = rate['base'] if rate['quote'] == 'NOK' else rate['quote']
                all_rates.append({
                    "from_currency": "NOK",
                    "to_currency": currency,
                    "rate": rate['value'],
                    "source": "norgesbank",
                    "timestamp": rate['date']
                })
        except Exception as e:
            add_log("WARNING", f"Failed to fetch Norges Bank rates: {str(e)}")
        
        # Abstract API rates
        if abstract_api.api_key:
            try:
                abstract_rates = abstract_api.get_latest_rates(base="NOK", targets=config.ABSTRACT_CURRENCIES)
                for rate in abstract_rates:
                    currency = rate['base'] if rate['quote'] == 'NOK' else rate['quote']
                    all_rates.append({
                        "from_currency": "NOK",
                        "to_currency": currency,
                        "rate": rate['value'],
                        "source": "abstract",
                        "timestamp": rate['date']
                    })
            except Exception as e:
                add_log("WARNING", f"Failed to fetch Abstract API rates: {str(e)}")
        
        # Add manual rates
        for rate in manual_rates.values():
            all_rates.append(rate)
        
        return jsonify({
            "success": True,
            "rates": all_rates,
            "count": len(all_rates),
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        add_log("ERROR", f"Failed to list rates: {str(e)}")
        return jsonify({"error": str(e)}), 500


# =================================================================
# STATUS & MONITORING ENDPOINTS
# =================================================================


@app.route("/api/currency/status", methods=["GET"])
def get_service_status():
    """Get service status including cron jobs, logs, and service health."""
    try:
        # Check service health
        services = []
        
        # Norges Bank status
        try:
            nb_rates = norges_bank.get_latest_rates(currencies=["USD"])
            services.append({
                "service": "Norges Bank",
                "status": "healthy",
                "last_check": datetime.utcnow().isoformat(),
                "response_time_ms": 100  # Simplified
            })
        except Exception as e:
            services.append({
                "service": "Norges Bank",
                "status": "down",
                "last_check": datetime.utcnow().isoformat(),
                "error": str(e)
            })
        
        # Abstract API status
        if abstract_api.api_key:
            try:
                abstract_rates = abstract_api.get_latest_rates(base="NOK", targets=["CLP"])
                services.append({
                    "service": "Abstract API",
                    "status": "healthy",
                    "last_check": datetime.utcnow().isoformat(),
                    "response_time_ms": 150  # Simplified
                })
            except Exception as e:
                services.append({
                    "service": "Abstract API",
                    "status": "down",
                    "last_check": datetime.utcnow().isoformat(),
                    "error": str(e)
                })
        else:
            services.append({
                "service": "Abstract API",
                "status": "degraded",
                "last_check": datetime.utcnow().isoformat(),
                "error": "API key not configured"
            })
        
        # Calculate next run (hourly sync)
        next_run = None
        if cron_status["last_run"]:
            last_run_dt = datetime.fromisoformat(cron_status["last_run"])
            next_run = (last_run_dt + timedelta(hours=1)).isoformat()
        
        # Build cron jobs status
        cron_jobs = [{
            "name": "Currency Rate Sync",
            "schedule": "0 * * * * (Every hour)",
            "last_run": cron_status["last_run"],
            "next_run": next_run,
            "status": cron_status["status"],
            "last_error": cron_status["last_error"]
        }]
        
        return jsonify({
            "success": True,
            "cron_jobs": cron_jobs,
            "services": services,
            "logs": log_entries[-50:],  # Last 50 logs
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        logger.error(f"Failed to get service status: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/currency/sync", methods=["POST"])
def manual_sync():
    """Manually trigger rate synchronization."""
    try:
        add_log("INFO", "Manual sync triggered")
        cron_status["status"] = "active"
        
        updated_count = 0
        errors = []
        
        # Sync from Norges Bank
        try:
            nb_rates = norges_bank.get_latest_rates(currencies=config.NORGES_BANK_CURRENCIES)
            updated_count += len(nb_rates)
            add_log("INFO", f"Synced {len(nb_rates)} rates from Norges Bank")
        except Exception as e:
            error_msg = f"Norges Bank sync failed: {str(e)}"
            errors.append(error_msg)
            add_log("ERROR", error_msg)
        
        # Sync from Abstract API
        if abstract_api.api_key:
            try:
                abstract_rates = abstract_api.get_latest_rates(base="NOK", targets=config.ABSTRACT_CURRENCIES)
                updated_count += len(abstract_rates)
                add_log("INFO", f"Synced {len(abstract_rates)} rates from Abstract API")
            except Exception as e:
                error_msg = f"Abstract API sync failed: {str(e)}"
                errors.append(error_msg)
                add_log("ERROR", error_msg)
        
        # Update cron status
        cron_status["last_run"] = datetime.utcnow().isoformat()
        cron_status["status"] = "idle"
        
        if errors:
            cron_status["last_error"] = "; ".join(errors)
        else:
            cron_status["last_error"] = None
        
        add_log("INFO", f"Sync completed: {updated_count} rates updated")
        
        return jsonify({
            "success": True,
            "updated_count": updated_count,
            "errors": errors if errors else None,
            "timestamp": datetime.utcnow().isoformat()
        })
        
    except Exception as e:
        error_msg = f"Manual sync failed: {str(e)}"
        add_log("ERROR", error_msg)
        cron_status["status"] = "error"
        cron_status["last_error"] = error_msg
        return jsonify({"error": str(e)}), 500


# =================================================================
# ERROR HANDLERS
# =================================================================


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
    logger.info(f"Cache TTL: {config.cache_ttl}s")
    
    # Validate configuration
    config.validate()
    
    # Run Flask app
    app.run(
        host=config.host,
        port=config.port,
        debug=config.DEBUG,
    )
