"""
Moment Integration Service - Flask REST API

Provides API access to Moment time tracking and project management platform.
"""

import logging
import os
import sys
import traceback
from datetime import datetime, timezone
from typing import Optional

from flask import Flask, jsonify, request
from flask_cors import CORS

from clients import MomentAPIClient
from clients.moment_api_client import MomentAPIError, MomentAuthenticationError, MomentRateLimitError
from config import config
import sync as moment_sync

# Add shared module to path
sys.path.insert(0, '/app/shared')
sys.path.insert(0, '/app')

# Windows consoles default to a legacy codepage (e.g. cp1252) that can't encode
# the unicode symbols (✓, ✗, ✅, ❌) used in log/print output below. Force UTF-8
# so local `python app.py` runs on Windows don't crash on startup logging.
if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)

# Initialize Flask app
app = Flask(__name__)
app.secret_key = config.flask_secret_key

# Enable CORS with proper credentials support
CORS(app, resources={
    r"/api/*": {
        "origins": [
            "http://localhost:3000",
            "http://localhost:3001",
            "http://localhost:8080"
        ],
        "supports_credentials": True,
        "allow_headers": ["Content-Type", "Authorization"],
        "methods": ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]
    }
})

# Initialize Moment API client
try:
    moment_client = MomentAPIClient(
        api_key=config.api_key,
        company_code=config.company_code,
        base_url=config.base_url,
    )
    logger.info(f"✓ Initialized Moment API client for company: {config.company_code}")
except Exception as e:
    logger.error(f"✗ Failed to initialize Moment API client: {e}")
    moment_client = None

# Initialize GCP client
gcp_client = None
bigquery_sync = None
try:
    from shared.gcp_client import GCPClient
    from bigquery_sync import BigQuerySync
    
    gcp_client = GCPClient()
    bigquery_sync = BigQuerySync(gcp_client)
    logger.info("✓ Initialized GCP client for BigQuery sync")
except Exception as e:
    logger.warning(f"✗ GCP client not available: {e}")
    logger.warning("BigQuery sync features will be disabled")


# ===== Error Handlers =====

@app.errorhandler(MomentRateLimitError)
def handle_rate_limit_error(error):
    """Handle rate limit errors."""
    logger.warning(f"Rate limit exceeded: {error}")
    return jsonify({
        "error": "rate_limit_exceeded",
        "message": str(error)
    }), 429


@app.errorhandler(MomentAuthenticationError)
def handle_auth_error(error):
    """Handle authentication errors."""
    logger.error(f"Authentication error: {error}")
    return jsonify({
        "error": "authentication_failed",
        "message": "Invalid API key or unauthorized access"
    }), 401


@app.errorhandler(MomentAPIError)
def handle_api_error(error):
    """Handle general API errors."""
    logger.error(f"Moment API error: {error}")
    return jsonify({
        "error": "api_error",
        "message": str(error)
    }), 500


@app.errorhandler(Exception)
def handle_unexpected_error(error):
    """Handle unexpected errors."""
    logger.error(f"Unexpected error: {error}")
    logger.error(traceback.format_exc())
    return jsonify({
        "error": "internal_error",
        "message": "An unexpected error occurred"
    }), 500


# ===== Health & Status =====

@app.route("/health", methods=["GET"])
def health_check():
    """Health check endpoint with connection status."""
    is_connected = False
    connection_details = {}
    
    if moment_client and config.api_key:
        try:
            # Try to fetch a small dataset to verify connection
            projects = moment_client.get_projects(params={"page": 0, "size": 1})
            is_connected = True
            connection_details = {
                "company_code": config.company_code,
                "api_configured": True
            }
        except Exception as e:
            logger.error(f"Connection check failed: {e}")
            connection_details = {
                "error": str(e),
                "company_code": config.company_code,
                "api_configured": bool(config.api_key)
            }
    
    return jsonify({
        "status": "healthy" if is_connected else "degraded",
        "service": "moment_service",
        "version": "1.0.0",
        "connected": is_connected,
        "details": connection_details,
        "timestamp": datetime.now(timezone.utc).isoformat()
    })


