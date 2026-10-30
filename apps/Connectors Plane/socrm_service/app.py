"""SuperOffice CRM Integration Service - Main Application."""

import logging
from datetime import datetime
from typing import Dict, Any

from flask import Flask, jsonify, request
from flask_cors import CORS

from config.settings import Settings
from auth.oauth_handler import SuperOfficeOAuthHandler
from auth.microsoft_auth import MicrosoftAuthHandler
from clients.socrm_client import SuperOfficeClient
from clients.microsoft_graph import MicrosoftGraphClient
from clients.currency_converter import CurrencyConverter
from sync.contact_sync import ContactSync
from sync.company_sync import CompanySync
from sync.sale_sync import SaleSync
from cache.cache_manager import CacheManager
from utils.error_handler import handle_errors
from utils.logger import setup_logger

# Initialize Flask app
app = Flask(__name__)
CORS(app)

# Load configuration
settings = Settings()
logger = setup_logger(__name__, settings.log_level)
microsoft_auth = MicrosoftAuthHandler(settings)
cache_manager = CacheManager(settings.cache_dir)
currency_converter = CurrencyConverter(settings.currency_service_url
# Initialize components
oauth_handler = SuperOfficeOAuthHandler(settings)
cache_manager = CacheManager(settings.cache_dir)


@app.route("/health", methods=["GET"])
def health_check() -> tuple[Dict[str, Any], int]:
    """Health check endpoint."""
    return jsonify({
        "status": "healthy",
        "service": "socrm-service",
        "timestamp": datetime.utcnow().isoformat(),
        "version": "0.1.0"
    }), 200
microsoft/authorize", methods=["GET"])
@handle_errors
def microsoft_authorize() -> tuple[Dict[str, str], int]:
    """Initialize Microsoft OAuth authorization flow."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    auth_url = microsoft_auth.get_authorization_url(organization_id)
    return jsonify({"authorization_url": auth_url}), 200


@app.route("/api/auth/microsoft/callback", methods=["GET"])
@handle_errors
def microsoft_callback() -> tuple[Dict[str, str], int]:
    """Handle Microsoft OAuth callback."""
    code = request.args.get("code")
    state = request.args.get("state")
    
    if not code or not state:
        return jsonify({"error": "Missing code or state parameter"}), 400
    
    # Exchange code for tokens
    tokens = microsoft_auth.handle_callback(code, state)
    organization_id = tokens.get("organization_id")
    
    # Get user profile and currency preference
    try:
        graph_client = MicrosoftGraphClient(tokens["access_token"])
        user_profile = graph_client.get_user_profile()
        currency_preference = graph_client.get_user_currency_preference()
        
        return jsonify({
            "status": "success",
            "message": "Microsoft authorization successful",
            "organization_id": organization_id,
            "user": {
                "email": user_profile.get("mail") or user_profile.get("userPrincipalName"),
                "name": user_profile.get("displayName"),
                "currency_preference": currency_preference or settings.default_currency
            }
        }), 200
    except Exception as e:
        logger.warning(f"Could not fetch user profile: {e}")
        return jsonify({
            "status": "success",
            "message": "Authorization successful",
            "organization_id": organization_id
        }), 200


@app.route("/api/auth/callback", methods=["GET"])
@handle_errors
def oauth_callback() -> tuple[Dict[str, str], int]:
    """Handle OAuth callback from SuperOffice (Legacy)egacy SuperOffice)
def authorize() -> tuple[Dict[str, str], int]:
    """Initialize OAuth authorization flow."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    auth_url = oauth_handler.get_authorization_url(organization_id)
    return jsonify({"authorization_url": auth_url}), 200


@app.route("/api/auth/callback", methods=["GET"])
@handle_errors
def oauth_callback() -> tuple[Dict[str, str], int]:
    """Handle OAuth callback from SuperOffice."""
    code = request.args.get("code")
    state = request.args.get("state")
    
    if not code or not state:
        return jsonify({"error": "Missing code or state parameter"}), 400
    
    # Exchange code for tokens
    tokens = oauth_handler.handle_callback(code, state)
    
    return jsonify({
        "status": "success",
        "message": "Authorization successful",
        "organization_id": tokens.get("organization_id")
    }), 200


@app.route("/api/contacts", methods=["GET"])
@handle_errors
def get_contacts() -> tuple[Dict[str, Any], int]:
    """Get contacts from SuperOffice CRM."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    contact_sync = ContactSync(client, cache_manager)
    
    contacts = contact_sync.get_contacts()
    
    return jsonify({
        "contacts": contacts,
        "count": len(contacts)
    }), 200


