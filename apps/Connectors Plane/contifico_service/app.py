"""Contifico Service - Main Flask Application."""
import logging
from flask import Flask, jsonify, request
from flask_cors import CORS

from config.settings import settings
from clients.contifico_client import ContificoClient, ContificoAPIError
from utils.logger import setup_logger
from bigquery_sync import BigQuerySyncManager


# Setup logging
logger = setup_logger("contifico_service", level=settings.LOG_LEVEL)

# Initialize Flask app
app = Flask(__name__)
app.config["SECRET_KEY"] = settings.FLASK_SECRET_KEY
CORS(app)

# Initialize Contifico client
contifico_client = ContificoClient()

# Initialize BigQuery Sync Manager (if configured)
bq_sync = None
if settings.GCP_PROJECT_ID and settings.GCP_DATASET_ID:
    try:
        bq_sync = BigQuerySyncManager(
            project_id=settings.GCP_PROJECT_ID,
            dataset_id=settings.GCP_DATASET_ID,
            location=settings.GCP_REGION
        )
        logger.info(f"BigQuery Sync Manager initialized: {settings.GCP_PROJECT_ID}.{settings.GCP_DATASET_ID}")
    except Exception as e:
        logger.error(f"Failed to initialize BigQuery Sync Manager: {str(e)}")
        bq_sync = None


@app.route("/health", methods=["GET"])
def health_check():
    """Health check endpoint."""
    # Check if API credentials are configured
    configured = bool(settings.CONTIFICO_API_KEY and settings.CONTIFICO_API_TOKEN)
    
    return jsonify({
        "status": "healthy",
        "service": "contifico_service",
        "version": "1.0.0",
        "configured": configured,
        "connected": configured,
        "company_name": "Aquatiq" if configured else "",
        "api_version": "v1"
    }), 200


@app.errorhandler(ContificoAPIError)
def handle_api_error(error):
    """Handle Contifico API errors."""
    logger.error(f"Contifico API error: {str(error)}")
    return jsonify({
        "error": "contifico_api_error",
        "message": str(error)
    }), 500


@app.errorhandler(Exception)
def handle_generic_error(error):
    """Handle generic errors."""
    logger.error(f"Unexpected error: {str(error)}", exc_info=True)
    return jsonify({
        "error": "internal_error",
        "message": "An unexpected error occurred"
    }), 500


# =========================================================================
# Data Endpoints (Simplified access for frontend)
# =========================================================================

@app.route("/data/categories", methods=["GET"])
def get_data_categories():
    """Get all product categories (data endpoint)."""
    try:
        categories = contifico_client.get_categories()
        return jsonify(categories), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching categories: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/data/warehouses", methods=["GET"])
def get_data_warehouses():
    """Get all warehouses (data endpoint)."""
    try:
        warehouses = contifico_client.get_warehouses()
        return jsonify(warehouses), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching warehouses: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/data/products", methods=["GET"])
def get_data_products():
    """Get all products (data endpoint)."""
    try:
        products = contifico_client.get_products(params=request.args.to_dict())
        return jsonify(products), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching products: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/data/brands", methods=["GET"])
def get_data_brands():
    """Get all brands (data endpoint)."""
    try:
        brands = contifico_client.get_brands()
        return jsonify(brands), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching brands: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/data/variants", methods=["GET"])
def get_data_variants():
    """Get all product variants (data endpoint)."""
    try:
        # For now, return empty list as variants might not be directly available
        return jsonify([]), 200
    except ContificoAPIError as e:
        return jsonify([]), 200


@app.route("/data/documents", methods=["GET"])
def get_data_documents():
    """Get all documents (data endpoint)."""
    try:
        documents = contifico_client.get_documents(params=request.args.to_dict())
        return jsonify(documents), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching documents: {str(e)}")
        return jsonify({"error": str(e)}), 500