@app.route("/api/status", methods=["GET"])
def get_status():
    """Get service status with organizations (API key info)."""
    is_connected = bool(moment_client and config.api_key)
    
    organizations = []
    if is_connected:
        organizations.append({
            "id": config.company_code,
            "name": f"Moment - {config.company_code}",
            "type": "moment",
            "status": "connected"
        })
    
    return jsonify({
        "connected": is_connected,
        "organizations": organizations,
        "company_code": config.company_code,
        "environment": config.environment,
        "api_configured": bool(config.api_key),
        "rate_limits": {
            "requests_per_second": config.rate_limit_requests_per_second,
            "requests_per_minute": config.rate_limit_requests_per_minute,
        }
    })


@app.route("/api/moment/status", methods=["GET"])
def get_moment_status():
    """Get detailed Moment service status (alias for /api/status)."""
    return get_status()


@app.route("/api/moment/data", methods=["GET"])
def get_moment_data():
    """
    Get data from Moment based on the table parameter.
    
    Query params:
        - table: Type of data (projects, time_records, customers)
        - limit: Number of records (default: 100)
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    table = request.args.get("table", "projects")
    limit = int(request.args.get("limit", "100"))
    
    try:
        if table == "projects":
            raw_data = moment_client.get_projects(params={"size": limit})
        elif table == "time_records":
            # Get time records from last 30 days
            from datetime import timedelta
            to_date = datetime.now()
            from_date = to_date - timedelta(days=30)
            raw_data = moment_client.get_time_records(
                from_date=from_date.strftime("%Y-%m-%d"),
                to_date=to_date.strftime("%Y-%m-%d"),
                params={"size": limit}
            )
        elif table == "customers":
            raw_data = moment_client.get_customers(params={"size": limit})
        else:
            return jsonify({"error": f"Unknown table: {table}"}), 400
        
        # Transform to expected format
        data_items = raw_data.get("items", []) if isinstance(raw_data, dict) else raw_data
        
        return jsonify({
            "data": data_items,
            "records": data_items,  # Backwards compatibility
            "count": len(data_items),
            "table": table
        })
    
    except Exception as e:
        logger.error(f"Error fetching {table}: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/moment/disconnect", methods=["POST"])
def disconnect_moment():
    """
    Disconnect Moment (clear API configuration).
    
    Note: Since Moment uses API keys, this is more of a "reset" operation.
    """
    try:
        # For API key-based services, we can't really "disconnect"
        # But we can clear any cached data or session info
        logger.info("Disconnect requested for Moment service")
        
        return jsonify({
            "success": True,
            "message": "Moment service disconnect requested",
            "note": "API key-based authentication cannot be fully disconnected without removing credentials"
        })
    
    except Exception as e:
        logger.error(f"Disconnect failed: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


@app.route("/api/moment/sync", methods=["POST"])
@app.route("/api/moment/sync/start", methods=["POST"])
def sync_moment_data():
    """
    Trigger data synchronization for Moment.
    
    Syncs projects, time records, and customer data.
    """
    if not moment_client:
        return jsonify({
            "success": False,
            "message": "Client not initialized"
        }), 500
    
    try:
        # In production, this would trigger background sync jobs
        logger.info(f"Starting sync for Moment company: {config.company_code}")
        
        return jsonify({
            "success": True,
            "message": f"Sync started for {config.company_code}",
            "organization_count": 1,
            "status": "started",
            "job_id": f"moment-sync-{datetime.now(timezone.utc).strftime('%Y%m%d%H%M%S')}",
            "sync_types": ["projects", "time_records", "customers"],
            "timestamp": datetime.now(timezone.utc).isoformat()
        })
    
    except Exception as e:
        logger.error(f"Sync failed: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


# ===== Projects =====

@app.route("/api/projects", methods=["GET"])
def get_projects():
    """
    Get list of projects.
    
    Query params:
        - includes: Optional includes string (e.g., "base+customer")
        - Additional filter params as needed
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        projects = moment_client.get_projects(includes=includes, params=params or None)
        return jsonify(projects)
    except Exception as e:
        logger.error(f"Error fetching projects: {e}")
        raise


