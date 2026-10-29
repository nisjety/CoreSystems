"""SuperOffice System User Client.

Client for making API calls using System User (SOTicket) authentication.
This enables server-to-server communication without user interaction.
"""

import logging
from typing import Dict, List, Optional, Any, Union

import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

from config.settings import Settings
from auth.system_user_handler import SystemUserHandler, SystemUserError

logger = logging.getLogger(__name__)


class SuperOfficeSystemUserClient:
    """
    Client for SuperOffice API using System User authentication.
    
    Uses SOTicket authorization instead of Bearer tokens.
    Ideal for:
    - Background jobs and scheduled tasks
    - Automated workflows
    - System integrations without user presence
    - Bulk data operations
    
    Usage:
        settings = Settings()
        handler = SystemUserHandler(settings)
        
        # Get ticket (cached automatically)
        ticket = handler.get_system_user_ticket(
            system_user_token="YourApp-abc123...",
            context_identifier="Cust12345"
        )
        
        # Create client
        client = SuperOfficeSystemUserClient(ticket, settings)
        
        # Make API calls
        companies = client.get_companies()
    """
    
    def __init__(self, system_user_ticket: str, settings: Settings):
        """
        Initialize System User client.
        
        Args:
            system_user_ticket: System User ticket credential (not JWT)
            settings: Application settings
        """
        self.ticket = system_user_ticket
        self.settings = settings
        self.session = self._create_session()
    
    def _create_session(self) -> requests.Session:
        """Create requests session with retry logic and SOTicket authentication."""
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
        
        # Set SOTicket authorization headers
        session.headers.update({
            "Authorization": f"SOTicket {self.ticket}",
            "SO-AppToken": self.settings.client_secret,  # OAuth client_secret
            "Accept": "application/json",
            "Content-Type": "application/json"
        })
        
        logger.debug("Created session with SOTicket authentication")
        
        return session
    
    def update_ticket(self, new_ticket: str):
        """
        Update the System User ticket (e.g., after refresh).
        
        Args:
            new_ticket: New ticket credential
        """
        self.ticket = new_ticket
        self.session.headers.update({
            "Authorization": f"SOTicket {new_ticket}"
        })
        logger.info("Updated System User ticket")
    
    def _make_request(
        self,
        method: str,
        endpoint: str,
        **kwargs
    ) -> Union[Dict[str, Any], List[Any]]:
        """
        Make HTTP request to SuperOffice API.
        
        Args:
            method: HTTP method (GET, POST, PUT, DELETE)
            endpoint: API endpoint path
            **kwargs: Additional arguments for requests
            
        Returns:
            Response data as dict or list
            
        Raises:
            SystemUserError: If authentication fails
            Exception: For other API errors
        """
        url = self.settings.get_api_url(endpoint)
        
        try:
            response = self.session.request(method, url, **kwargs)
            
            # Handle 401 specially - ticket may have expired
            if response.status_code == 401:
                logger.error("System User ticket authentication failed (401 Unauthorized)")
                raise SystemUserError(
                    "System User ticket expired or invalid. "
                    "Call get_system_user_ticket() to refresh."
                )
            
            response.raise_for_status()
            
            # Return JSON if present
            if response.content:
                return response.json()
            return {}
        
        except SystemUserError:
            raise
        except requests.exceptions.HTTPError as e:
            logger.error(f"HTTP error: {e}")
            raise
        except Exception as e:
            logger.error(f"Request failed: {e}")
            raise
    
    # ============================================================
    # USER / LOOKUP OPERATIONS
    # ============================================================

    def get_associates(
        self,
        page: int = 0,
        page_size: int = 200,
        select: Optional[str] = None
    ) -> List[Dict[str, Any]]:
        """Get associates/users with pagination for lookup scenarios."""
        params = {
            "$top": page_size,
            "$skip": page * page_size,
            "$orderBy": "fullName",
            "$jsonSafe": "true",
        }

        if select:
            params["$select"] = select

        return self._make_request("GET", "User", params=params)

    def get_sale_types(self, include_deleted: bool = False) -> List[Dict[str, Any]]:
        """Get sale type list items for lead registration lookups."""
        return self._make_request(
            "GET",
            "List/SaleType/Items",
            params={"includeDeleted": str(include_deleted).lower()},
        )

    # ============================================================
    # COMPANY/CONTACT OPERATIONS
    # ============================================================
    
    def get_companies(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get companies/contacts with pagination."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            params.update(filters)
        
        return self._make_request("GET", "Contact", params=params)
    
    def get_company_by_id(self, contact_id: int) -> Dict[str, Any]:
        """Get company by ID."""
        return self._make_request("GET", f"Contact/{contact_id}")
    
    def create_company(self, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new company."""
        return self._make_request("POST", "Contact", json=company_data)
    
    def update_company(self, contact_id: int, company_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update existing company."""
        return self._make_request("PUT", f"Contact/{contact_id}", json=company_data)
    
    # ============================================================
    # PERSON/CONTACT OPERATIONS
    # ============================================================
    
    def get_persons(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get persons with pagination."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            params.update(filters)
        
        return self._make_request("GET", "Person", params=params)
    
    def get_person_by_id(self, person_id: int) -> Dict[str, Any]:
        """Get person by ID."""
        return self._make_request("GET", f"Person/{person_id}")
    
    def create_person(self, person_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new person."""
        return self._make_request("POST", "Person", json=person_data)
    
    def update_person(self, person_id: int, person_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update existing person."""
        return self._make_request("PUT", f"Person/{person_id}", json=person_data)
    
    # ============================================================
    # SALE OPERATIONS
    # ============================================================
    
    def get_sales(
        self,
        page: int = 0,
        page_size: int = 100,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """Get sales with pagination."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        if filters:
            params.update(filters)
        
        return self._make_request("GET", "Sale", params=params)
    
    def get_sale_by_id(self, sale_id: int) -> Dict[str, Any]:
        """Get sale by ID."""
        return self._make_request("GET", f"Sale/{sale_id}")
    
    def create_sale(self, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Create a new sale."""
        return self._make_request("POST", "Sale", json=sale_data)
    
    def update_sale(self, sale_id: int, sale_data: Dict[str, Any]) -> Dict[str, Any]:
        """Update existing sale."""
        return self._make_request("PUT", f"Sale/{sale_id}", json=sale_data)
    
    # ============================================================
    # PROJECT OPERATIONS
    # ============================================================
    
    def get_projects(
        self,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """Get projects with pagination."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        return self._make_request("GET", "Project", params=params)
    
    def get_project_by_id(self, project_id: int) -> Dict[str, Any]:
        """Get project by ID."""
        return self._make_request("GET", f"Project/{project_id}")
    
    # ============================================================
    # APPOINTMENT OPERATIONS
    # ============================================================
    
    def get_appointments(
        self,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """Get appointments with pagination."""
        params = {
            "$top": page_size,
            "$skip": page * page_size
        }
        
        return self._make_request("GET", "Appointment", params=params)
    
    def get_appointment_by_id(self, appointment_id: int) -> Dict[str, Any]:
        """Get appointment by ID."""
        return self._make_request("GET", f"Appointment/{appointment_id}")
    
    # ============================================================
    # UTILITY OPERATIONS
    # ============================================================
    
    def health_check(self) -> bool:
        """
        Check if API connection with System User ticket is working.
        
        Returns:
            True if authenticated and can access API
        """
        try:
            # Try to get current principal (user info)
            result = self._make_request("GET", "User/currentPrincipal")
            logger.info(f"Health check passed - authenticated as: {result.get('FullName', 'Unknown')}")
            return True
        except SystemUserError:
            logger.error("Health check failed - ticket authentication error")
            return False
        except Exception as e:
            logger.error(f"Health check failed: {e}")
            return False
    
    def get_current_principal(self) -> Dict[str, Any]:
        """
        Get information about the current authenticated principal (System User).
        
        Returns:
            User/principal information
        """
        return self._make_request("GET", "User/currentPrincipal")
