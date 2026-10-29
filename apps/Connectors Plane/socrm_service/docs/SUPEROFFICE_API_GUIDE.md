# SuperOffice API Integration Guide

## Overview

This service integrates with SuperOffice CRM using the RESTful Web Services API. SuperOffice provides a comprehensive NetServer-based API that enables CRUD operations, advanced search capabilities, and webhook support.

## Architecture

### NetServer Overview

NetServer is SuperOffice's multi-tiered database access layer that provides:
- **Low-level APIs**: OSQL, Row and Rows (onsite only)
- **High-level APIs**: Entities, Archive providers
- **Web Services**: RESTful and SOAP endpoints (preferred for integrations)

### Our Implementation

We use the **RESTful Web Services API**, which is:
- The recommended approach for all modern integrations
- Suitable for both Online and Onsite deployments
- Fully supported across all SuperOffice platforms
- Provides OData-style query capabilities

## Authentication

### OAuth 2.0 Flow

SuperOffice uses standard OAuth 2.0 for authentication:

1. **Authorization**: User is redirected to SuperOffice login
2. **Callback**: SuperOffice returns authorization code
3. **Token Exchange**: Code is exchanged for access/refresh tokens
4. **API Calls**: Access token is used in Bearer authorization header

### Configuration

Set the following environment variables:

```bash
# SuperOffice OAuth credentials
SOCRM_CLIENT_ID=your_client_id
SOCRM_CLIENT_SECRET=your_client_secret
SOCRM_REDIRECT_URI=http://localhost:5006/api/auth/callback
SOCRM_TENANT=online  # or 'sod' for development, 'stage' for testing

# Microsoft Authentication (primary)
MICROSOFT_CLIENT_ID=your_microsoft_client_id
MICROSOFT_CLIENT_SECRET=your_microsoft_client_secret
MICROSOFT_REDIRECT_URI=http://localhost:5006/api/auth/microsoft/callback
MICROSOFT_TENANT_ID=common  # or your specific tenant ID
```

### Token Management

- **Access tokens** expire after 1 hour
- **Refresh tokens** are used to get new access tokens
- Our implementation automatically refreshes tokens when needed
- Tokens are stored securely (in production, use encrypted database storage)

## API Endpoints

### Base URL Structure

The API base URL depends on your environment:

- **Online**: `https://online.superoffice.com/api/v1`
- **Stage**: `https://stage.superoffice.com/api/v1`
- **SOD (Development)**: `https://sod.superoffice.com/api/v1`

### Core Entities

#### Contacts (Person)

```python
# Get all contacts with pagination
contacts = client.get_contacts(page=0, page_size=100)

# Get single contact
contact = client.get_contact(contact_id=123)

# Create new contact
new_contact = client.create_contact({
    "Firstname": "John",
    "Lastname": "Doe",
    "Email": "john.doe@example.com",
    "ContactId": 456  # Associated company
})

# Update contact
updated = client.update_contact(123, {
    "Firstname": "Jane",
    "Email": "jane.doe@example.com"
})
```

#### Companies (Contact)

```python
# Get all companies
companies = client.get_companies(page=0, page_size=100)

# Get single company
company = client.get_company(company_id=456)

# Create company
new_company = client.create_company({
    "Name": "Acme Corp",
    "OrgNr": "123456789",
    "Country": {"Id": 578}  # Norway
})

# Update company
updated = client.update_company(456, {
    "Name": "Acme Corporation Ltd"
})
```

#### Sales (Sale)

```python
# Get all sales
sales = client.get_sales(page=0, page_size=100)

# Get single sale
sale = client.get_sale(sale_id=789)

# Create sale
new_sale = client.create_sale({
    "Heading": "Enterprise License Deal",
    "Amount": 150000,
    "Currency": "NOK",
    "Contact": {"Id": 456},
    "Person": {"Id": 123},
    "SaleDate": "2025-01-15",
    "Status": {"Id": 1}  # Open
})

# Update sale
updated = client.update_sale(789, {
    "Amount": 175000,
    "Status": {"Id": 4}  # Sold
})
```

#### Projects

```python
# Get all projects
projects = client.get_projects(page=0, page_size=100)

# Get single project
project = client.get_project(project_id=321)

# Create project
new_project = client.create_project({
    "Name": "Digital Transformation 2025",
    "ProjectNumber": "DT-2025-001",
    "Status": {"Id": 1}
})

# Update project
updated = client.update_project(321, {
    "Status": {"Id": 4}  # Completed
})
```

#### Documents

```python
# Get all documents
documents = client.get_documents(page=0, page_size=100)

# Get single document
document = client.get_document(document_id=111)

# Create document
new_doc = client.create_document({
    "Header": "Contract Agreement",
    "Contact": {"Id": 456},
    "Person": {"Id": 123}
})
```

#### Appointments (Calendar)