@app.route("/api/projects/<int:project_id>", methods=["GET"])
def get_project(project_id: int):
    """
    Get single project by ID.
    
    Query params:
        - includes: Optional includes string
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    
    try:
        project = moment_client.get_project(project_id, includes=includes)
        return jsonify(project)
    except Exception as e:
        logger.error(f"Error fetching project {project_id}: {e}")
        raise


@app.route("/api/projects/registered-hours-report", methods=["GET"])
def get_project_registered_hours_report():
    """
    Get project registered hours report.
    
    Query params (required):
        - fromDate: Start date (YYYY-MM-DD)
        - toDate: End date (YYYY-MM-DD)
    
    Optional params:
        - projectTagOperator
        - timeRecordTagOperator
        - groupUserAccountOption
        - billableRateOption
        - subtractAbsences
        - etc.
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    from_date = request.args.get("fromDate")
    to_date = request.args.get("toDate")
    
    if not from_date or not to_date:
        return jsonify({
            "error": "missing_parameters",
            "message": "fromDate and toDate are required"
        }), 400
    
    # Get all other params
    kwargs = {k: v for k, v in request.args.items() if k not in ["fromDate", "toDate"]}
    
    try:
        report = moment_client.get_project_registered_hours_report(
            from_date=from_date,
            to_date=to_date,
            **kwargs
        )
        return jsonify(report)
    except Exception as e:
        logger.error(f"Error fetching registered hours report: {e}")
        raise


# ===== Time Records =====

@app.route("/api/time-records", methods=["GET", "POST"])
def time_records():
    """
    GET: List time records
    POST: Create new time record
    
    Query params for GET:
        - includes: Optional includes string
        - Additional filter params
    
    Body for POST:
        - Time record data (projectId, date, hours, etc.)
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    if request.method == "GET":
        includes = request.args.get("includes")
        params = {k: v for k, v in request.args.items() if k != "includes"}
        
        try:
            records = moment_client.get_time_records(includes=includes, params=params or None)
            return jsonify(records)
        except Exception as e:
            logger.error(f"Error fetching time records: {e}")
            raise
    
    elif request.method == "POST":
        data = request.get_json()
        includes = request.args.get("includes")
        
        if not data:
            return jsonify({
                "error": "missing_data",
                "message": "Request body is required"
            }), 400
        
        try:
            record = moment_client.create_time_record(data, includes=includes)
            return jsonify(record), 201
        except Exception as e:
            logger.error(f"Error creating time record: {e}")
            raise


@app.route("/api/time-records/<int:record_id>", methods=["GET", "PATCH", "DELETE"])
def time_record(record_id: int):
    """
    GET: Get single time record
    PATCH: Update time record
    DELETE: Delete time record
    
    Query params for GET/PATCH:
        - includes: Optional includes string
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    if request.method == "GET":
        includes = request.args.get("includes")
        
        try:
            record = moment_client.get_time_record(record_id, includes=includes)
            return jsonify(record)
        except Exception as e:
            logger.error(f"Error fetching time record {record_id}: {e}")
            raise
    
    elif request.method == "PATCH":
        data = request.get_json()
        includes = request.args.get("includes")
        
        if not data:
            return jsonify({
                "error": "missing_data",
                "message": "Request body is required"
            }), 400
        
        try:
            record = moment_client.update_time_record(record_id, data, includes=includes)
            return jsonify(record)
        except Exception as e:
            logger.error(f"Error updating time record {record_id}: {e}")
            raise
    
    elif request.method == "DELETE":
        try:
            moment_client.delete_time_record(record_id)
            return jsonify({"message": "Time record deleted"}), 204
        except Exception as e:
            logger.error(f"Error deleting time record {record_id}: {e}")
            raise


@app.route("/api/time-records/report", methods=["GET"])
def get_time_records_report():
    """
    Get complex time records report.
    
    Query params (required):
        - from: Start date (YYYY-MM-DD)
    
    Optional params:
        - groupBy: e.g., "userAccount,week"
        - projectId
        - orderBy
        - showProjectResourceReservationInfo
        - etc.
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    from_date = request.args.get("from")
    
    if not from_date:
        return jsonify({
            "error": "missing_parameters",
            "message": "from date is required"
        }), 400
    
    group_by = request.args.get("groupBy")
    kwargs = {k: v for k, v in request.args.items() if k not in ["from", "groupBy"]}
    
    try:
        report = moment_client.get_time_records_report(
            from_date=from_date,
            group_by=group_by,
            **kwargs
        )
        return jsonify(report)
    except Exception as e:
        logger.error(f"Error fetching time records report: {e}")
        raise


# ===== Batch Operations =====

@app.route("/api/<entity_type>/batch", methods=["POST"])
def batch_operations(entity_type: str):
    """
    Batch create, update, delete operations.
    
    Body:
        {
            "create": [{...}, {...}],  // Optional
            "update": {"id": {...}, "id2": {...}},  // Optional
            "delete": [id1, id2]  // Optional
        }
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    data = request.get_json()
    
    if not data:
        return jsonify({
            "error": "missing_data",
            "message": "Request body is required"
        }), 400
    
    create = data.get("create")
    update = data.get("update")
    delete = data.get("delete")
    
    try:
        result = moment_client.batch(
            endpoint=entity_type,
            create=create,
            update=update,
            delete=delete
        )
        return jsonify(result)
    except Exception as e:
        logger.error(f"Error in batch operation for {entity_type}: {e}")
        raise


