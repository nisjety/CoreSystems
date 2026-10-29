"""Contifico API Client."""
import logging
import time
from typing import Dict, Any, List, Optional
import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

from config.settings import settings
from utils.translator import ContificoTranslator


logger = logging.getLogger(__name__)


class ContificoAPIError(Exception):
    """Custom exception for Contifico API errors."""
    pass


class ContificoClient:
    """Client for interacting with Contifico ERP API."""

    def __init__(
        self,
        api_key: Optional[str] = None,
        api_token: Optional[str] = None,
        base_url: Optional[str] = None,
        translate_responses: bool = True
    ):
        """
        Initialize Contifico API client.
        
        Args:
            api_key: Contifico API key (uses settings if not provided)
            api_token: Contifico API token (uses settings if not provided)
            base_url: Base API URL (uses settings if not provided)
            translate_responses: If True, translate Spanish responses to English
        """
        self.api_key = api_key or settings.CONTIFICO_API_KEY
        self.api_token = api_token or settings.CONTIFICO_API_TOKEN
        self.base_url = (base_url or settings.CONTIFICO_API_BASE_URL).rstrip("/")
        self.translate_responses = translate_responses
        
        # Setup session with retry logic
        self.session = self._setup_session()
        
        logger.info("Contifico API client initialized")

    def _setup_session(self) -> requests.Session:
        """Setup requests session with retry logic."""
        session = requests.Session()
        
        # Configure retry strategy
        retry_strategy = Retry(
            total=settings.CONTIFICO_MAX_RETRIES,
            backoff_factor=settings.CONTIFICO_RETRY_DELAY,
            status_forcelist=[429, 500, 502, 503, 504],
            allowed_methods=["GET", "POST", "PUT", "PATCH", "DELETE"]
        )
        
        adapter = HTTPAdapter(max_retries=retry_strategy)
        session.mount("https://", adapter)
        session.mount("http://", adapter)
        
        # Set default headers
        # Contifico API uses the API key in Authorization header
        # and the token is sent as a query parameter or in requests
        session.headers.update({
            "Authorization": self.api_key,
            "Content-Type": "application/json",
            "Accept": "application/json",
            "api_key": self.api_key,
            "api_token": self.api_token
        })
        
        return session

    def _make_request(
        self,
        method: str,
        endpoint: str,
        params: Optional[Dict[str, Any]] = None,
        json: Optional[Dict[str, Any]] = None,
        translate: bool = True
    ) -> Dict[str, Any]:
        """
        Make HTTP request to Contifico API.
        
        Args:
            method: HTTP method (GET, POST, PUT, PATCH, DELETE)
            endpoint: API endpoint (without base URL)
            params: Query parameters
            json: JSON body data
            translate: If True, translate response to English
            
        Returns:
            Response data as dictionary
            
        Raises:
            ContificoAPIError: If request fails
        """
        url = f"{self.base_url}/{endpoint.lstrip('/')}"
        
        try:
            logger.debug(f"Making {method} request to {url}")
            
            response = self.session.request(
                method=method,
                url=url,
                params=params,
                json=json,
                timeout=30
            )
            
            # Check for HTTP errors
            if response.status_code == 401:
                raise ContificoAPIError("Unauthorized: Invalid API credentials")
            elif response.status_code == 403:
                raise ContificoAPIError("Forbidden: Access denied to resource")
            elif response.status_code == 404:
                raise ContificoAPIError(f"Not found: {url}")
            elif response.status_code >= 400:
                error_msg = f"API request failed with status {response.status_code}"
                try:
                    error_data = response.json()
                    error_msg += f": {error_data}"
                except:
                    error_msg += f": {response.text}"
                raise ContificoAPIError(error_msg)
            
            # Parse response
            if response.status_code == 204:  # No content
                return {}
            
            data = response.json()
            
            # Translate response if enabled
            if translate and self.translate_responses:
                if isinstance(data, list):
                    data = ContificoTranslator.translate_list(data)
                elif isinstance(data, dict):
                    data = ContificoTranslator.translate_dict(data)
            
            return data
            
        except requests.exceptions.RequestException as e:
            logger.error(f"Request failed: {str(e)}")
            raise ContificoAPIError(f"Request failed: {str(e)}")

    # =========================================================================
    # Inventory API Methods
    # =========================================================================

    def get_categories(self) -> List[Dict[str, Any]]:
        """Get all product categories."""
        return self._make_request("GET", "categoria/")

    def get_category(self, category_id: int) -> Dict[str, Any]:
        """Get a specific category by ID."""
        return self._make_request("GET", f"categoria/{category_id}/")

    def get_warehouses(self) -> List[Dict[str, Any]]:
        """Get all warehouses (bodegas)."""
        return self._make_request("GET", "bodega/")

    def get_warehouse(self, warehouse_id: int) -> Dict[str, Any]:
        """Get a specific warehouse by ID."""
        return self._make_request("GET", f"bodega/{warehouse_id}/")

    def get_variants(self) -> List[Dict[str, Any]]:
        """Get all product variants."""
        return self._make_request("GET", "variante/")

    def get_variant(self, variant_id: int) -> Dict[str, Any]:
        """Get a specific variant by ID."""
        return self._make_request("GET", f"variante/{variant_id}/")

    def get_brands(self) -> List[Dict[str, Any]]:
        """Get all brands (marcas)."""
        return self._make_request("GET", "marca/")

    def get_products(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get all products with optional filters."""
        return self._make_request("GET", "producto/", params=params)

    def get_product(self, product_id: int) -> Dict[str, Any]:
        """Get a specific product by ID."""
        return self._make_request("GET", f"producto/{product_id}/")

    def create_product(self, product_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new product."""
        return self._make_request("POST", "producto/", json=product_data)

    def update_product(self, product_id: int, product_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing product."""
        return self._make_request("PATCH", f"producto/{product_id}/", json=product_data)

    def get_product_stock(self, product_id: int) -> Dict[str, Any]:
        """Get stock information for a product."""
        return self._make_request("GET", f"producto/{product_id}/stock/")

    def get_inventory_movements(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get inventory movements."""
        return self._make_request("GET", "movimiento-inventario/", params=params)

    def get_inventory_movement(self, movement_id: int) -> Dict[str, Any]:
        """Get a specific inventory movement by ID."""
        return self._make_request("GET", f"movimiento-inventario/{movement_id}/")

    def create_inventory_movement(self, movement_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new inventory movement."""
        return self._make_request("POST", "movimiento-inventario/", json=movement_data)

    def get_delivery_notes(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get delivery notes (guías de remisión)."""
        return self._make_request("GET", "inventario/guia/", params=params)

    def create_delivery_note(self, note_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new delivery note."""
        return self._make_request("POST", "inventario/guia/", json=note_data)

    # =========================================================================
    # Transaction API Methods
    # =========================================================================

    def get_documents(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get all documents with optional filters."""
        return self._make_request("GET", "documento/", params=params)

    def get_document(self, document_id: int) -> Dict[str, Any]:
        """Get a specific document by ID."""
        return self._make_request("GET", f"documento/{document_id}/")

    def create_document(self, document_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new document."""
        return self._make_request("POST", "documento/", json=document_data)

    def update_document(self, document_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing document."""
        return self._make_request("PUT", "documento/", json=document_data)

    def send_document_to_sri(self, document_id: int) -> Dict[str, Any]:
        """Send document to SRI (tax authority)."""
        return self._make_request("PUT", f"documento/{document_id}/sri/")

    def get_document_collections(self, document_id: int) -> List[Dict[str, Any]]:
        """Get collections (cobros) for a document."""
        return self._make_request("GET", f"documento/{document_id}/cobro/")

    def create_document_collection(self, document_id: int, collection_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a collection for a document."""
        return self._make_request("POST", f"documento/{document_id}/cobro/", json=collection_data)

    def delete_document_collection(self, document_id: int, collection_id: int) -> Dict[str, Any]:
        """Delete a document collection."""
        return self._make_request("DELETE", f"documento/{document_id}/cobro/{collection_id}/")

    def create_document_cross(self, document_id: int, cross_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a document cross (cruce)."""
        return self._make_request("POST", f"documento/{document_id}/cruce/", json=cross_data)

    def create_document_account_cross(self, document_id: int, cross_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a document account cross (cruce de cuenta)."""
        return self._make_request("POST", f"documento/{document_id}/cruce_cuenta/", json=cross_data)

    def get_document_retention(self, document_id: int) -> Dict[str, Any]:
        """Get retention information for a document."""
        return self._make_request("GET", f"documento/{document_id}/retencion/")

    def get_transactions(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get all transactions."""
        return self._make_request("GET", "registro/transaccion/", params=params)

    def get_transaction(self, transaction_id: int) -> Dict[str, Any]:
        """Get a specific transaction by ID."""
        return self._make_request("GET", f"registro/transaccion/{transaction_id}/")

    # =========================================================================
    # Person API Methods
    # =========================================================================

    def get_persons(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get all persons (customers/suppliers) with optional filters."""
        return self._make_request("GET", "persona/", params=params)

    def get_person(self, person_id: int) -> Dict[str, Any]:
        """Get a specific person by ID."""
        return self._make_request("GET", f"persona/{person_id}/")

    def create_person(self, person_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new person."""
        return self._make_request("POST", "persona/", json=person_data)

    def update_person(self, person_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing person."""
        return self._make_request("PUT", "persona/", json=person_data)

    # =========================================================================
    # Accounting API Methods
    # =========================================================================

    def get_cost_centers(self) -> List[Dict[str, Any]]:
        """Get all cost centers."""
        return self._make_request("GET", "contabilidad/centro-costo/")

    def get_accounting_accounts(self) -> List[Dict[str, Any]]:
        """Get all accounting accounts (cuentas contables)."""
        return self._make_request("GET", "contabilidad/cuenta-contable/")

    def create_journal_entry(self, entry_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a journal entry (asiento contable)."""
        return self._make_request("POST", "contabilidad/asiento/", json=entry_data)

    def get_journal_entry(self, entry_id: int) -> Dict[str, Any]:
        """Get a specific journal entry by ID."""
        return self._make_request("GET", f"contabilidad/asiento/{entry_id}/")

    # =========================================================================
    # Bank API Methods
    # =========================================================================

    def get_bank_accounts(self) -> List[Dict[str, Any]]:
        """Get all bank accounts."""
        return self._make_request("GET", "banco/cuenta/")

    def get_bank_movements(self, params: Optional[Dict[str, Any]] = None) -> List[Dict[str, Any]]:
        """Get bank movements with optional filters."""
        return self._make_request("GET", "banco/movimiento/", params=params)