@app.route("/data/movements", methods=["GET"])
def get_data_movements():
    """Get inventory movements (data endpoint)."""
    try:
        movements = contifico_client.get_inventory_movements(params=request.args.to_dict())
        return jsonify(movements), 200
    except ContificoAPIError as e:
        logger.error(f"API error fetching movements: {str(e)}")
        return jsonify({"error": str(e)}), 500


# =========================================================================
# BigQuery Sync Endpoints
# =========================================================================

@app.route("/sync/entity/<entity_type>", methods=["POST"])
def sync_entity_to_bigquery(entity_type):
    """
    Sync a specific entity type to BigQuery with historical tracking.
    
    Supported entity types:
    - categories
    - warehouses
    - products
    - brands
    - documents
    - movements
    """
    if not bq_sync:
        return jsonify({
            "error": "BigQuery sync not configured",
            "message": "GCP credentials or dataset not configured"
        }), 503
    
    # Entity type mapping - using actual field names from Contifico API
    entity_config = {
        "categories": {
            "fetch_fn": contifico_client.get_categories,
            "table_name": "contifico_categories",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/categories"
        },
        "warehouses": {
            "fetch_fn": contifico_client.get_warehouses,
            "table_name": "contifico_warehouses",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/warehouses"
        },
        "products": {
            "fetch_fn": lambda: contifico_client.get_products(params=request.args.to_dict()),
            "table_name": "contifico_products",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/products"
        },
        "brands": {
            "fetch_fn": contifico_client.get_brands,
            "table_name": "contifico_brands",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/brands"
        },
        "documents": {
            "fetch_fn": lambda: contifico_client.get_documents(params=request.args.to_dict()),
            "table_name": "contifico_documents",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/documents"
        },
        "movements": {
            "fetch_fn": lambda: contifico_client.get_inventory_movements(params=request.args.to_dict()),
            "table_name": "contifico_inventory_movements",
            "id_field": "id",  # Fixed: actual field name is 'id'
            "endpoint": "/api/data/movements"
        }
    }
    
    if entity_type not in entity_config:
        return jsonify({
            "error": "invalid_entity_type",
            "message": f"Entity type '{entity_type}' not supported",
            "supported_types": list(entity_config.keys())
        }), 400
    
    try:
        config = entity_config[entity_type]
        
        # Fetch data from Contifico API
        logger.info(f"Fetching {entity_type} from Contifico API...")
        data = config["fetch_fn"]()
        
        if not data:
            return jsonify({
                "status": "skipped",
                "reason": "no data from API",
                "entity_type": entity_type
            }), 200
        
        # Sync to BigQuery
        logger.info(f"Syncing {len(data)} {entity_type} records to BigQuery...")
        result = bq_sync.sync_data(
            table_name=config["table_name"],
            data=data,
            id_field=config["id_field"],
            source_api=config["endpoint"]
        )
        
        return jsonify({
            **result,
            "entity_type": entity_type,
            "api_records": len(data)
        }), 200
        
    except ContificoAPIError as e:
        logger.error(f"API error syncing {entity_type}: {str(e)}")
        return jsonify({
            "status": "error",
            "error": "api_error",
            "message": str(e),
            "entity_type": entity_type
        }), 500
    except Exception as e:
        logger.error(f"Error syncing {entity_type}: {str(e)}")
        return jsonify({
            "status": "error",
            "error": "sync_error",
            "message": str(e),
            "entity_type": entity_type
        }), 500


@app.route("/sync/all", methods=["POST"])
def sync_all_entities():
    """Sync all entity types to BigQuery."""
    if not bq_sync:
        return jsonify({
            "error": "BigQuery sync not configured"
        }), 503
    
    entity_types = ["categories", "warehouses", "products", "brands", "documents", "movements"]
    results = {}
    
    for entity_type in entity_types:
        try:
            # Call the sync endpoint for each entity
            with app.test_client() as client:
                response = client.post(f"/sync/entity/{entity_type}")
                results[entity_type] = response.get_json()
        except Exception as e:
            logger.error(f"Error syncing {entity_type}: {str(e)}")
            results[entity_type] = {
                "status": "error",
                "message": str(e)
            }
    
    # Calculate summary
    successful = sum(1 for r in results.values() if r.get("status") == "success")
    total = len(entity_types)
    
    return jsonify({
        "status": "completed",
        "summary": {
            "total_entities": total,
            "successful": successful,
            "failed": total - successful
        },
        "results": results
    }), 200