# ===== Customers =====

@app.route("/api/customers", methods=["GET"])
def get_customers():
    """Get list of customers."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        customers = moment_client.get_customers(includes=includes, params=params or None)
        return jsonify(customers)
    except Exception as e:
        logger.error(f"Error fetching customers: {e}")
        raise


@app.route("/api/customers/<int:customer_id>", methods=["GET"])
def get_customer(customer_id: int):
    """Get single customer by ID."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    
    try:
        customer = moment_client.get_customer(customer_id, includes=includes)
        return jsonify(customer)
    except Exception as e:
        logger.error(f"Error fetching customer {customer_id}: {e}")
        raise


# ===== User Accounts =====

@app.route("/api/user-accounts", methods=["GET"])
def get_user_accounts():
    """Get list of user accounts."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        users = moment_client.get_user_accounts(includes=includes, params=params or None)
        return jsonify(users)
    except Exception as e:
        logger.error(f"Error fetching user accounts: {e}")
        raise


@app.route("/api/user-accounts/<int:user_id>", methods=["GET"])
def get_user_account(user_id: int):
    """Get single user account by ID."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    
    try:
        user = moment_client.get_user_account(user_id, includes=includes)
        return jsonify(user)
    except Exception as e:
        logger.error(f"Error fetching user account {user_id}: {e}")
        raise


# ===== Tags =====

@app.route("/api/tags", methods=["GET"])
def get_tags():
    """Get list of tags."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        tags = moment_client.get_tags(includes=includes, params=params or None)
        return jsonify(tags)
    except Exception as e:
        logger.error(f"Error fetching tags: {e}")
        raise


# ===== Invoices =====

@app.route("/api/invoices", methods=["GET"])
def get_invoices():
    """Get list of invoices."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        invoices = moment_client.get_invoices(includes=includes, params=params or None)
        return jsonify(invoices)
    except Exception as e:
        logger.error(f"Error fetching invoices: {e}")
        raise


@app.route("/api/invoices/<int:invoice_id>", methods=["GET"])
def get_invoice(invoice_id: int):
    """Get single invoice by ID."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    
    try:
        invoice = moment_client.get_invoice(invoice_id, includes=includes)
        return jsonify(invoice)
    except Exception as e:
        logger.error(f"Error fetching invoice {invoice_id}: {e}")
        raise


# ===== Offers =====

@app.route("/api/offers", methods=["GET"])
def get_offers():
    """Get list of offers (quotes/proposals)."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}

    try:
        offers = moment_client.get_offers(includes=includes, params=params or None)
        return jsonify(offers)
    except Exception as e:
        logger.error(f"Error fetching offers: {e}")
        raise


@app.route("/api/offers/<int:offer_id>", methods=["GET"])
def get_offer(offer_id: int):
    """Get single offer by ID."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    includes = request.args.get("includes")

    try:
        offer = moment_client.get_offer(offer_id, includes=includes)
        return jsonify(offer)
    except Exception as e:
        logger.error(f"Error fetching offer {offer_id}: {e}")
        raise


# ===== Invoicing Currency Setups =====

@app.route("/api/invoicing-currency-setups", methods=["GET"])
def get_invoicing_currency_setups():
    """Get list of invoicing currency setups."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    params = dict(request.args)

    try:
        setups = moment_client.get_invoicing_currency_setups(params=params or None)
        return jsonify(setups)
    except Exception as e:
        logger.error(f"Error fetching invoicing currency setups: {e}")
        raise


# ===== Project Membership Roles =====

@app.route("/api/project-membership-roles", methods=["GET"])
def get_project_membership_roles():
    """Get list of project membership roles."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    params = dict(request.args)

    try:
        roles = moment_client.get_project_membership_roles(params=params or None)
        return jsonify(roles)
    except Exception as e:
        logger.error(f"Error fetching project membership roles: {e}")
        raise


# ===== Expense Categories =====

@app.route("/api/expense-categories", methods=["GET"])
def get_expense_categories():
    """Get list of expense categories."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    params = dict(request.args)

    try:
        categories = moment_client.get_expense_categories(params=params or None)
        return jsonify(categories)
    except Exception as e:
        logger.error(f"Error fetching expense categories: {e}")
        raise


