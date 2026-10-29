"""Integration layer for using SuperOffice APIs in the application.

This module shows how to use both REST and Agent APIs together
for optimal performance and functionality.
"""

from typing import Dict, List, Any, Optional
import logging
from datetime import datetime

from clients.socrm_client import SuperOfficeClient
from clients.socrm_agent_client import SuperOfficeAgentClient
from config.settings import Settings

logger = logging.getLogger(__name__)


class SuperOfficeService:
    """
    High-level service for SuperOffice operations.
    
    Combines REST API (fast reads) with Agent API (complex operations)
    for optimal performance and functionality.
    """
    
    def __init__(self, access_token: str, settings: Settings):
        """Initialize both REST and Agent API clients."""
        self.rest_client = SuperOfficeClient(access_token, settings)
        self.agent_client = SuperOfficeAgentClient(access_token, settings)
        self.settings = settings
    
    # ============================================================
    # COMPANY OPERATIONS
    # ============================================================
    
    def get_all_companies(
        self,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """
        Get all companies (optimized for bulk reads).
        
        Uses REST API for fast pagination.
        """
        all_companies = []
        page = 0
        page_size = 100
        
        while True:
            companies = self.rest_client.get_companies(
                page=page,
                page_size=page_size,
                filters=filters
            )
            
            if not companies:
                break
            
            all_companies.extend(companies)
            page += 1
        
        logger.info(f"Retrieved {len(all_companies)} companies")
        return all_companies
    
    def get_company_details(self, company_id: int) -> Dict[str, Any]:
        """
        Get complete company details including all persons.
        
        Uses Agent API for full entity with relations.
        """
        return self.agent_client.get_contact_with_persons(company_id)
    
    def create_company(
        self,
        name: str,
        country: str = "Norway",
        category: str = "Customer",
        **kwargs
    ) -> Dict[str, Any]:
        """
        Create a new company with validation.
        
        Uses Agent API to ensure defaults and validation.
        """
        # Get default entity
        company = self.agent_client.create_default_contact_entity()
        
        # Set required fields
        company["Name"] = name
        company["Country"] = {"Value": country}
        company["Category"] = {"Value": category}
        
        # Set optional fields
        for key, value in kwargs.items():
            if key in ["Department", "OrgNr", "Number1"]:
                company[key] = value
            elif key == "Business":
                company[key] = {"Value": value}
        
        # Save with validation
        saved = self.agent_client.save_contact_entity(company)
        logger.info(f"Created company: {name} (ID: {saved['ContactId']})")
        
        return saved
    
    def update_company(
        self,
        company_id: int,
        **updates
    ) -> Dict[str, Any]:
        """
        Update company fields.
        
        Uses Agent API for proper validation.
        """
        # Get current entity
        company = self.agent_client.get_contact_entity(company_id)
        
        # Apply updates
        for key, value in updates.items():
            if key in ["Name", "Department", "OrgNr", "Number1"]:
                company[key] = value
            elif key in ["Country", "Category", "Business"]:
                company[key] = {"Value": value}
        
        # Save
        saved = self.agent_client.save_contact_entity(company)
        logger.info(f"Updated company ID {company_id}")
        
        return saved
    
    # ============================================================
    # PERSON/CONTACT OPERATIONS
    # ============================================================
    
    def get_all_contacts(
        self,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """
        Get all contacts/persons (optimized for bulk reads).
        
        Uses REST API for fast pagination.
        """
        all_contacts = []
        page = 0
        page_size = 100
        
        while True:
            contacts = self.rest_client.get_contacts(
                page=page,
                page_size=page_size,
                filters=filters
            )
            
            if not contacts:
                break
            
            all_contacts.extend(contacts)
            page += 1
        
        logger.info(f"Retrieved {len(all_contacts)} contacts")
        return all_contacts
    
    def create_contact(
        self,
        firstname: str,
        lastname: str,
        company_id: int,
        email: Optional[str] = None,
        phone: Optional[str] = None,
        **kwargs
    ) -> Dict[str, Any]:
        """
        Create a new person/contact.
        
        Uses Agent API for validation.
        """
        # Get default entity
        person = self.agent_client.create_default_person_entity()
        
        # Set required fields
        person["Firstname"] = firstname
        person["Lastname"] = lastname
        person["Contact"] = {"ContactId": company_id}
        
        # Set optional fields
        if email:
            person["Email"] = email
        if phone:
            person["MobilePhone"] = phone
        
        for key, value in kwargs.items():
            if key == "Position":
                person[key] = {"Value": value}
            elif key in ["Title", "Department"]:
                person[key] = value
        
        # Save
        saved = self.agent_client.save_person_entity(person)
        logger.info(
            f"Created contact: {firstname} {lastname} "
            f"(ID: {saved['PersonId']})"
        )
        
        return saved
    
    # ============================================================
    # SALE OPERATIONS
    # ============================================================
    
    def get_all_sales(
        self,
        filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """
        Get all sales (optimized for bulk reads).
        
        Uses REST API for fast pagination.
        """
        all_sales = []
        page = 0
        page_size = 100
        
        while True:
            sales = self.rest_client.get_sales(
                page=page,
                page_size=page_size,
                filters=filters
            )
            
            if not sales:
                break
            
            all_sales.extend(sales)
            page += 1
        
        logger.info(f"Retrieved {len(all_sales)} sales")
        return all_sales
    
    def create_sale(
        self,
        heading: str,
        amount: float,
        company_id: int,
        person_id: Optional[int] = None,
        currency: str = "NOK",
        sale_date: Optional[str] = None,
        **kwargs
    ) -> Dict[str, Any]:
        """
        Create a new sale.
        
        Uses Agent API for validation.
        """
        # Get default entity
        sale = self.agent_client.create_default_sale_entity()
        
        # Set required fields
        sale["Heading"] = heading
        sale["Amount"] = amount
        sale["Currency"] = {"Value": currency}
        sale["Contact"] = {"ContactId": company_id}
        
        if person_id:
            sale["Person"] = {"PersonId": person_id}
        
        if sale_date:
            sale["SaleDate"] = sale_date
        
        # Set optional fields
        for key, value in kwargs.items():
            if key == "Probability":
                sale[key] = {"Id": value}
            elif key in ["Status", "Description"]:
                sale[key] = value
        
        # Save
        saved = self.agent_client.save_sale_entity(sale)
        logger.info(f"Created sale: {heading} (ID: {saved['SaleId']})")
        
        return saved
    
    def mark_sale_as_sold(self, sale_id: int) -> Dict[str, Any]:
        """Mark a sale as sold (won)."""
        result = self.agent_client.set_as_sold(sale_id)
        logger.info(f"Marked sale {sale_id} as sold")
        return result
    
    def mark_sale_as_lost(self, sale_id: int) -> Dict[str, Any]:
        """Mark a sale as lost."""
        result = self.agent_client.set_as_lost(sale_id)
        logger.info(f"Marked sale {sale_id} as lost")
        return result
    
    # ============================================================
    # SEARCH OPERATIONS
    # ============================================================
    
    def search_companies(
        self,
        name: Optional[str] = None,
        country: Optional[str] = None,
        category: Optional[str] = None
    ) -> List[Dict[str, Any]]:
        """
        Search companies with filters.
        
        Uses REST API with OData filters.
        """
        filters = {}
        if country:
            filters["Country"] = country
        if category:
            filters["Category"] = category
        
        companies = self.rest_client.get_companies(
            page=0,
            page_size=100,
            filters=filters
        )
        
        # Client-side name filter (if OData doesn't support contains)
        if name:
            companies = [
                c for c in companies
                if name.lower() in c.get("Name", "").lower()
            ]
        
        return companies
    
    def advanced_search(
        self,
        entity_type: str,
        columns: List[str],
        restriction: str,
        page: int = 0,
        page_size: int = 100
    ) -> List[Dict[str, Any]]:
        """
        Advanced search using Archive Provider.
        
        Uses Agent API for complex queries.
        
        Args:
            entity_type: "Company", "Contact", or "Sale"
            columns: List of columns to retrieve
            restriction: Search criteria (e.g., "country = 'Norway'")
            page: Page number
            page_size: Results per page
        """
        # Map entity types to Archive Provider names
        provider_map = {
            "Company": "ContactDynamic",
            "Contact": "PersonDynamic",
            "Sale": "SaleDynamic"
        }
        
        provider = provider_map.get(entity_type)
        if not provider:
            raise ValueError(f"Unknown entity type: {entity_type}")
        
        results = self.agent_client.get_archive_list_by_columns(
            provider_name=provider,
            columns=columns,
            restriction=restriction,
            page=page,
            page_size=page_size
        )
        
        return results
    
    # ============================================================
    # LIST OPERATIONS (MDO)
    # ============================================================
    
    def get_categories(self) -> List[Dict[str, Any]]:
        """Get all category list items."""
        return self.rest_client.get_list("Category")
    
    def get_countries(self) -> List[Dict[str, Any]]:
        """Get all country list items."""
        return self.rest_client.get_list("Country")
    
    def get_business_types(self) -> List[Dict[str, Any]]:
        """Get all business type list items."""
        return self.rest_client.get_list("Business")
    
    def get_currencies(self) -> List[Dict[str, Any]]:
        """Get all currency list items."""
        return self.rest_client.get_list("Currency")
    
    # ============================================================
    # BULK SYNC OPERATIONS
    # ============================================================
    
    def sync_all_data(self) -> Dict[str, int]:
        """
        Sync all data from SuperOffice.
        
        Returns count of each entity type synced.
        """
        logger.info("Starting full sync...")
        
        # Sync companies
        companies = self.get_all_companies()
        
        # Sync contacts
        contacts = self.get_all_contacts()
        
        # Sync sales
        sales = self.get_all_sales()
        
        result = {
            "companies": len(companies),
            "contacts": len(contacts),
            "sales": len(sales),
            "timestamp": datetime.utcnow().isoformat()
        }
        
        logger.info(f"Sync complete: {result}")
        return result
    
    def sync_entity_type(self, entity_type: str) -> List[Dict[str, Any]]:
        """
        Sync specific entity type.
        
        Args:
            entity_type: "Company", "Contact", or "Sale"
        """
        if entity_type == "Company":
            return self.get_all_companies()
        elif entity_type == "Contact":
            return self.get_all_contacts()
        elif entity_type == "Sale":
            return self.get_all_sales()
        else:
            raise ValueError(f"Unknown entity type: {entity_type}")
    
    # ============================================================
    # UTILITY METHODS
    # ============================================================
    
    def health_check(self) -> bool:
        """Check if SuperOffice API is accessible."""
        return self.rest_client.health_check()
    
    def get_current_user(self) -> Dict[str, Any]:
        """Get current authenticated user."""
        return self.rest_client.get_current_user()


# ============================================================
# USAGE EXAMPLE
# ============================================================

def example_usage():
    """Example of how to use the SuperOfficeService."""
    
    from config.settings import Settings
    
    settings = Settings()
    access_token = "your_access_token"
    
    # Initialize service
    service = SuperOfficeService(access_token, settings)
    
    # Create a company
    company = service.create_company(
        name="Acme Corporation",
        country="Norway",
        category="Customer",
        org_nr="123456789"
    )
    
    # Create a contact
    contact = service.create_contact(
        firstname="John",
        lastname="Doe",
        company_id=company["ContactId"],
        email="john@acme.com",
        phone="+47 123 45 678"
    )
    
    # Create a sale
    sale = service.create_sale(
        heading="Enterprise Deal",
        amount=500000,
        company_id=company["ContactId"],
        person_id=contact["PersonId"],
        currency="NOK"
    )
    
    # Mark sale as sold
    service.mark_sale_as_sold(sale["SaleId"])
    
    # Search companies
    results = service.search_companies(
        name="Acme",
        country="Norway"
    )
    
    # Advanced search
    advanced = service.advanced_search(
        entity_type="Company",
        columns=["contactId", "name", "country"],
        restriction="country = 'Norway' AND category = 'Customer'"
    )
    
    # Sync all data
    sync_result = service.sync_all_data()
    print(f"Synced {sync_result['companies']} companies")


if __name__ == "__main__":
    example_usage()