@app.route("/sync/history/<entity_type>/<record_id>", methods=["GET"])
def get_record_history(entity_type, record_id):
    """Get all historical versions of a specific record."""
    if not bq_sync:
        return jsonify({"error": "BigQuery sync not configured"}), 503
    
    entity_tables = {
        "categories": "contifico_categories",
        "warehouses": "contifico_warehouses",
        "products": "contifico_products",
        "brands": "contifico_brands",
        "documents": "contifico_documents",
        "movements": "contifico_inventory_movements"
    }
    
    if entity_type not in entity_tables:
        return jsonify({
            "error": "invalid_entity_type",
            "supported_types": list(entity_tables.keys())
        }), 400
    
    try:
        history = bq_sync.query_historical_data(
            table_name=entity_tables[entity_type],
            record_id=record_id
        )
        
        return jsonify({
            "entity_type": entity_type,
            "record_id": record_id,
            "versions": len(history),
            "history": history
        }), 200
        
    except Exception as e:
        logger.error(f"Error querying history: {str(e)}")
        return jsonify({
            "error": "query_error",
            "message": str(e)
        }), 500


@app.route("/sync/tables/info", methods=["GET"])
def get_all_tables_info():
    """Get information about all BigQuery tables."""
    if not bq_sync:
        return jsonify({"error": "BigQuery sync not configured"}), 503
    
    tables = [
        "contifico_categories",
        "contifico_warehouses",
        "contifico_products",
        "contifico_brands",
        "contifico_documents",
        "contifico_inventory_movements"
    ]
    
    tables_info = {}
    for table_name in tables:
        tables_info[table_name] = bq_sync.get_table_info(table_name)
    
    return jsonify({
        "dataset": f"{settings.GCP_PROJECT_ID}.{settings.GCP_DATASET_ID}",
        "region": settings.GCP_REGION,
        "tables": tables_info
    }), 200


@app.route("/gcp/query", methods=["POST"])
def query_bigquery():
    """
    Execute a query against BigQuery tables.
    Request body can be either:
    1. {"entity_type": "categories|warehouses|...", "limit": 100, ...}
    2. {"table": "contifico_categories", "dataset": "dw_1_staging_contifico", "limit": 100, ...}
    """
    try:
        data = request.get_json()
        
        # Support both frontend format (table + dataset) and original format (entity_type)
        table_name = data.get("table")
        dataset_id = data.get("dataset")
        entity_type = data.get("entity_type")
        limit = data.get("limit", 100)
        where_clause = data.get("where", "")
        order_by = data.get("order_by", "")

        # If table is provided directly, use it
        if table_name:
            # Validate it's a contifico table
            valid_tables = [
                "contifico_categories",
                "contifico_warehouses",
                "contifico_products",
                "contifico_brands",
                "contifico_documents",
                "contifico_inventory_movements"
            ]
            if table_name not in valid_tables:
                return jsonify({"error": f"Invalid table: {table_name}"}), 400
            
            # Use provided dataset or default
            if not dataset_id:
                dataset_id = settings.GCP_DATASET_ID
        
        # If entity_type is provided, map it to table name
        elif entity_type:
            table_mapping = {
                "categories": "contifico_categories",
                "warehouses": "contifico_warehouses",
                "products": "contifico_products",
                "brands": "contifico_brands",
                "documents": "contifico_documents",
                "movements": "contifico_inventory_movements"
            }
            table_name = table_mapping.get(entity_type)
            if not table_name:
                return jsonify({"error": f"Invalid entity_type: {entity_type}"}), 400
            dataset_id = settings.GCP_DATASET_ID
        
        else:
            return jsonify({"error": "Either 'table' or 'entity_type' is required"}), 400

        # Build query - use the provided dataset_id or default
        full_table_id = f"`{settings.GCP_PROJECT_ID}.{dataset_id}.{table_name}`"
        query = f"""
            SELECT * 
            FROM {full_table_id}
            WHERE `_is_current` = TRUE
        """

        if where_clause:
            query += f" AND {where_clause}"
        
        if order_by:
            query += f" ORDER BY {order_by}"
        else:
            query += " ORDER BY `_valid_from` DESC"
        
        query += f" LIMIT {int(limit)}"

        logger.info(f"Executing BigQuery query for {entity_type}: {query}")

        # Execute query using existing bq_sync client
        query_job = bq_sync.client.query(query)
        results = query_job.result()

        # Convert results to list of dicts
        rows = []
        for row in results:
            row_dict = dict(row.items())
            # Convert datetime objects to ISO strings
            for key, value in row_dict.items():
                if hasattr(value, 'isoformat'):
                    row_dict[key] = value.isoformat()
            rows.append(row_dict)

        return jsonify({
            "status": "success",
            "table": f"{settings.GCP_PROJECT_ID}.{dataset_id}.{table_name}",
            "dataset": dataset_id,
            "table_name": table_name,
            "count": len(rows),
            "data": rows
        }), 200

    except Exception as e:
        logger.error(f"Error querying BigQuery: {str(e)}")
        return jsonify({
            "error": str(e),
            "status": "error"
        }), 500


