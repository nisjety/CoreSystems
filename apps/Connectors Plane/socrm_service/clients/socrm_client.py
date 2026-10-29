"""SuperOffice CRM API Client.

Provides a comprehensive client for interacting with SuperOffice CRM using the RESTful API.
Supports contacts, companies, sales, projects, documents, appointments, and archive providers.

Based on SuperOffice NetServer Web Services documentation:
https://docs.superoffice.com/
"""

import logging
from datetime import datetime
from typing import Dict, List, Optional, Any, Union
from urllib.parse import urlencode

import requests
from requests.adapters import HTTPAdapter
from requests.exceptions import RequestException, HTTPError, Timeout
from urllib3.util.retry import Retry

from config.settings import Settings

logger = logging.getLogger(__name__)


class SuperOfficeAPIError(Exception):
    """Base exception for SuperOffice API errors."""
    pass


class SuperOfficeAuthError(SuperOfficeAPIError):
    """Exception for authentication/authorization errors."""
    pass


class SuperOfficeNotFoundError(SuperOfficeAPIError):
    """Exception when resource is not found."""
    pass


class SuperOfficeValidationError(SuperOfficeAPIError):
    """Exception for validation errors."""
    pass


class SuperOfficeClient:
    """Client for interacting with SuperOffice CRM API."""
    
    def __init__(self, access_token: str, settings: Settings):
        """Initialize SuperOffice client."""
        self.access_token = access_token
        self.settings = settings
        self.session = self._create_session()
    
    def _create_session(self) -> requests.Session:
        """Create requests session with retry logic."""
        session = requests.Session()
        
        retry_strategy = Retry(
            total=3,
            backoff_factor=1,
            status_forcelist=[429, 500, 502, 503, 504],
            allowed_methods=["GET", "POST", "PUT", "DELETE"]
        )
        
        adapter = HTTPAdapter(max_retries=retry_strategy)
        session.mount("http://", adapter)
        session.mount("https://", adapter)
        
        # Set default headers
        session.headers.update({
            "Authorization": f"Bearer {self.access_token}",
            "Accept": "application/json",
            "Content-Type": "application/json"
        })
        
        return session
    
    def _make_request(
        self,
        method: str,
        endpoint: str,
        **kwargs
    ) -> Union[Dict[str, Any], List[Any]]:
        """Make HTTP request to SuperOffice API with proper error handling.
        
        Args:
            method: HTTP method (GET, POST, PUT, DELETE)
            endpoint: API endpoint path
            **kwargs: Additional arguments for requests
            
        Returns:
            Response data as dict or list
            
        Raises:
            SuperOfficeAuthError: Authentication/authorization failed
            SuperOfficeNotFoundError: Resource not found
            SuperOfficeValidationError: Validation failed
            SuperOfficeAPIError: Other API errors
        """
        url = self.settings.get_api_url(endpoint)
        
        try:
            response = self.session.request(
                method,
                url,
                timeout=self.settings.request_timeout,
                **kwargs
            )
            response.raise_for_status()
            
            # Handle empty responses (e.g., DELETE)
            if response.status_code == 204 or not response.content:
                return {"success": True}
            
            return response.json()
        
        except HTTPError as e:
            status_code = e.response.status_code if e.response else None
            
            if status_code == 401:
                logger.error(f"Authentication failed: {method} {url}")
                raise SuperOfficeAuthError(f"Authentication failed: {e}")
            elif status_code == 403:
                logger.error(f"Authorization failed: {method} {url}")
                raise SuperOfficeAuthError(f"Authorization failed: {e}")
            elif status_code == 404:
                logger.warning(f"Resource not found: {method} {url}")
                raise SuperOfficeNotFoundError(f"Resource not found: {e}")
            elif status_code == 400:
                logger.error(f"Validation error: {method} {url}")
                raise SuperOfficeValidationError(f"Validation error: {e}")
            else:
                logger.error(f"API request failed: {method} {url} - {e}")
                raise SuperOfficeAPIError(f"API request failed: {e}")
        
        except Timeout as e:
            logger.error(f"Request timeout: {method} {url}")
            raise SuperOfficeAPIError(f"Request timeout: {e}")
        
        except RequestException as e:
            logger.error(f"Request failed: {method} {url} - {e}")
            raise SuperOfficeAPIError(f"Request failed: {e}")
    
    def get_contacts(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get contacts from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            # Add OData filters
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Contact", params=params)
        return response.get("value", [])
    
    def get_contact(self, contact_id: int) -> Dict[str, Any]:
        """Get single contact by ID."""
        response = self._make_request("GET", f"/Contact/{contact_id}")
        return response
    
    def create_contact(self, contact_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new contact."""
        response = self._make_request("POST", "/Contact", json=contact_data)
        return response
    
    def update_contact(self, contact_id: int, contact_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing contact."""
        response = self._make_request("PUT", f"/Contact/{contact_id}", json=contact_data)
        return response
    
    def get_companies(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get companies from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Company", params=params)
        return response.get("value", [])
    
    def get_company(self, company_id: int) -> Dict[str, Any]:
        """Get single company by ID."""
        response = self._make_request("GET", f"/Company/{company_id}")
        return response
    
    def create_company(self, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new company."""
        response = self._make_request("POST", "/Company", json=company_data)
        return response
    
    def update_company(self, company_id: int, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing company."""
        response = self._make_request("PUT", f"/Company/{company_id}", json=company_data)
        return response
    
    def get_sales(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get sales from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Sale", params=params)
        return response.get("value", [])
    
    def get_sale(self, sale_id: int) -> Dict[str, Any]:
        """Get single sale by ID."""
        response = self._make_request("GET", f"/Sale/{sale_id}")
        return response
    
    def create_sale(self, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new sale."""
        response = self._make_request("POST", "/Sale", json=sale_data)
        return response
    
    def update_sale(self, sale_id: int, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing sale."""
        response = self._make_request("PUT", f"/Sale/{sale_id}", json=sale_data)
        return response
    
    def get_activities(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get activities from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Activity", params=params)
        return response.get("value", [])
    
    def get_projects(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get projects from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Project", params=params)
        return response.get("value", [])
    
    def get_project(self, project_id: int) -> Dict[str, Any]:
        """Get single project by ID."""
        response = self._make_request("GET", f"/Project/{project_id}")
        return response
    
    def create_project(self, project_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new project."""
        response = self._make_request("POST", "/Project", json=project_data)
        return response
    
    def update_project(self, project_id: int, project_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing project."""
        response = self._make_request("PUT", f"/Project/{project_id}", json=project_data)
        return response
    
    # Document operations
    def get_documents(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get documents from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Document", params=params)
        return response.get("value", [])
    
    def get_document(self, document_id: int) -> Dict[str, Any]:
        """Get single document by ID."""
        response = self._make_request("GET", f"/Document/{document_id}")
        return response
    
    def create_document(self, document_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new document."""
        response = self._make_request("POST", "/Document", json=document_data)
        return response
    
    # Appointment operations
    def get_appointments(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get appointments from SuperOffice CRM."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                filter_strings.append(f"{key} eq '{value}'")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request("GET", "/Appointment", params=params)
        return response.get("value", [])
    
    def get_appointment(self, appointment_id: int) -> Dict[str, Any]:
        """Get single appointment by ID."""
        response = self._make_request("GET", f"/Appointment/{appointment_id}")
        return response
    
    def create_appointment(self, appointment_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new appointment."""
        response = self._make_request("POST", "/Appointment", json=appointment_data)
        return response
    
    def update_appointment(self, appointment_id: int, appointment_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update an existing appointment."""
        response = self._make_request("PUT", f"/Appointment/{appointment_id}", json=appointment_data)
        return response
    
    # Archive Provider - advanced search capabilities
    def search_archive(
        self,
        provider_name: str,
        columns: List[str],
        filters: Optional[Dict[str, Any]] = None,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """Search using SuperOffice Archive Provider.
        
        Archive providers provide powerful search capabilities across multiple entities.
        Common providers: ContactPersonDynamic, CompanyDynamic, SaleDynamic, ProjectDynamic
        
        Args:
            provider_name: Name of the archive provider
            columns: List of column names to retrieve
            filters: Dictionary of filter conditions
            page: Page number
            page_size: Number of results per page
            
        Returns:
            List of search results
        """
        params = {
            "$select": ",".join(columns),
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            filter_strings = []
            for key, value in filters.items():
                if isinstance(value, str):
                    filter_strings.append(f"{key} contains '{value}'")
                else:
                    filter_strings.append(f"{key} eq {value}")
            if filter_strings:
                params["$filter"] = " and ".join(filter_strings)
        
        response = self._make_request(
            "GET",
            f"/Archive/{provider_name}",
            params=params
        )
        return response.get("value", [])
    
    # User and associate operations
    def get_current_user(self) -> Dict[str, Any]:
        """Get current authenticated user information."""
        response = self._make_request("GET", "/User/currentPrincipal")
        return response
    
    def get_associates(self) -> List[Dict[str, Any]]:
        """Get all associates (users) in the database."""
        response = self._make_request("GET", "/User")
        return response.get("value", [])
    
    # MDO List operations (Multi-Database Objects)
    def get_list(
        self,
        list_name: str,
        flat_list: bool = False
    ) -> List[Dict[str, Any]]:
        """Get MDO list items.
        
        Common lists: category, business, position, country, currency, etc.
        
        Args:
            list_name: Name of the MDO list
            flat_list: If True, return flat list without hierarchy
            
        Returns:
            List of MDO items
        """
        params = {"flatList": str(flat_list).lower()}
        response = self._make_request("GET", f"/List/{list_name}", params=params)
        return response.get("value", []) if isinstance(response, dict) else response
    
    # Selection operations
    def get_selections(
        self,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """Get selections (saved searches)."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        response = self._make_request("GET", "/Selection", params=params)
        return response.get("value", [])
    
    def get_selection(self, selection_id: int) -> Dict[str, Any]:
        """Get single selection by ID."""
        response = self._make_request("GET", f"/Selection/{selection_id}")
        return response
    
    def get_selection_members(
        self,
        selection_id: int,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """Get members of a selection."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        response = self._make_request(
            "GET",
            f"/Selection/{selection_id}/Members",
            params=params
        )
        return response.get("value", [])
    
    # Webhook operations
    def create_webhook(
        self,
        name: str,
        events: List[str],
        target_url: str,
        secret: Optional[str] = None
    ) -> Dict[str, Any]:
        """Create a new webhook.
        
        Args:
            name: Webhook name
            events: List of events to subscribe to (e.g., ['contact.created', 'sale.changed'])
            target_url: URL to receive webhook notifications
            secret: Optional secret for webhook signature validation
            
        Returns:
            Created webhook data
        """
        webhook_data = {
            "Name": name,
            "Events": events,
            "TargetUrl": target_url,
            "State": "Active"
        }
        
        if secret:
            webhook_data["Secret"] = secret
        
        response = self._make_request("POST", "/Webhook", json=webhook_data)
        return response
    
    def get_webhooks(self) -> List[Dict[str, Any]]:
        """Get all webhooks."""
        response = self._make_request("GET", "/Webhook")
        return response.get("value", []) if isinstance(response, dict) else response
    
    def delete_webhook(self, webhook_id: int) -> bool:
        """Delete a webhook."""
        try:
            self._make_request("DELETE", f"/Webhook/{webhook_id}")
            return True
        except SuperOfficeAPIError:
            return False
    
    # Utility methods
    def health_check(self) -> bool:
        """Check if SuperOffice API is accessible."""
        try:
            self.get_current_user()
            return True
        except SuperOfficeAPIError:
            return False