```python
# Get all appointments
appointments = client.get_appointments(page=0, page_size=100)

# Get single appointment
appointment = client.get_appointment(appointment_id=222)

# Create appointment
new_appointment = client.create_appointment({
    "Description": "Project Planning Meeting",
    "StartDate": "2025-01-20T09:00:00",
    "EndDate": "2025-01-20T10:00:00",
    "Contact": {"Id": 456},
    "Associate": {"Id": 10}  # Assigned user
})

# Update appointment
updated = client.update_appointment(222, {
    "StartDate": "2025-01-20T10:00:00",
    "EndDate": "2025-01-20T11:00:00"
})
```

### Advanced Features

#### Archive Providers (Advanced Search)

Archive providers offer powerful search capabilities across multiple entities:

```python
# Search contacts with dynamic filters
results = client.search_archive(
    provider_name="ContactPersonDynamic",
    columns=["personId", "fullName", "email", "contactId"],
    filters={
        "fullName": "John",
        "email": "@acme.com"
    },
    page=0,
    page_size=50
)

# Common archive providers:
# - ContactPersonDynamic: People/contacts
# - CompanyDynamic: Companies
# - SaleDynamic: Sales
# - ProjectDynamic: Projects
# - AppointmentDynamic: Appointments
# - DocumentDynamic: Documents
```

#### MDO Lists (Multi-Database Objects)

SuperOffice uses MDO lists for categories, business types, and other reference data:

```python
# Get list of categories
categories = client.get_list("category")

# Get business types
business_types = client.get_list("business")

# Get countries
countries = client.get_list("country")

# Get currencies
currencies = client.get_list("currency")

# Get user-defined fields for contact
udefs = client.get_list("udcontactsmall")
```

#### User and Associate Operations

```python
# Get current authenticated user
current_user = client.get_current_user()

# Get all users/associates
associates = client.get_associates()
```

#### Selections (Saved Searches)

```python
# Get all selections
selections = client.get_selections()

# Get specific selection
selection = client.get_selection(selection_id=50)

# Get members of a selection
members = client.get_selection_members(selection_id=50, page=0, page_size=100)
```

### Webhooks

SuperOffice supports webhooks for real-time event notifications:

```python
# Create webhook
webhook = client.create_webhook(
    name="Contact Changes Webhook",
    events=["contact.created", "contact.changed", "contact.deleted"],
    target_url="https://your-server.com/webhooks/superoffice",
    secret="your_webhook_secret_key"
)

# Get all webhooks
webhooks = client.get_webhooks()

# Delete webhook
success = client.delete_webhook(webhook_id=10)
```

### Available Webhook Events

- `contact.created`, `contact.changed`, `contact.deleted`
- `person.created`, `person.changed`, `person.deleted`
- `sale.created`, `sale.changed`, `sale.deleted`
- `project.created`, `project.changed`, `project.deleted`
- `appointment.created`, `appointment.changed`, `appointment.deleted`
- `document.created`, `document.changed`, `document.deleted`

## Filtering and Pagination

### OData Query Parameters

SuperOffice supports OData-style queries:

```python
# Pagination
contacts = client.get_contacts(
    page=0,          # Page number (0-indexed)
    page_size=50     # Results per page
)

# Filtering
contacts = client.get_contacts(
    filters={
        "firstname": "John",
        "lastname": "Doe"
    }
)

# Filters are converted to OData $filter parameter:
# $filter=firstname eq 'John' and lastname eq 'Doe'
```

### Common OData Operators

- `eq`: Equal to
- `ne`: Not equal
- `gt`: Greater than
- `ge`: Greater than or equal
- `lt`: Less than
- `le`: Less than or equal
- `contains`: String contains
- `startswith`: String starts with
- `endswith`: String ends with

## Error Handling

Our client provides custom exceptions for common error scenarios:

```python
from clients.socrm_client import (
    SuperOfficeAPIError,
    SuperOfficeAuthError,
    SuperOfficeNotFoundError,
    SuperOfficeValidationError
)

try:
    contact = client.get_contact(123)
except SuperOfficeNotFoundError:
    print("Contact not found")
except SuperOfficeAuthError:
    print("Authentication failed - token may be expired")
except SuperOfficeValidationError as e:
    print(f"Validation error: {e}")
except SuperOfficeAPIError as e:
    print(f"API error: {e}")
```

## Best Practices

### 1. Use Web Services (RESTful)

- **✓ DO**: Use RESTful API for all integrations
- **✗ DON'T**: Mix NetServer Core and Web Services in the same solution
- **✗ DON'T**: Use COM APIs (legacy, Windows-only)

### 2. Token Management

- Store refresh tokens securely (encrypted database)
- Implement automatic token refresh
- Handle 401/403 errors gracefully
- Add 5-minute buffer before token expiry

### 3. Rate Limiting

- Respect API rate limits (typically 60 requests/minute)
- Implement exponential backoff for retries
- Use bulk operations when available
- Cache frequently accessed reference data (MDO lists)

### 4. Pagination

- Always use pagination for list operations
- Default page size: 100 items
- Maximum page size: typically 1000 items
- Use Archive Providers for complex searches

### 5. Error Handling

- Catch and handle specific exceptions
- Log all API errors with context
- Implement retry logic for transient errors (500, 502, 503, 504)
- Don't retry authentication errors (401, 403)

### 6. Data Modeling

- Use proper entity IDs for relationships
- Always specify required fields
- Validate data before sending to API
- Use MDO lists for category/reference fields