# ===== Custom Fields =====

@app.route("/api/custom-fields", methods=["GET"])
def get_custom_fields():
    """Get list of custom field definitions."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    custom_field_entity = request.args.get("customFieldEntity")
    params = {k: v for k, v in request.args.items() if k != "customFieldEntity"}

    try:
        fields = moment_client.get_custom_fields(custom_field_entity=custom_field_entity, params=params or None)
        return jsonify(fields)
    except Exception as e:
        logger.error(f"Error fetching custom fields: {e}")
        raise


# ===== Departments =====

@app.route("/api/departments", methods=["GET"])
def get_departments():
    """Get list of departments."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    params = dict(request.args)

    try:
        departments = moment_client.get_departments(params=params or None)
        return jsonify(departments)
    except Exception as e:
        logger.error(f"Error fetching departments: {e}")
        raise


# ===== Absences =====

@app.route("/api/absences", methods=["GET"])
def get_absences():
    """Get list of absences."""
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500

    params = dict(request.args)

    try:
        absences = moment_client.get_absences(params=params or None)
        return jsonify(absences)
    except Exception as e:
        logger.error(f"Error fetching absences: {e}")
        raise


# ===== Generic Entity Endpoint =====

@app.route("/api/entities/<entity_type>", methods=["GET"])
def get_entities(entity_type: str):
    """
    Generic endpoint to get any entity type.
    
    Path params:
        - entity_type: e.g., "projects", "timeRecords", "customers"
    
    Query params:
        - includes: Optional includes string
        - Additional filter params
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    params = {k: v for k, v in request.args.items() if k != "includes"}
    
    try:
        entities = moment_client.get_entities(
            entity_type=entity_type,
            includes=includes,
            params=params or None
        )
        return jsonify(entities)
    except Exception as e:
        logger.error(f"Error fetching {entity_type}: {e}")
        raise


@app.route("/api/entities/<entity_type>/<int:entity_id>", methods=["GET"])
def get_entity(entity_type: str, entity_id: int):
    """
    Generic endpoint to get single entity.
    
    Path params:
        - entity_type: e.g., "projects", "timeRecords"
        - entity_id: Entity ID
    
    Query params:
        - includes: Optional includes string
    """
    if not moment_client:
        return jsonify({"error": "Client not initialized"}), 500
    
    includes = request.args.get("includes")
    
    try:
        entity = moment_client.get_entity(
            entity_type=entity_type,
            entity_id=entity_id,
            includes=includes
        )
        return jsonify(entity)
    except Exception as e:
        logger.error(f"Error fetching {entity_type} {entity_id}: {e}")
        raise


# ===== GCP & BigQuery Endpoints =====

@app.route("/api/gcp/status", methods=["GET"])
def gcp_status():
    """Check GCP connection status and report which Moment tables exist in BigQuery."""
    try:
        if not gcp_client:
            return jsonify({
                "connected": False,
                "error": "GCP client not initialized",
                "project_id": os.getenv("GCP_PROJECT_ID"),
                "dataset": os.getenv("GCP_DATASET_ID")
            }), 200

        project_id = gcp_client.project_id
        dataset_id = gcp_client.dataset_id
        bucket_name = os.getenv("GCP_BUCKET_NAME")
        region = os.getenv("GCP_REGION")

        bigquery_tables = {}
        for entity_key, table_name in BigQuerySync.TABLE_MAP.items():
            table_ref = f"{project_id}.{dataset_id}.{table_name}"
            try:
                gcp_client.bigquery.get_table(table_ref)
                bigquery_tables[entity_key] = True
                query = f"SELECT COUNT(*) AS total_rows, MAX(synced_at) AS last_synced FROM `{table_ref}`"
                for row in gcp_client.query_bigquery(query):
                    bigquery_tables[f"{entity_key}_count"] = row.total_rows
                    bigquery_tables[f"{entity_key}_last_synced"] = str(row.last_synced) if row.last_synced else None
            except Exception:
                bigquery_tables[entity_key] = False

        return jsonify({
            "connected": True,
            "project_id": project_id,
            "dataset": dataset_id,
            "bucket": bucket_name,
            "region": region,
            "bigquery_tables": bigquery_tables
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


@app.route("/api/moment/bigquery/data", methods=["GET"])
def get_bigquery_data():
    """
    Query Moment data from BigQuery.
    
    Query params:
        - entity_type: Type of entity (time_records, projects, customers)
        - limit: Maximum number of records (default: 100)
        - project_id: Filter by project ID
        - customer_id: Filter by customer ID
        - user_id: Filter by user ID
        - start_date: Filter by start date (YYYY-MM-DD)
        - end_date: Filter by end date (YYYY-MM-DD)
    """
    try:
        if not bigquery_sync:
            return jsonify({
                "error": "BigQuery sync not available"
            }), 503
        
        entity_type = request.args.get("entity_type") or request.args.get("table") or "time_records"
        limit = int(request.args.get("limit", 100))
        
        # Build filters
        filters = {}
        if request.args.get("project_id"):
            filters["project_id"] = int(request.args.get("project_id"))
        if request.args.get("customer_id"):
            filters["customer_id"] = int(request.args.get("customer_id"))
        if request.args.get("user_id"):
            filters["user_id"] = int(request.args.get("user_id"))
        if request.args.get("start_date"):
            filters["start_date"] = request.args.get("start_date")
        if request.args.get("end_date"):
            filters["end_date"] = request.args.get("end_date")
        
        result = bigquery_sync.query_bigquery_data(
            entity_type=entity_type,
            limit=limit,
            filters=filters if filters else None
        )
        
        if result.get("success"):
            return jsonify(result)
        else:
            return jsonify(result), 500
            
    except Exception as e:
        logger.error(f"Error querying BigQuery data: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/moment/sync/gcp", methods=["POST"])
def sync_to_gcp():
    """
    Sync a single Moment entity type to BigQuery.

    JSON body:
        - entity_type: One of moment_sync.ALL_ENTITIES (default: "time_records")
        - data: Optional array of already-fetched items to sync instead of
          calling the Moment API (mainly useful for testing/backfills)
    """
    try:
        if not bigquery_sync:
            return jsonify({"error": "BigQuery sync not available"}), 503
        if not moment_client:
            return jsonify({"error": "Moment API client not initialized"}), 500

        data = request.get_json(silent=True) or {}
        entity_type = data.get("entity_type", "time_records")
        provided_data = data.get("data")

        if entity_type not in moment_sync.ALL_ENTITIES:
            return jsonify({
                "error": f"Invalid entity type: {entity_type}",
                "valid_entities": moment_sync.ALL_ENTITIES,
            }), 400

        if provided_data is not None:
            sync_method = getattr(bigquery_sync, moment_sync.ENTITY_SYNC_METHODS[entity_type])
            result = {"entity": entity_type, "fetched": len(provided_data), **sync_method(provided_data)}
        else:
            result = moment_sync.sync_entity(moment_client, bigquery_sync, entity_type)

        return jsonify(result) if result.get("success") else (jsonify(result), 500)

    except Exception as e:
        logger.error(f"Error syncing to GCP: {e}")
        return jsonify({"error": str(e)}), 500


@app.route("/api/moment/sync/full", methods=["POST"])
def sync_full_to_gcp():
    """
    Full sync: fetch every page of every configured Moment entity and
    full-refresh load it into BigQuery.

    JSON body (optional):
        - entities: array of entity keys to sync (default: all of moment_sync.ALL_ENTITIES)
    """
    try:
        if not bigquery_sync:
            return jsonify({"error": "BigQuery sync not available"}), 503
        if not moment_client:
            return jsonify({"error": "Moment API client not initialized"}), 500

        data = request.get_json(silent=True) or {}
        entities = data.get("entities")

        result = moment_sync.sync_all(moment_client, bigquery_sync, entities=entities)
        return jsonify(result) if result.get("success") else (jsonify(result), 500)

    except Exception as e:
        logger.error(f"Error running full sync to GCP: {e}")
        return jsonify({"error": str(e)}), 500


# ===== Run Server =====

if __name__ == "__main__":
    port = config.flask_port
    logger.info(f"Starting Moment service on port {port}")
    logger.info(f"Company: {config.company_code}")
    logger.info(f"Environment: {config.environment}")
    
    app.run(
        host="0.0.0.0",
        port=port,
        debug=os.getenv("FLASK_DEBUG", "false").lower() == "true"
    )
