"""SuperOffice API Integration Guide.

This document provides examples of how to use the SuperOffice REST API and Agent API
for common integration scenarios.
"""

# ==========================================
# SUPEROFFICE API ARCHITECTURE
# ==========================================

"""
SuperOffice provides two complementary APIs:

1. REST API (socrm_client.py)
   - Simple CRUD operations on entities
   - OData query support ($select, $filter, $top, $skip)
   - Direct entity access (GET /Contact/123)
   - Best for: Simple reads, lists, basic updates

2. Agent API (socrm_agent_client.py)
   - Business logic methods
   - Complex operations (merge, move, convert)
   - Transaction support
   - Best for: Complex workflows, business rules, validations
"""

# ==========================================
# MAPPING: SuperOffice vs Standard Naming
# ==========================================

"""
SuperOffice uses non-standard naming:

Standard Term  | SuperOffice REST | SuperOffice Agent | Entity Type
---------------|------------------|-------------------|-------------
Company        | /Contact         | ContactAgent      | ContactEntity
Contact/Person | /Person          | PersonAgent       | PersonEntity
Sale/Deal      | /Sale            | SaleAgent         | SaleEntity
Project        | /Project         | ProjectAgent      | ProjectEntity
Appointment    | /Appointment     | AppointmentAgent  | AppointmentEntity
Document       | /Document        | DocumentAgent     | DocumentEntity
"""

# ==========================================
# EXAMPLE 1: Get Companies (Simple)
# ==========================================

from clients.socrm_client import SuperOfficeClient

def get_companies_simple(client: SuperOfficeClient):
    """Get companies using REST API (simple, fast)."""
    
    # Get first 100 companies
    companies = client.get_companies(page=0, page_size=100)
    
    # Get companies with filter
    companies = client.get_companies(
        page=0,
        page_size=50,
        filters={
            "Category": "Customer",
            "Country": "Norway"
        }
    )
    
    # Get single company
    company = client.get_company(company_id=123)
    
    return companies


# ==========================================
# EXAMPLE 2: Get Companies (Advanced)
# ==========================================

from clients.socrm_agent_client import SuperOfficeAgentClient

def get_companies_advanced(client: SuperOfficeAgentClient):
    """Get companies using Agent API (complex queries)."""
    
    # Get company with all persons
    company_with_persons = client.get_contact_with_persons(contact_id=123)
    
    # Get full entity with all related data
    company_entity = client.get_contact_entity(contact_id=123)
    # Returns: ContactEntity with all fields, relations, interests, etc.
    
    return company_entity


# ==========================================
# EXAMPLE 3: Create Company
# ==========================================

def create_company_rest(client: SuperOfficeClient):
    """Create company using REST API."""
    
    company_data = {
        "Name": "Acme Corporation",
        "Department": "Sales",
        "Country": {"Value": "Norway"},
        "Category": {"Value": "Customer"},
        "OrgNr": "123456789"
    }
    
    new_company = client.create_company(company_data)
    return new_company


def create_company_agent(client: SuperOfficeAgentClient):
    """Create company using Agent API (with validation)."""
    
    # Step 1: Get default entity (with defaults from SuperOffice)
    company = client.create_default_contact_entity()
    
    # Step 2: Set values
    company["Name"] = "Acme Corporation"
    company["Department"] = "Sales"
    company["Country"] = {"Value": "Norway"}
    company["Category"] = {"Value": "Customer"}
    company["OrgNr"] = "123456789"
    
    # Step 3: Save (validates and applies business rules)
    saved_company = client.save_contact_entity(company)
    
    return saved_company


# ==========================================
# EXAMPLE 4: Create Person/Contact
# ==========================================

def create_person(client: SuperOfficeAgentClient):
    """Create a person associated with a company."""
    
    # Get default person entity
    person = client.create_default_person_entity()
    
    # Set values
    person["Firstname"] = "John"
    person["Lastname"] = "Doe"
    person["Contact"] = {"ContactId": 123}  # Associate with company
    person["Email"] = "john.doe@acme.com"
    person["MobilePhone"] = "+47 123 45 678"
    person["Position"] = {"Value": "CEO"}
    
    # Save
    saved_person = client.save_person_entity(person)
    
    return saved_person


# ==========================================
# EXAMPLE 5: Create Sale
# ==========================================