### 7. Webhooks

- Verify webhook signatures
- Implement idempotent webhook handlers
- Store webhook payloads for audit
- Handle webhook retries gracefully

## Testing

### Health Check

```python
# Test API connectivity
if client.health_check():
    print("SuperOffice API is accessible")
else:
    print("Cannot reach SuperOffice API")
```

### Development Environment

Use the SOD (SuperOffice Development) tenant for testing:

```bash
SOCRM_TENANT=sod
```

## Performance Optimization

### 1. Caching

Cache reference data that rarely changes:
- MDO lists (categories, business types, countries)
- User/associate lists
- Selection definitions

### 2. Bulk Operations

When syncing large datasets:
- Use pagination with appropriate page sizes
- Implement batch processing
- Use Archive Providers for efficient searches
- Consider off-peak hours for large operations

### 3. Selective Fields

Use `$select` parameter to retrieve only needed fields:

```python
# More efficient - only get specific fields
results = client.search_archive(
    provider_name="ContactPersonDynamic",
    columns=["personId", "fullName", "email"],  # Only these fields
    page_size=100
)
```

## Common Integration Scenarios

### 1. Sync Contacts from Microsoft 365

```python
# Get contacts from Microsoft Graph
ms_contacts = microsoft_graph_client.get_contacts()

# Create in SuperOffice
for ms_contact in ms_contacts:
    so_contact = client.create_contact({
        "Firstname": ms_contact["givenName"],
        "Lastname": ms_contact["surname"],
        "Email": ms_contact["emailAddresses"][0]["address"],
        "MobilePhone": ms_contact.get("mobilePhone"),
        "ContactId": company_id  # Link to company
    })
```

### 2. Create Sale from Opportunity

```python
# Create company first if it doesn't exist
company = client.create_company({
    "Name": "New Customer Corp",
    "Country": {"Id": 578}
})

# Create contact person
contact = client.create_contact({
    "Firstname": "Jane",
    "Lastname": "Smith",
    "Email": "jane.smith@newcustomer.com",
    "ContactId": company["ContactId"]
})

# Create sale
sale = client.create_sale({
    "Heading": "Annual Subscription",
    "Amount": 100000,
    "Currency": "NOK",
    "Contact": {"Id": company["ContactId"]},
    "Person": {"Id": contact["PersonId"]},
    "SaleDate": "2025-02-01",
    "Probability": 75
})
```

### 3. Real-time Notifications with Webhooks

```python
# Setup webhook for contact changes
webhook = client.create_webhook(
    name="Contact Sync Webhook",
    events=["contact.created", "contact.changed"],
    target_url="https://your-server.com/api/webhooks/contacts",
    secret="your_secret_key"
)

# In your webhook handler:
@app.route('/api/webhooks/contacts', methods=['POST'])
def handle_contact_webhook():
    # Verify signature
    signature = request.headers.get('X-SuperOffice-Signature')
    verify_webhook_signature(request.data, signature, webhook_secret)
    
    # Process event
    event_data = request.json
    event_type = event_data['EventName']
    contact_id = event_data['Entity']['PrimaryKey']
    
    if event_type == 'contact.created':
        # Sync new contact to other systems
        sync_new_contact(contact_id)
    elif event_type == 'contact.changed':
        # Update contact in other systems
        sync_updated_contact(contact_id)
    
    return {'status': 'ok'}
```

## Troubleshooting

### Common Issues

#### 401 Unauthorized

- **Cause**: Invalid or expired access token
- **Solution**: Refresh token or re-authenticate

#### 403 Forbidden

- **Cause**: Insufficient permissions
- **Solution**: Check user permissions in SuperOffice, verify scopes

#### 404 Not Found

- **Cause**: Entity doesn't exist or wrong endpoint
- **Solution**: Verify entity ID, check API documentation

#### 429 Too Many Requests

- **Cause**: Rate limit exceeded
- **Solution**: Implement rate limiting, use exponential backoff

#### 500 Internal Server Error

- **Cause**: SuperOffice server error
- **Solution**: Retry with exponential backoff, contact support if persists

### Debugging Tips

1. **Enable verbose logging**:
   ```bash
   LOG_LEVEL=DEBUG
   ```

2. **Check API responses**:
   ```python
   import logging
   logging.basicConfig(level=logging.DEBUG)
   ```

3. **Use SOD environment** for testing without affecting production data

4. **Monitor webhook deliveries** in SuperOffice admin panel

## Resources

- [SuperOffice Developer Portal](https://docs.superoffice.com/)
- [RESTful Web Services API](https://docs.superoffice.com/en/api/reference/restful/rest/index.html)
- [Authentication Guide](https://docs.superoffice.com/en/authentication/)
- [Archive Providers](https://docs.superoffice.com/en/api/archive-providers/)
- [Webhooks Documentation](https://docs.superoffice.com/en/automation/webhook/)

## Support

For API-related questions:
- Email: sdk@superoffice.com
- Developer Community: community.superoffice.com

For application-specific issues:
- Check service logs: `/app/logs/`
- Review error traces in monitoring dashboard
- Contact development team