# =========================================================================
# Inventory Endpoints
# =========================================================================

@app.route("/api/inventory/categories", methods=["GET"])
def get_categories():
    """Get all product categories."""
    try:
        categories = contifico_client.get_categories()
        return jsonify(categories), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/categories/<int:category_id>", methods=["GET"])
def get_category(category_id):
    """Get a specific category."""
    try:
        category = contifico_client.get_category(category_id)
        return jsonify(category), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/warehouses", methods=["GET"])
def get_warehouses():
    """Get all warehouses."""
    try:
        warehouses = contifico_client.get_warehouses()
        return jsonify(warehouses), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/products", methods=["GET", "POST"])
def products():
    """Get all products or create a new product."""
    try:
        if request.method == "GET":
            products = contifico_client.get_products(params=request.args.to_dict())
            return jsonify(products), 200
        else:  # POST
            product_data = request.get_json()
            result = contifico_client.create_product(product_data)
            return jsonify(result), 201
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/products/<int:product_id>", methods=["GET", "PATCH"])
def product_detail(product_id):
    """Get or update a specific product."""
    try:
        if request.method == "GET":
            product = contifico_client.get_product(product_id)
            return jsonify(product), 200
        else:  # PATCH
            product_data = request.get_json()
            result = contifico_client.update_product(product_id, product_data)
            return jsonify(result), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/products/<int:product_id>/stock", methods=["GET"])
def get_product_stock(product_id):
    """Get stock information for a product."""
    try:
        stock = contifico_client.get_product_stock(product_id)
        return jsonify(stock), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/movements", methods=["GET", "POST"])
def inventory_movements():
    """Get inventory movements or create a new movement."""
    try:
        if request.method == "GET":
            movements = contifico_client.get_inventory_movements(params=request.args.to_dict())
            return jsonify(movements), 200
        else:  # POST
            movement_data = request.get_json()
            result = contifico_client.create_inventory_movement(movement_data)
            return jsonify(result), 201
    except ContificoAPIError as e:
        raise e


@app.route("/api/inventory/brands", methods=["GET"])
def get_brands():
    """Get all brands."""
    try:
        brands = contifico_client.get_brands()
        return jsonify(brands), 200
    except ContificoAPIError as e:
        raise e


# =========================================================================
# Transaction Endpoints
# =========================================================================

