# SuperOffice API Integration

Complete integration with SuperOffice CRM using both REST API and Agent API.

## 📋 Table of Contents

- [Overview](#overview)
- [API Architecture](#api-architecture)
- [Quick Start](#quick-start)
- [Available Clients](#available-clients)
- [Common Operations](#common-operations)
- [Examples](#examples)
- [Configuration](#configuration)
- [Best Practices](#best-practices)

## 🎯 Overview

This integration provides comprehensive access to SuperOffice CRM through two complementary APIs:

### REST API (`socrm_client.py`)
- **Purpose**: Simple CRUD operations on entities
- **Use For**: Basic reads, lists, simple updates
- **Features**: OData support, pagination, filtering

### Agent API (`socrm_agent_client.py`)
- **Purpose**: Complex business logic operations
- **Use For**: Complex workflows, validations, transactions
- **Features**: Full entity access, business rules, advanced operations

## 🏗️ API Architecture

### Entity Mapping

SuperOffice uses non-standard naming conventions:

| Standard Term | SuperOffice REST | SuperOffice Agent | Entity Type |
|--------------|------------------|-------------------|-------------|
| **Company** | `/Contact` | `ContactAgent` | `ContactEntity` |
| **Contact/Person** | `/Person` | `PersonAgent` | `PersonEntity` |
| **Sale** | `/Sale` | `SaleAgent` | `SaleEntity` |
| **Project** | `/Project` | `ProjectAgent` | `ProjectEntity` |
| **Appointment** | `/Appointment` | `AppointmentAgent` | `AppointmentEntity` |
| **Document** | `/Document` | `DocumentAgent` | `DocumentEntity` |

⚠️ **Important**: Companies are accessed via `/Contact`, not `/Company`!

### Available Endpoints

Based on `Swagger-v1-REST.json` and `Swagger-v1-Agents.json`:

#### Core Entity Endpoints (REST API)
```
/api/v1/Contact          - Companies (ContactEntity)
/api/v1/Person           - Contacts (PersonEntity)
/api/v1/Sale             - Sales (SaleEntity)
/api/v1/Project          - Projects
/api/v1/Appointment      - Appointments
/api/v1/Document         - Documents
/api/v1/Selection        - Selections (saved searches)
/api/v1/Relation         - Relations
```

#### Agent API Endpoints
```
/api/v1/Agents/Contact/*        - Contact operations
/api/v1/Agents/Person/*         - Person operations
/api/v1/Agents/Sale/*           - Sale operations
/api/v1/Agents/Project/*        - Project operations
/api/v1/Agents/Appointment/*    - Appointment operations
/api/v1/Agents/Document/*       - Document operations
/api/v1/Agents/Archive/*        - Advanced search
/api/v1/Agents/Selection/*      - Selection management
/api/v1/Agents/List/*           - List operations
/api/v1/Agents/User/*           - User management
/api/v1/Agents/Find/*           - Search operations
```

## 🚀 Quick Start

### 1. Initialize Client

```python
from config.settings import Settings
from clients.socrm_client import SuperOfficeClient
from clients.socrm_agent_client import SuperOfficeAgentClient

settings = Settings()

# For simple operations
client = SuperOfficeClient(access_token, settings)

# For complex operations
agent_client = SuperOfficeAgentClient(access_token, settings)
```

### 2. Fetch Data

```python
# Get companies (simple)
companies = client.get_companies(page=0, page_size=100)

# Get company with all persons (complex)
company = agent_client.get_contact_with_persons(contact_id=123)

# Get contacts
contacts = client.get_contacts(page=0, page_size=100)

# Get sales
sales = client.get_sales(page=0, page_size=100)
```

### 3. Create Data

```python
# Create company
company_data = {
    "Name": "Acme Corporation",
    "Country": {"Value": "Norway"},
    "Category": {"Value": "Customer"}
}
new_company = client.create_company(company_data)

# Create person
person = agent_client.create_default_person_entity()
person["Firstname"] = "John"
person["Lastname"] = "Doe"
person["Contact"] = {"ContactId": 123}
saved_person = agent_client.save_person_entity(person)
```

## 📦 Available Clients

### SuperOfficeClient (REST API)

Simple CRUD operations with OData support.

**Methods:**
- `get_companies(page, page_size, filters)` - List companies
- `get_company(company_id)` - Get single company
- `create_company(company_data)` - Create company
- `update_company(company_id, company_data)` - Update company
- `get_contacts(page, page_size, filters)` - List contacts
- `get_contact(contact_id)` - Get single contact
- `create_contact(contact_data)` - Create contact
- `update_contact(contact_id, contact_data)` - Update contact
- `get_sales(page, page_size, filters)` - List sales
- `get_sale(sale_id)` - Get single sale
- `create_sale(sale_data)` - Create sale
- `update_sale(sale_id, sale_data)` - Update sale
- `get_projects(page, page_size, filters)` - List projects
- `get_appointments(page, page_size, filters)` - List appointments
- `get_documents(page, page_size, filters)` - List documents
- `search_archive(provider_name, columns, filters)` - Advanced search
- `get_list(list_name)` - Get MDO list items

### SuperOfficeAgentClient (Agent API)

Complex operations with business logic validation.

**Contact Agent:**
- `create_default_contact_entity()` - New company with defaults
- `get_contact_entity(contact_id)` - Full company entity
- `save_contact_entity(contact_entity)` - Save company
- `delete_contact_entity(contact_id)` - Delete company
- `get_contact_with_persons(contact_id)` - Company with all persons

**Person Agent:**
- `create_default_person_entity()` - New person with defaults
- `get_person_entity(person_id)` - Full person entity
- `save_person_entity(person_entity)` - Save person
- `delete_person_entity(person_id)` - Delete person
- `get_person_list(person_ids)` - Multiple persons

**Sale Agent:**
- `create_default_sale_entity()` - New sale with defaults
- `get_sale_entity(sale_id)` - Full sale entity
- `save_sale_entity(sale_entity)` - Save sale
- `delete_sale_entity(sale_id)` - Delete sale
- `set_as_lost(sale_id)` - Mark sale as lost
- `set_as_sold(sale_id)` - Mark sale as sold
- `get_sale_stakeholders(sale_id)` - Get stakeholders

**Project Agent:**
- `create_default_project_entity()` - New project
- `get_project_entity(project_id)` - Full project entity
- `save_project_entity(project_entity)` - Save project
- `get_project_members(project_id)` - Get members

**Appointment Agent:**
- `create_default_appointment_entity()` - New appointment
- `get_appointment_entity(appointment_id)` - Full appointment
- `save_appointment_entity(appointment_entity)` - Save appointment
- `accept_appointment(appointment_id)` - Accept invitation
- `decline_appointment(appointment_id)` - Decline invitation

**Document Agent:**
- `create_default_document_entity()` - New document
- `get_document_entity(document_id)` - Full document
- `save_document_entity(document_entity)` - Save document
- `get_document_stream(document_id)` - Get content

**Archive Agent:**
- `get_archive_list_by_columns()` - Advanced search

**Selection Agent:**
- `create_default_selection_entity()` - New selection
- `save_selection_entity(selection_entity)` - Save selection
- `add_contact_selection_members()` - Add members
- `remove_contact_selection_members()` - Remove members

**Search Methods:**
- `find_contact(name, department, orgnr, phone)` - Search companies
- `find_person(firstname, lastname, phone, email)` - Search persons

## 💡 Common Operations

### Get All Companies

```python
def get_all_companies(client):
    all_companies = []
    page = 0
    page_size = 100
    
    while True:
        companies = client.get_companies(page=page, page_size=page_size)
        if not companies:
            break
        all_companies.extend(companies)
        page += 1
    
    return all_companies
```

### Filter Companies

```python
# Using REST API (OData)
norwegian_customers = client.get_companies(
    page=0,
    page_size=100,
    filters={
        "Country": "Norway",
        "Category": "Customer"
    }
)

# Using Agent API (Archive)
results = agent_client.get_archive_list_by_columns(
    provider_name="ContactDynamic",
    columns=["contactId", "name", "country", "category"],
    restriction="country = 'Norway' AND category = 'Customer'",
    page=0,
    page_size=100
)
```

### Create Company with Person

```python
# Step 1: Create company
company = agent_client.create_default_contact_entity()
company["Name"] = "Acme Corporation"
company["Country"] = {"Value": "Norway"}
saved_company = agent_client.save_contact_entity(company)

# Step 2: Create person
person = agent_client.create_default_person_entity()
person["Firstname"] = "John"
person["Lastname"] = "Doe"
person["Contact"] = {"ContactId": saved_company["ContactId"]}
person["Email"] = "john@acme.com"
saved_person = agent_client.save_person_entity(person)
```

### Create Sale

```python
sale = agent_client.create_default_sale_entity()
sale["Heading"] = "Enterprise Deal"
sale["Amount"] = 500000
sale["Currency"] = {"Value": "NOK"}
sale["Contact"] = {"ContactId": 123}
sale["Person"] = {"PersonId": 456}
sale["SaleDate"] = "2026-06-01"
saved_sale = agent_client.save_sale_entity(sale)
```

### Search Contacts

```python
# Using Find Agent
results = agent_client.find_person(
    firstname="John",
    email="@acme.com"
)

# Using Archive Provider (more powerful)
results = agent_client.get_archive_list_by_columns(
    provider_name="PersonDynamic",
    columns=["personId", "fullName", "email", "contactName"],
    restriction="email contains '@acme.com'",
    page=0,
    page_size=50
)
```

## 🔧 Configuration

### Environment Variables

```bash
SUPEROFFICE_CLIENT_ID=your_client_id
SUPEROFFICE_CLIENT_SECRET=your_client_secret
SUPEROFFICE_REDIRECT_URI=http://localhost:8000/callback
SUPEROFFICE_BASE_URL=https://sod.superoffice.com
```

### Settings

```python
from config.settings import Settings

settings = Settings()
settings.superoffice_base_url  # API base URL
settings.request_timeout       # Request timeout (default: 30s)
```

## 📚 Examples

See [SUPEROFFICE_API_EXAMPLES.py](./SUPEROFFICE_API_EXAMPLES.py) for complete code examples:

- Example 1: Get companies (simple)
- Example 2: Get companies (advanced)
- Example 3: Create company (REST)
- Example 4: Create person
- Example 5: Create sale
- Example 6: Create appointment
- Example 7: Search using Archive Provider
- Example 8: Working with lists (MDO)
- Example 9: Manage selections
- Example 10: Bulk operations
- Example 11: Error handling
- Example 12: Complete sync flow

## ✅ Best Practices

### 1. Choose the Right API

- **Use REST API** for:
  - Simple reads (list companies, get sale)
  - Bulk data retrieval
  - Basic CRUD operations

- **Use Agent API** for:
  - Complex business logic
  - Creating entities with defaults
  - Operations requiring validation
  - Transactions

### 2. Pagination

Always use pagination for large datasets:

```python
page = 0
page_size = 100

while True:
    batch = client.get_companies(page=page, page_size=page_size)
    if not batch:
        break
    
    # Process batch
    process_companies(batch)
    
    page += 1
```

### 3. Error Handling

```python
from clients.socrm_client import (
    SuperOfficeAPIError,
    SuperOfficeAuthError,
    SuperOfficeNotFoundError,
    SuperOfficeValidationError
)

try:
    company = client.get_company(123)
except SuperOfficeNotFoundError:
    # Handle not found
    pass
except SuperOfficeAuthError:
    # Re-authenticate
    pass
except SuperOfficeValidationError:
    # Fix data and retry
    pass
```

### 4. Caching

Cache list data (categories, countries, etc.):

```python
from cache.cache_manager import CacheManager

cache = CacheManager()

# Cache for 24 hours
categories = cache.get_or_set(
    key="categories",
    fn=lambda: client.get_list("Category"),
    ttl=86400
)
```

### 5. Field Structure

SuperOffice uses specific structures for fields:

```python
# Simple values
{"Name": "Acme Corp"}

# List values (MDO)
{"Category": {"Id": 1, "Value": "Customer"}}
{"Country": {"Id": 58, "Value": "Norway"}}

# Entity references
{"Contact": {"ContactId": 123}}
{"Person": {"PersonId": 456}}

# Dates
{"SaleDate": "2026-01-15T10:00:00"}
```

### 6. Bulk Operations

For syncing large amounts of data:

```python
def sync_all_companies():
    all_companies = []
    page = 0
    
    while True:
        batch = client.get_companies(page=page, page_size=100)
        if not batch:
            break
        
        # Transform and load to BigQuery
        for company in batch:
            transformed = transform_company(company)
            bigquery_client.insert_row("companies", transformed)
        
        all_companies.extend(batch)
        page += 1
    
    return all_companies
```

### 7. Rate Limiting

Implement exponential backoff:

```python
import time
from requests.exceptions import HTTPError

def retry_with_backoff(fn, max_retries=3):
    for attempt in range(max_retries):
        try:
            return fn()
        except HTTPError as e:
            if e.response.status_code == 429:  # Rate limited
                wait_time = 2 ** attempt
                time.sleep(wait_time)
            else:
                raise
    
    raise Exception("Max retries exceeded")
```

## 🔗 References

- [SuperOffice Developer Portal](https://community.superoffice.com/en/developer/)
- [REST API Documentation](https://docs.superoffice.com/)
- [Agent API Documentation](https://docs.superoffice.com/)
- Swagger Files:
  - `config/Swagger-v1-REST.json` - REST API specification
  - `config/Swagger-v1-Agents.json` - Agent API specification

## 📝 Notes

### Important Naming Conventions

- **Companies** are called **Contacts** in SuperOffice
- **Contacts/Persons** are called **Persons** in SuperOffice
- Always use the SuperOffice naming in API calls
- Use standard naming in your application layer

### Data Structure

- SuperOffice uses **PascalCase** for field names
- List values are **nested objects**: `{"Value": "Norway"}`
- Entity references use specific ID fields: `{"ContactId": 123}`
- Dates use **ISO 8601** format: `"2026-01-15T10:00:00"`

### Limitations

- SuperOffice has rate limits (monitor usage)
- Large datasets require pagination
- Some operations only work with Agent API
- Document content requires special handling

## 🆘 Troubleshooting

### Authentication Issues

```python
# Check if token is valid
try:
    user = client.get_current_user()
    print(f"Authenticated as: {user}")
except SuperOfficeAuthError:
    # Token expired or invalid - re-authenticate
    pass
```

### Not Found Errors

```python
# Remember: Companies are at /Contact, not /Company
try:
    company = client.get_company(123)  # Calls /Contact/123
except SuperOfficeNotFoundError:
    print("Company not found")
```

### Validation Errors

```python
# Use Agent API defaults to ensure valid structure
try:
    company = agent_client.create_default_contact_entity()
    company["Name"] = "Acme"
    saved = agent_client.save_contact_entity(company)
except SuperOfficeValidationError as e:
    print(f"Validation failed: {e}")
```

---

**Version**: 1.0.0  
**Last Updated**: January 2026  
**Maintainer**: Aquatiq Integration Team