def create_sale(client: SuperOfficeAgentClient):
    """Create a new sale."""
    
    # Get default sale entity
    sale = client.create_default_sale_entity()
    
    # Set values
    sale["Heading"] = "Large Enterprise Deal"
    sale["Amount"] = 500000
    sale["Currency"] = {"Value": "NOK"}
    sale["Contact"] = {"ContactId": 123}  # Company
    sale["Person"] = {"PersonId": 456}    # Contact person
    sale["Associate"] = {"AssociateId": 7}  # Sales rep
    sale["Probability"] = {"Id": 50}  # 50% probability
    sale["SaleDate"] = "2026-06-01"
    sale["Status"] = "Open"
    
    # Save
    saved_sale = client.save_sale_entity(sale)
    
    return saved_sale


# ==========================================
# EXAMPLE 6: Create Appointment
# ==========================================

def create_appointment(client: SuperOfficeAgentClient):
    """Create an appointment."""
    
    # Get default appointment
    appointment = client.create_default_appointment_entity()
    
    # Set values
    appointment["Description"] = "Customer Meeting"
    appointment["Location"] = "Office"
    appointment["Contact"] = {"ContactId": 123}
    appointment["Person"] = {"PersonId": 456}
    appointment["StartDate"] = "2026-01-15T10:00:00"
    appointment["EndDate"] = "2026-01-15T11:00:00"
    appointment["Type"] = "Meeting"
    
    # Save
    saved_appointment = client.save_appointment_entity(appointment)
    
    return saved_appointment


# ==========================================
# EXAMPLE 7: Search Using Archive Provider
# ==========================================

def search_data(client: SuperOfficeAgentClient):
    """Advanced search using Archive Provider."""
    
    # Search contacts (persons)
    results = client.get_archive_list_by_columns(
        provider_name="PersonDynamic",
        columns=[
            "personId",
            "fullName",
            "contactId",
            "contactName",
            "email",
            "phone"
        ],
        restriction="email contains 'acme.com'",
        page=0,
        page_size=50
    )
    
    return results


def search_sales(client: SuperOfficeAgentClient):
    """Search sales with filters."""
    
    results = client.get_archive_list_by_columns(
        provider_name="SaleDynamic",
        columns=[
            "saleId",
            "heading",
            "amount",
            "contactId",
            "contactName",
            "probability",
            "saleDate"
        ],
        restriction="amount > 100000 AND probability > 50",
        sort_order="saleDate DESC",
        page=0,
        page_size=100
    )
    
    return results


# ==========================================
# EXAMPLE 8: Working with Lists (MDO)
# ==========================================

def get_list_data(client: SuperOfficeClient):
    """Get list data (categories, countries, etc.)."""
    
    # Get categories
    categories = client.get_list("Category")
    
    # Get countries
    countries = client.get_list("Country")
    
    # Get business types
    business_types = client.get_list("Business")
    
    # Get currencies
    currencies = client.get_list("Currency")
    
    return {
        "categories": categories,
        "countries": countries,
        "business": business_types,
        "currencies": currencies
    }


# ==========================================
# EXAMPLE 9: Manage Selections
# ==========================================

def create_selection(client: SuperOfficeAgentClient):
    """Create a dynamic selection."""
    
    # Get default selection
    selection = client.create_default_selection_entity()
    
    # Set values
    selection["Name"] = "High Value Customers"
    selection["Description"] = "Companies with >1M in sales"
    selection["TargetTableNumber"] = 5  # Contact table
    selection["Completed"] = 0  # Dynamic selection
    
    # Save
    saved_selection = client.save_selection_entity(selection)
    
    # Add members
    client.add_contact_selection_members(
        selection_id=saved_selection["SelectionId"],
        contact_ids=[123, 456, 789]
    )
    
    return saved_selection


# ==========================================
# EXAMPLE 10: Bulk Operations
# ==========================================

def sync_companies_to_bigquery(client: SuperOfficeClient):
    """Example: Sync all companies to BigQuery."""
    
    all_companies = []
    page = 0
    page_size = 100
    
    while True:
        # Get page of companies
        companies = client.get_companies(page=page, page_size=page_size)
        
        if not companies:
            break
        
        all_companies.extend(companies)
        
        # Process companies (transform and load to BigQuery)
        for company in companies:
            # Transform to BigQuery schema
            transformed = {
                "company_id": company.get("ContactId"),
                "name": company.get("Name"),
                "org_nr": company.get("OrgNr"),
                "country": company.get("Country", {}).get("Value"),
                "category": company.get("Category", {}).get("Value"),
                "updated_at": company.get("Updated")
            }
            
            # Load to BigQuery (pseudo-code)
            # bigquery_client.insert_row("companies", transformed)
        
        page += 1
    
    return len(all_companies)