@app.route("/api/transactions/documents", methods=["GET", "POST", "PUT"])
def documents():
    """Get, create, or update documents."""
    try:
        if request.method == "GET":
            documents = contifico_client.get_documents(params=request.args.to_dict())
            return jsonify(documents), 200
        elif request.method == "POST":
            document_data = request.get_json()
            result = contifico_client.create_document(document_data)
            return jsonify(result), 201
        else:  # PUT
            document_data = request.get_json()
            result = contifico_client.update_document(document_data)
            return jsonify(result), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/transactions/documents/<int:document_id>", methods=["GET"])
def get_document(document_id):
    """Get a specific document."""
    try:
        document = contifico_client.get_document(document_id)
        return jsonify(document), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/transactions/documents/<int:document_id>/sri", methods=["PUT"])
def send_to_sri(document_id):
    """Send document to SRI."""
    try:
        result = contifico_client.send_document_to_sri(document_id)
        return jsonify(result), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/transactions/documents/<int:document_id>/collections", methods=["GET", "POST"])
def document_collections(document_id):
    """Get or create document collections."""
    try:
        if request.method == "GET":
            collections = contifico_client.get_document_collections(document_id)
            return jsonify(collections), 200
        else:  # POST
            collection_data = request.get_json()
            result = contifico_client.create_document_collection(document_id, collection_data)
            return jsonify(result), 201
    except ContificoAPIError as e:
        raise e


@app.route("/api/transactions", methods=["GET"])
def get_transactions():
    """Get all transactions."""
    try:
        transactions = contifico_client.get_transactions(params=request.args.to_dict())
        return jsonify(transactions), 200
    except ContificoAPIError as e:
        raise e


# =========================================================================
# Person Endpoints
# =========================================================================

@app.route("/api/persons", methods=["GET", "POST", "PUT"])
def persons():
    """Get, create, or update persons."""
    try:
        if request.method == "GET":
            persons = contifico_client.get_persons(params=request.args.to_dict())
            return jsonify(persons), 200
        elif request.method == "POST":
            person_data = request.get_json()
            result = contifico_client.create_person(person_data)
            return jsonify(result), 201
        else:  # PUT
            person_data = request.get_json()
            result = contifico_client.update_person(person_data)
            return jsonify(result), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/persons/<int:person_id>", methods=["GET"])
def get_person(person_id):
    """Get a specific person."""
    try:
        person = contifico_client.get_person(person_id)
        return jsonify(person), 200
    except ContificoAPIError as e:
        raise e


# =========================================================================
# Accounting Endpoints
# =========================================================================

@app.route("/api/accounting/cost-centers", methods=["GET"])
def get_cost_centers():
    """Get all cost centers."""
    try:
        cost_centers = contifico_client.get_cost_centers()
        return jsonify(cost_centers), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/accounting/accounts", methods=["GET"])
def get_accounting_accounts():
    """Get all accounting accounts."""
    try:
        accounts = contifico_client.get_accounting_accounts()
        return jsonify(accounts), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/accounting/journal-entries", methods=["POST"])
def create_journal_entry():
    """Create a journal entry."""
    try:
        entry_data = request.get_json()
        result = contifico_client.create_journal_entry(entry_data)
        return jsonify(result), 201
    except ContificoAPIError as e:
        raise e


@app.route("/api/accounting/journal-entries/<int:entry_id>", methods=["GET"])
def get_journal_entry(entry_id):
    """Get a specific journal entry."""
    try:
        entry = contifico_client.get_journal_entry(entry_id)
        return jsonify(entry), 200
    except ContificoAPIError as e:
        raise e


# =========================================================================
# Bank Endpoints
# =========================================================================

@app.route("/api/bank/accounts", methods=["GET"])
def get_bank_accounts():
    """Get all bank accounts."""
    try:
        accounts = contifico_client.get_bank_accounts()
        return jsonify(accounts), 200
    except ContificoAPIError as e:
        raise e


@app.route("/api/bank/movements", methods=["GET"])
def get_bank_movements():
    """Get bank movements."""
    try:
        movements = contifico_client.get_bank_movements(params=request.args.to_dict())
        return jsonify(movements), 200
    except ContificoAPIError as e:
        raise e