@app.route("/api/companies", methods=["GET"])
@handle_errors
def get_companies() -> tuple[Dict[str, Any], int]:
    """Get companies from SuperOffice CRM."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    company_sync = CompanySync(client, cache_manager)
    
    companies = company_sync.get_companies()
    
    return jsonify({
        "companies": companies,
        "count": len(companies)
    }), 200


@app.route("/api/sales", methods=["GET"])
@handle_errors
def get_sales() -> tuple[Dict[str, Any], int]:
    """Get sales from SuperOffice CRM."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    sale_sync = SaleSync(client, cache_manager)
    
    sales = sale_sync.get_sales()
    
    return jsonify({
        "sales": sales,
        "count": len(sales)
    }), 200


@app.route("/api/sync/contacts", methods=["POST"])
@handle_errors
def sync_contacts() -> tuple[Dict[str, Any], int]:
    """Trigger contact synchronization."""
    organization_id = request.json.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    contact_sync = ContactSync(client, cache_manager)
    
    result = contact_sync.sync_contacts()
    
    return jsonify(result), 200


@app.route("/api/sync/companies", methods=["POST"])
@handle_errors
def sync_companies() -> tuple[Dict[str, Any], int]:
    """Trigger company synchronization."""
    organization_id = request.json.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    company_sync = CompanySync(client, cache_manager)
    
    result = company_sync.sync_companies()
    
    return jsonify(result), 200


@app.route("/api/sync/sales", methods=["POST"])
@handle_errors
def sync_sales() -> tuple[Dict[str, Any], int]:
    """Trigger sales synchronization."""
    organization_id = request.json.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get access token
    access_token = oauth_handler.get_access_token(organization_id)
    
    # Initialize client and sync
    client = SuperOfficeClient(access_token, settings)
    sale_sync = SaleSync(client, cache_manager)
    
    result = sale_sync.sync_sales()
    
    return jsonify(result), 200


@app.route("/api/user/profile", methods=["GET"])
@handle_errors
def get_user_profile() -> tuple[Dict[str, Any], int]:
    """Get user profile with currency preference from Microsoft Graph."""
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get Microsoft access token
    access_token = microsoft_auth.get_access_token(organization_id)
    
    # Get user profile
    graph_client = MicrosoftGraphClient(access_token)
    profile = graph_client.get_user_profile()
    currency = graph_client.get_user_currency_preference()
    location = graph_client.get_user_location()
    
    return jsonify({
        "email": profile.get("mail") or profile.get("userPrincipalName"),
        "name": profile.get("displayName"),
        "location": location,
        "currency_preference": currency or settings.default_currency,
        "language": profile.get("preferredLanguage")
    }), 200


@app.route("/api/currency/convert", methods=["POST"])
@handle_errors
def convert_currency() -> tuple[Dict[str, Any], int]:
    """Convert currency amount."""
    data = request.json
    
    amount = data.get("amount")
    from_currency = data.get("from_currency")
    to_currency = data.get("to_currency")
    
    if not all([amount, from_currency, to_currency]):
        return jsonify({"error": "amount, from_currency, and to_currency are required"}), 400
    
    result = currency_converter.convert_amount(
        amount,
        from_currency,
        to_currency,
        date=data.get("date")
    )
    
    if not result:
        return jsonify({"error": "Currency conversion failed"}), 500
    
    return jsonify(result), 200