# ==========================================
# EXAMPLE 11: Error Handling
# ==========================================

from clients.socrm_client import (
    SuperOfficeAPIError,
    SuperOfficeAuthError,
    SuperOfficeNotFoundError,
    SuperOfficeValidationError
)

def robust_get_company(client: SuperOfficeClient, company_id: int):
    """Get company with proper error handling."""
    
    try:
        company = client.get_company(company_id)
        return {"success": True, "data": company}
    
    except SuperOfficeNotFoundError:
        return {"success": False, "error": "Company not found"}
    
    except SuperOfficeAuthError:
        return {"success": False, "error": "Authentication failed"}
    
    except SuperOfficeValidationError as e:
        return {"success": False, "error": f"Validation error: {e}"}
    
    except SuperOfficeAPIError as e:
        return {"success": False, "error": f"API error: {e}"}


# ==========================================
# EXAMPLE 12: Complete Sync Flow
# ==========================================

def complete_sync_flow(access_token: str, settings):
    """Complete example: Sync SuperOffice data to BigQuery."""
    
    from clients.socrm_agent_client import SuperOfficeAgentClient
    
    # Initialize client
    client = SuperOfficeAgentClient(access_token, settings)
    
    # 1. Sync Companies
    print("Syncing companies...")
    companies = []
    page = 0
    while True:
        batch = client.get_companies(page=page, page_size=100)
        if not batch:
            break
        companies.extend(batch)
        page += 1
    print(f"Synced {len(companies)} companies")
    
    # 2. Sync Contacts (Persons)
    print("Syncing contacts...")
    contacts = []
    page = 0
    while True:
        batch = client.get_contacts(page=page, page_size=100)
        if not batch:
            break
        contacts.extend(batch)
        page += 1
    print(f"Synced {len(contacts)} contacts")
    
    # 3. Sync Sales
    print("Syncing sales...")
    sales = []
    page = 0
    while True:
        batch = client.get_sales(page=page, page_size=100)
        if not batch:
            break
        sales.extend(batch)
        page += 1
    print(f"Synced {len(sales)} sales")
    
    return {
        "companies": len(companies),
        "contacts": len(contacts),
        "sales": len(sales)
    }


# ==========================================
# FIELD MAPPING REFERENCE
# ==========================================

"""
Common SuperOffice field structures:

1. Simple Values:
   "Name": "Acme Corp"
   "OrgNr": "123456789"

2. List Values (MDO):
   "Category": {"Id": 1, "Value": "Customer"}
   "Country": {"Id": 58, "Value": "Norway"}
   "Currency": {"Id": 1, "Value": "NOK"}

3. Entity References:
   "Contact": {"ContactId": 123}
   "Person": {"PersonId": 456}
   "Associate": {"AssociateId": 7}

4. Nested Objects:
   "Address": {
       "Address1": "Main Street 1",
       "City": "Oslo",
       "PostalCode": "0001",
       "Country": {"Value": "Norway"}
   }

5. Dates:
   ISO 8601 format: "2026-01-15T10:00:00"
   or: "2026-01-15T10:00:00Z"

6. Arrays:
   "Emails": [
       {"Value": "email1@example.com", "Type": "Work"},
       {"Value": "email2@example.com", "Type": "Private"}
   ]
"""

# ==========================================
# BEST PRACTICES
# ==========================================

"""
1. Use REST API for simple operations
   - Faster, less overhead
   - Good for bulk reads
   - Limited to CRUD operations

2. Use Agent API for complex operations
   - Business logic validation
   - Complex workflows
   - Transactions

3. Always use pagination
   - SuperOffice limits results
   - Use $top and $skip (REST)
   - Use page and pageSize (Agent)

4. Handle errors properly
   - Use specific exception types
   - Log errors for debugging
   - Retry on transient failures

5. Cache list data
   - Categories, countries, etc. rarely change
   - Cache for 24 hours
   - Reduces API calls

6. Use OData filters
   - Filter on server side
   - Reduces data transfer
   - Faster queries

7. Batch operations
   - Group related API calls
   - Reduces round trips
   - Better performance

8. Monitor rate limits
   - SuperOffice has rate limits
   - Implement exponential backoff
   - Track API usage
"""