# =========================================================================
# GCP / BigQuery Endpoints
# =========================================================================

@app.route("/api/gcp/status", methods=["GET"])
def gcp_status():
    """
    Get GCP integration status.
    
    Returns BigQuery and Cloud Storage connection status,
    available tables, and last sync information.
    """
    import os
    try:
        from google.cloud import bigquery
        
        # Get project and dataset info
        project_id = os.getenv("GCP_PROJECT_ID", "prj-dw-dev")
        dataset_id = os.getenv("BIGQUERY_DATASET", "dw_1_staging_contifico")
        bucket_name = os.getenv("GCP_BUCKET_NAME", "aquatiq-dw-dev-storage")
        region = os.getenv("GCP_REGION", "europe-west1")
        
        # Initialize BigQuery client with location
        client = bigquery.Client(project=project_id, location=region)
        
        # Check BigQuery tables for Contifico
        bigquery_tables = {}
        table_names = ["Product", "Category", "Warehouse", "Document", "BankMovement"]
        
        for table_name in table_names:
            try:
                table_ref = f"{project_id}.{dataset_id}.{table_name}"
                table = client.get_table(table_ref)
                bigquery_tables[table_name] = True
            except:
                bigquery_tables[table_name] = False
        
        return jsonify({
            "connected": True,
            "project_id": project_id,
            "dataset": dataset_id,
            "bucket": bucket_name,
            "region": region,
            "bigquery_tables": bigquery_tables
        }), 200
        
    except Exception as e:
        logger.error(f"Error getting GCP status: {e}")
        return jsonify({
            "connected": False,
            "error": str(e),
            "project_id": os.getenv("GCP_PROJECT_ID", "prj-dw-dev"),
            "dataset": os.getenv("BIGQUERY_DATASET", "dw_1_staging_contifico"),
            "bucket": os.getenv("GCP_BUCKET_NAME", "aquatiq-dw-dev-storage"),
            "region": os.getenv("GCP_REGION", "europe-west1")
        }), 200  # Return 200 even on error so frontend can show status


@app.route("/api/gcp/query", methods=["POST"])
def query_gcp_data():
    """
    Query data from GCP BigQuery.
    
    Request body:
        {
            "table": "Product",  # required
            "limit": 100,        # optional, default 100
            "filters": {}        # optional
        }
    
    Returns:
        200: Query results
        400: Invalid request
        500: Server error
    """
    import os
    try:
        from google.cloud import bigquery
        
        data = request.get_json()
        
        if not data or "table" not in data:
            return jsonify({
                "error": "table parameter is required in request body"
            }), 400
        
        table = data.get("table")
        limit = data.get("limit", 100)
        
        project_id = os.getenv("GCP_PROJECT_ID", "prj-dw-dev")
        dataset_id = os.getenv("BIGQUERY_DATASET", "dw_1_staging_contifico")
        region = os.getenv("GCP_REGION", "europe-west1")
        
        # Initialize BigQuery client with location
        client = bigquery.Client(project=project_id, location=region)
        
        # Build and execute query
        query = f"""
            SELECT *
            FROM `{project_id}.{dataset_id}.{table}`
            LIMIT {limit}
        """
        
        logger.info(f"Executing BigQuery: {query}")
        
        query_job = client.query(query)
        results = query_job.result()
        
        # Convert results to list of dicts
        rows = []
        for row in results:
            rows.append(dict(row))
        
        return jsonify({
            "success": True,
            "table": table,
            "row_count": len(rows),
            "data": rows
        }), 200
        
    except Exception as e:
        logger.error(f"Error querying GCP data: {e}")
        return jsonify({
            "success": False,
            "error": str(e)
        }), 500


if __name__ == "__main__":
    logger.info(f"Starting Contifico Service on {settings.HOST}:{settings.PORT}")
    app.run(
        host=settings.HOST,
        port=settings.PORT,
        debug=settings.FLASK_DEBUG
    )