@app.route("/api/sales/converted", methods=["GET"])
@handle_errors
def get_sales_with_conversion() -> tuple[Dict[str, Any], int]:
    """Get sales with automatic currency conversion based on user preference."""
    organization_id = request.args.get("organization_id")
    target_currency = request.args.get("currency")
    
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # If no target currency specified, try to get from user profile
    if not target_currency:
        try:
            access_token = microsoft_auth.get_access_token(organization_id)
            graph_client = MicrosoftGraphClient(access_token)
            target_currency = graph_client.get_user_currency_preference() or settings.default_currency
        except Exception as e:
            logger.warning(f"Could not get user currency preference: {e}")
            target_currency = settings.default_currency
    
    # Get sales data
    access_token = oauth_handler.get_access_token(organization_id)
    client = SuperOfficeClient(access_token, settings)
    sale_sync = SaleSync(client, cache_manager)
    sales = sale_sync.get_sales()
    
    # Convert each sale to target currency
    converted_sales = []
    for sale in sales:
        converted_sale = currency_converter.convert_superoffice_data(
            sale,
            target_currency
        )
        converted_sales.append(converted_sale)
    
    return jsonify({
        "sales": converted_sales,
        "count": len(converted_sales),
        "target_currency": target_currency
    }), 200


@app.route("/api/dashboard/data", methods=["GET"])
@handle_errors
def get_dashboard_data() -> tuple[Dict[str, Any], int]:
    """Get comprehensive dashboard data with automatic currency conversion.
    
    This endpoint provides all data needed for AI-powered dashboard creation:
    - Sales data with currency conversion
    - Contact and company information
    - User preferences
    - Currency rates
    """
    organization_id = request.args.get("organization_id")
    if not organization_id:
        return jsonify({"error": "organization_id is required"}), 400
    
    # Get user profile and preferences
    try:
        ms_token = microsoft_auth.get_access_token(organization_id)
        graph_client = MicrosoftGraphClient(ms_token)
        user_profile = graph_client.get_user_profile()
        target_currency = graph_client.get_user_currency_preference() or settings.default_currency
    except Exception as e:
        logger.warning(f"Could not get user preferences: {e}")
        target_currency = settings.default_currency
        user_profile = {}
    
    # Get SuperOffice data
    so_token = oauth_handler.get_access_token(organization_id)
    client = SuperOfficeClient(so_token, settings)
    
    # Get sales with conversion
    sale_sync = SaleSync(client, cache_manager)
    sales = sale_sync.get_sales()
    converted_sales = [
        currency_converter.convert_superoffice_data(sale, target_currency)
        for sale in sales
    ]
    
    # Get contacts
    contact_sync = ContactSync(client, cache_manager)
    contacts = contact_sync.get_contacts()
    
    # Get companies
    company_sync = CompanySync(client, cache_manager)
    companies = company_sync.get_companies()
    
    # Prepare dashboard data
    dashboard_data = {
        "user": {
            "name": user_profile.get("displayName"),
            "email": user_profile.get("mail") or user_profile.get("userPrincipalName"),
            "currency_preference": target_currency
        },
        "summary": {
            "total_sales": len(sales),
            "total_contacts": len(contacts),
            "total_companies": len(companies),
            "total_sales_value": sum(
                sale.get("AmountConverted", sale.get("Amount", 0))
                for sale in converted_sales
            ),
            "currency": target_currency
        },
        "sales": converted_sales[:20],  # Limit to latest 20 for dashboard
        "contacts": contacts[:50],
        "companies": companies[:50],
        "supported_currencies": currency_converter.get_supported_currencies()
    }
    
    return jsonify(dashboard_data), 200


@app.errorhandler(404)
def not_found(error: Any) -> tuple[Dict[str, str], int]:
    """Handle 404 errors."""
    return jsonify({"error": "Resource not found"}), 404


@app.errorhandler(500)
def internal_error(error: Any) -> tuple[Dict[str, str], int]:
    """Handle 500 errors."""
    logger.error(f"Internal server error: {error}")
    return jsonify({"error": "Internal server error"}), 500


if __name__ == "__main__":
    app.run(
        host=settings.host,
        port=settings.port,
        debug=settings.debug
    )
