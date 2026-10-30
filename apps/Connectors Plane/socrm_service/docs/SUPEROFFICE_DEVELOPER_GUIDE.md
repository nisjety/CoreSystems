# SuperOffice Online Development Guide

## Overview

This guide covers the complete lifecycle of developing applications for SuperOffice CRM Online, from initial setup through production deployment. It's designed to help you navigate the SuperOffice developer ecosystem and build robust, secure integrations.

## Table of Contents

1. [Understanding the Developer Ecosystem](#1-understanding-the-developer-ecosystem)
2. [Development Environments](#2-development-environments)
3. [Application Lifecycle](#3-application-lifecycle)
4. [Authentication & Authorization](#4-authentication--authorization)
5. [Working with SuperOffice APIs](#5-working-with-superoffice-apis)
6. [Testing & Validation](#6-testing--validation)
7. [Deployment Strategies](#7-deployment-strategies)
8. [Best Practices](#8-best-practices)

---

## 1. Understanding the Developer Ecosystem

### SuperOffice Developer Portal

The **SuperOffice Developer Portal** (https://dev.superoffice.com) is your central hub for:

- 📝 Registering applications
- 👥 Managing organization users
- 📊 Monitoring app usage and installations
- 🔗 Connecting apps to customer tenants
- 📈 Tracking application lifecycle

### Application Types

#### Standard Apps
- Published in the SuperOffice App Store
- Available to any SuperOffice customer
- Requires full certification process
- Includes security review by Telenor Cyberdefense
- Ongoing maintenance and support required

#### Custom Apps
- One-off solutions for specific tenants
- Private deployment
- Quicker validation process (no full certification)
- Ideal for specialized or internal use

**Our Implementation**: The Aquatiq SuperOffice integration is a **custom app** designed for specific organizations.

### Organization Roles

Within a partner organization, users can have different roles:

| Role | Responsibilities |
|------|-----------------|
| **Administrator** | Manage access and permissions |
| **Developer** | Build and maintain applications |
| **Marketing** | Create App Store content |
| **SuperOffice Staff** | Guide apps through testing and launch |

---

## 2. Development Environments

SuperOffice separates development from production using three distinct environments:

### SOD (SuperOffice Online Development)

```
Environment: https://sod.superoffice.com
Purpose: Development and Testing
Cost: Free
```

**Use Cases:**
- Initial development and testing
- Debugging and experimentation
- API exploration
- Safe environment - no risk to production data

**Configuration:**
```bash
SOCRM_TENANT=sod
SOCRM_API_URL=https://sod.superoffice.com/api/v1
```

### Stage

```
Environment: https://stage.superoffice.com
Purpose: Pre-production Testing
Cost: Partner access required
```

**Use Cases:**
- Dress rehearsal before production
- Real-world condition simulation
- Performance testing
- Integration testing with other systems

**Configuration:**
```bash
SOCRM_TENANT=stage
SOCRM_API_URL=https://stage.superoffice.com/api/v1
```

### Production

```
Environment: https://online.superoffice.com
Purpose: Live Customer Use
Cost: Based on customer licenses
```

**Use Cases:**
- Live customer applications
- Production data operations
- Real business transactions

**Configuration:**
```bash
SOCRM_TENANT=online
SOCRM_API_URL=https://online.superoffice.com/api/v1
```

**Important Notes:**
- Each environment has its own credentials and tenants
- Nothing done in SOD can affect production customers
- Always test in SOD before Stage
- Always test in Stage before Production
- Use separate database connections for each environment

---

## 3. Application Lifecycle

### Phase 1: Sign Up as Developer

1. **Create Account**
   - Visit https://dev.superoffice.com
   - Fill out partner profile
   - Submit developer application

2. **Wait for Approval**
   - Manual processing by SuperOffice
   - Confirmation email when approved
   - Access to SOD test tenant granted

3. **Set Up Development Environment**
   ```bash
   # Clone repository
   git clone <your-repo>
   
   # Configure for SOD
   cp .env.example .env
   # Set SOCRM_TENANT=sod
   
   # Install dependencies
   pip install -r requirements.txt
   ```

### Phase 2: Register Your Application

1. **Log in to Developer Portal**
   - https://dev.superoffice.com
   - Navigate to your partner page

2. **Create Application**
   - Click "Create application"
   - Choose application type (Standard or Custom)

3. **Configure Application**
   
   **Required Information:**
   ```
   App Name: Aquatiq SuperOffice Integration
   App Type: Custom App
   Redirect URI: https://your-domain.com/api/auth/callback
   Contact Info: your-email@company.com
   ```

4. **Generate Credentials**
   - Generate Client ID
   - Generate Client Secret
   - **Important**: Store these securely!

5. **Update Configuration**
   ```bash
   # In .env file
   SOCRM_CLIENT_ID=<your_client_id>
   SOCRM_CLIENT_SECRET=<your_client_secret>
   SOCRM_REDIRECT_URI=https://your-domain.com/api/auth/callback
   ```

### Phase 3: Build Your Application

1. **Implement OAuth Flow**
   - Use our existing `oauth_handler.py`
   - Follow OAuth 2.0 / OpenID Connect standards
   - Handle token refresh automatically

2. **Integrate SuperOffice APIs**
   - Use our `socrm_client.py` implementation
   - Follow RESTful API best practices
   - Implement proper error handling

3. **Implement Business Logic**
   - Data synchronization (sync modules)
   - Custom workflows
   - Integration with other systems

4. **Add Security Measures**
   - HTTPS only
   - Input validation
   - Token encryption
   - Webhook signature verification

### Phase 4: Test and Validate

#### For Custom Apps

1. **Test in SOD**
   ```bash
   SOCRM_TENANT=sod
   python test_superoffice_connection.py
   ```

2. **Validation Checklist**
   - [ ] OAuth flow completes successfully
   - [ ] Token handling is secure
   - [ ] API calls work as expected
   - [ ] Error handling is robust
   - [ ] Performance is acceptable
   - [ ] No security vulnerabilities

3. **Submit for Validation**
   - Request validation in Developer Portal
   - Wait for SuperOffice review
   - Address any feedback

#### For Standard Apps (App Store)

1. **Complete SOD Testing**
   - All functionality tested
   - Performance benchmarks met
   - Security review completed internally

2. **Security Review**
   - Conducted by Telenor Cyberdefence
   - Code review
   - Vulnerability assessment
   - Penetration testing

3. **Certification Process**
   - Submit app for certification
   - Address security findings
   - Re-test and re-submit if needed
   - Receive certification approval

### Phase 5: Go Live

#### Custom App Deployment

1. **Production Configuration**
   ```bash
   SOCRM_TENANT=online
   SOCRM_REDIRECT_URI=https://production-domain.com/api/auth/callback
   ```

2. **Deploy to Production Infrastructure**
   - Use production database
   - Enable monitoring and alerting
   - Configure backup strategy

3. **Activate for Customer**
   - Use Tenant Manager in Developer Portal
   - Activate app for specific tenant
   - Provide customer with installation instructions

#### Standard App Launch

1. **App Store Listing**
   - Create marketing content
   - Submit app description
   - Upload screenshots and videos

2. **Publish to App Store**
   - App appears in SuperOffice App Store
   - Available to all customers
   - Marketing users manage listing

3. **Monitor and Support**
   - Track installations via Developer Portal
   - Provide customer support
   - Release updates as needed

---

## 4. Authentication & Authorization

### OAuth 2.0 Flow

SuperOffice uses **OAuth 2.0** and **OpenID Connect** for secure authentication.

#### Standard Flow

```
1. User clicks "Sign in with SuperOffice"
   ↓
2. Redirect to SuperID (Identity Provider)
   URL: https://{env}.superoffice.com/login/common/oauth/authorize
   ↓
3. User logs in and grants permissions
   ↓
4. Redirect back with authorization code
   URL: https://your-app.com/callback?code=xxx&state=xxx
   ↓
5. Exchange code for tokens
   POST: https://{env}.superoffice.com/login/common/oauth/tokens
   ↓
6. Receive tokens:
   - Access Token (API access)
   - Refresh Token (get new access token)
   - ID Token (user identity)
```

#### Implementation in Our Service

```python
from auth.oauth_handler import SuperOfficeOAuthHandler
from config.settings import Settings

settings = Settings()
oauth_handler = SuperOfficeOAuthHandler(settings)

# Step 1: Get authorization URL
auth_url = oauth_handler.get_authorization_url(organization_id)
# Redirect user to auth_url

# Step 2: Handle callback
tokens = oauth_handler.handle_callback(code, state)
# tokens contains: access_token, refresh_token, expires_in

# Step 3: Use access token
access_token = oauth_handler.get_access_token(organization_id)
# Automatically refreshes if expired
```

### Token Management

#### Access Token
- **Lifetime**: 1 hour
- **Purpose**: Authenticate API requests
- **Usage**: Bearer token in Authorization header
- **Refresh**: Use refresh token when expired

```python
# Automatic token refresh in our client
client = SuperOfficeClient(access_token, settings)
# Client automatically uses fresh token
```

#### Refresh Token
- **Lifetime**: Long-lived (months)
- **Purpose**: Get new access tokens
- **Storage**: Encrypted in database
- **Security**: Never expose to client-side code

```python
# Our implementation handles refresh automatically
tokens = oauth_handler._refresh_access_token(organization_id)
```

#### ID Token
- **Format**: JWT (JSON Web Token)
- **Purpose**: User identity verification
- **Contents**: User info, tenant info, claims
- **Validation**: Verify signature and expiry

### Security Best Practices

1. **Token Storage**
   ```python
   # In production: Use encrypted database storage
   # Never store tokens in:
   # - Plain text files
   # - Version control
   # - Client-side storage (browser)
   # - Server logs
   ```

2. **HTTPS Only**
   ```python
   # Always use HTTPS for:
   # - OAuth redirects
   # - API calls
   # - Webhook endpoints
   ```

3. **State Parameter**
   ```python
   # Prevent CSRF attacks
   state = secrets.token_urlsafe(32)
   # Store and verify state in callback
   ```

---

## 5. Working with SuperOffice APIs

### API Options

SuperOffice provides two main API types:

#### RESTful API (Recommended)

```
Base URL: https://{env}.superoffice.com/api/v1
Format: JSON
Authentication: Bearer token
```

**Advantages:**
- Modern, lightweight
- Easy to use from any language
- JSON format
- OData query support
- Best for cloud integrations

**Our Implementation:**
```python
from clients.socrm_client import SuperOfficeClient

client = SuperOfficeClient(access_token, settings)

# Get contacts
contacts = client.get_contacts(page_size=100)

# Create company
company = client.create_company({
    "Name": "Acme Corp",
    "Country": {"Id": 578}
})
```

#### SOAP API (Legacy)

```
URL: https://{env}.superoffice.com/remote/services{version}/
Format: XML
Authentication: Bearer token (in SOAP header)
```

**Use Cases:**
- Legacy .NET integrations
- Enterprise systems requiring SOAP
- Complex batch operations

**Note**: Our implementation focuses on RESTful API for modern development.

### Common Operations

#### Contacts (Persons)

```python
# List contacts
contacts = client.get_contacts(
    page=0,
    page_size=100,
    filters={"email": "@example.com"}
)

# Get single contact
contact = client.get_contact(contact_id=123)

# Create contact
new_contact = client.create_contact({
    "Firstname": "Jane",
    "Lastname": "Doe",
    "Email": "jane.doe@example.com",
    "ContactId": 456  # Link to company
})

# Update contact
updated = client.update_contact(123, {
    "Email": "jane.new@example.com"
})
```

#### Companies (Contacts)

```python
# List companies
companies = client.get_companies(page_size=100)

# Create company
new_company = client.create_company({
    "Name": "Acme Corporation",
    "OrgNr": "123456789",
    "Country": {"Id": 578},  # Norway
    "Category": {"Id": 1}
})
```

#### Sales

```python
# List sales
sales = client.get_sales(
    filters={"status": "Open"}
)

# Create sale
new_sale = client.create_sale({
    "Heading": "Enterprise License Deal",
    "Amount": 150000,
    "Currency": "NOK",
    "Contact": {"Id": 456},
    "Person": {"Id": 123},
    "Probability": 75
})
```

### Advanced Features

#### Archive Providers

Powerful search across multiple entities:

```python
# Search contacts with custom criteria
results = client.search_archive(
    provider_name="ContactPersonDynamic",
    columns=["personId", "fullName", "email", "contactId"],
    filters={
        "fullName": "Smith",
        "email": "@acme.com"
    },
    page_size=50
)
```

**Common Archive Providers:**
- `ContactPersonDynamic` - People/contacts
- `CompanyDynamic` - Companies
- `SaleDynamic` - Sales
- `ProjectDynamic` - Projects
- `AppointmentDynamic` - Appointments

#### MDO Lists

Reference data for categories, types, etc.:

```python
# Get categories
categories = client.get_list("category")

# Get business types
business_types = client.get_list("business")

# Get countries
countries = client.get_list("country")
```

---

## 6. Testing & Validation

### Testing Requirements

**Important**: All applications must be tested in SOD or a production sandbox before going live. This is a requirement for both standard and custom apps.

### Testing Phases

#### Phase 1: Unit Testing

```bash
# Test individual components
pytest tests/test_oauth_handler.py
pytest tests/test_socrm_client.py
```

#### Phase 2: Integration Testing in SOD

```bash
# Set SOD environment
export SOCRM_TENANT=sod

# Run connection tests
python test_superoffice_connection.py

# Run sync tests
pytest tests/test_sync.py
```

#### Phase 3: End-to-End Testing

```python
# Test complete workflows
def test_contact_sync_workflow():
    # 1. Authenticate
    oauth_handler.handle_callback(code, state)
    
    # 2. Fetch from Microsoft 365
    ms_contacts = ms_graph_client.get_contacts()
    
    # 3. Transform data
    so_contacts = transform_contacts(ms_contacts)
    
    # 4. Sync to SuperOffice
    for contact in so_contacts:
        client.create_contact(contact)
    
    # 5. Verify sync
    synced = client.get_contacts()
    assert len(synced) >= len(so_contacts)
```

### Validation Checklist for Custom Apps

- [ ] **Authentication**
  - [ ] OAuth flow completes successfully
  - [ ] Tokens stored securely
  - [ ] Token refresh works automatically
  - [ ] Logout clears all tokens

- [ ] **API Calls**
  - [ ] All CRUD operations work
  - [ ] Pagination handles large datasets
  - [ ] Filters work correctly
  - [ ] Error handling is robust

- [ ] **Security**
  - [ ] HTTPS enabled
  - [ ] Input validation implemented
  - [ ] No credentials in logs
  - [ ] SQL injection prevented
  - [ ] XSS attacks prevented

- [ ] **Performance**
  - [ ] Response times acceptable
  - [ ] No memory leaks
  - [ ] Database queries optimized
  - [ ] Caching implemented

- [ ] **Reliability**
  - [ ] Retry logic works
  - [ ] Graceful degradation
  - [ ] Error recovery
  - [ ] Logging comprehensive

### Certification for Standard Apps

Additional requirements for App Store apps:

- [ ] **Security Review by Telenor Cyberdefence**
  - [ ] Code review passed
  - [ ] Vulnerability assessment passed
  - [ ] Penetration testing passed

- [ ] **Performance Testing**
  - [ ] Load testing completed
  - [ ] Stress testing completed
  - [ ] Benchmark requirements met

- [ ] **Documentation**
  - [ ] User documentation complete
  - [ ] API documentation complete
  - [ ] Installation guide complete

- [ ] **Support Plan**
  - [ ] Support channels defined
  - [ ] SLA documented
  - [ ] Escalation process defined

---

## 7. Deployment Strategies

### Custom App Deployment

#### Step 1: Prepare Production Environment

```bash
# Update configuration
SOCRM_TENANT=online
FLASK_DEBUG=false
LOG_LEVEL=WARNING

# Use production database
DATABASE_URL=postgresql://prod-user:password@prod-db:5432/aquatiq_prod

# Use production Redis
REDIS_URL=redis://prod-redis:6379/0
```

#### Step 2: Deploy Application

```bash
# Build Docker image
docker build -t socrm-service:1.0.0 .

# Push to registry
docker push your-registry/socrm-service:1.0.0

# Deploy with Kubernetes/Docker Compose
kubectl apply -f k8s/deployment.yaml
```

#### Step 3: Activate for Customer

1. Log in to Developer Portal
2. Navigate to Tenant Manager
3. Select customer tenant
4. Activate your application
5. Provide customer with OAuth URL

### Hosting Requirements

You are responsible for hosting your application. SuperOffice requires:

- ✅ **Public-facing OAuth callback page**
- ✅ **HTTPS with valid SSL certificates**
- ✅ **High availability (99.9% uptime)**
- ✅ **Secure infrastructure**
- ✅ **Regular security updates**

**Recommended Platforms:**
- Azure App Service
- AWS Elastic Beanstalk
- Google Cloud Run
- Heroku
- Your own servers

### Configuration Per Environment

```python
# Development (SOD)
config = {
    'tenant': 'sod',
    'debug': True,
    'log_level': 'DEBUG',
    'cache_ttl': 300,  # 5 minutes
}

# Staging
config = {
    'tenant': 'stage',
    'debug': False,
    'log_level': 'INFO',
    'cache_ttl': 1800,  # 30 minutes
}

# Production
config = {
    'tenant': 'online',
    'debug': False,
    'log_level': 'WARNING',
    'cache_ttl': 3600,  # 1 hour
}
```

---

## 8. Best Practices

### Security First

```python
# ✅ DO: Use HTTPS everywhere
SOCRM_REDIRECT_URI=https://your-app.com/callback

# ❌ DON'T: Use HTTP in production
SOCRM_REDIRECT_URI=http://your-app.com/callback

# ✅ DO: Encrypt sensitive data
encrypted_token = encrypt(refresh_token, encryption_key)

# ❌ DON'T: Store tokens in plain text
# Bad: tokens = {"refresh_token": "abc123"}

# ✅ DO: Validate all inputs
def create_contact(data):
    validate_email(data['email'])
    sanitize_input(data['name'])
    # ... then create

# ❌ DON'T: Trust user input
# Bad: sql = f"SELECT * FROM users WHERE email='{email}'"
```

### API Best Practices

```python
# ✅ DO: Call only what you need
contacts = client.get_contacts(
    page_size=20,  # Reasonable size
    filters={"updated_after": last_sync}  # Only new data
)

# ❌ DON'T: Over-fetch data
# Bad: contacts = client.get_contacts(page_size=10000)

# ✅ DO: Use pagination
for page in range(0, total_pages):
    batch = client.get_contacts(page=page, page_size=100)
    process_batch(batch)

# ✅ DO: Implement retry logic with backoff
@retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=1))
def sync_contacts():
    # ... sync logic

# ✅ DO: Cache reference data
@cache(ttl=3600)
def get_categories():
    return client.get_list("category")
```

### Code Maintainability

```python
# ✅ DO: Version your API calls
def get_contact_v1(contact_id):
    # Version 1 logic
    pass

def get_contact_v2(contact_id):
    # Version 2 with improvements
    pass

# ✅ DO: Use type hints
def create_contact(
    data: Dict[str, Any],
    organization_id: str
) -> Dict[str, Any]:
    # Implementation
    pass

# ✅ DO: Document your code
def sync_contacts(batch_size: int = 100) -> int:
    """
    Sync contacts from Microsoft 365 to SuperOffice.
    
    Args:
        batch_size: Number of contacts per batch
        
    Returns:
        Number of contacts synced
    """
    pass
```

### Error Handling

```python
# ✅ DO: Use specific exceptions
try:
    contact = client.get_contact(123)
except SuperOfficeNotFoundError:
    logger.warning("Contact not found")
    # Handle gracefully
except SuperOfficeAuthError:
    logger.error("Authentication failed")
    # Re-authenticate
except SuperOfficeAPIError as e:
    logger.error(f"API error: {e}")
    # Retry or fail gracefully

# ✅ DO: Log with context
logger.error(
    "Failed to sync contact",
    extra={
        "contact_id": contact_id,
        "organization_id": org_id,
        "error": str(e)
    }
)
```

### Stay Compliant

```python
# ✅ Monitor release notes
# Check: https://community.superoffice.com/en/developer/release-notes/

# ✅ Handle API version changes
def api_call_with_fallback():
    try:
        # Try new API version
        return client.new_api_method()
    except SuperOfficeAPIError:
        # Fall back to old version
        return client.old_api_method()

# ✅ Test after updates
@pytest.mark.integration
def test_api_compatibility():
    """Test that our code works with latest API."""
    # ... tests
```

### Test Early and Often

```bash
# ✅ Run tests before committing
pytest tests/

# ✅ Test in SOD regularly
SOCRM_TENANT=sod python test_superoffice_connection.py

# ✅ Monitor in production
# - Set up alerts for errors
# - Track API response times
# - Monitor token refresh rates
```

---

## Resources

### Official Documentation

- 🌐 [Developer Portal](https://dev.superoffice.com)
- 📚 [Developer Documentation](https://docs.superoffice.com)
- 🚀 [Online App Quickstart](https://docs.superoffice.com/en/online/getting-started/)
- 🔐 [Authentication Guide](https://docs.superoffice.com/en/authentication/)
- 📖 [REST API Reference](https://docs.superoffice.com/en/api/reference/restful/)

### Community

- 💬 [Developer Forum](https://community.superoffice.com/en/developer/)
- 📧 [SDK Support](mailto:sdk@superoffice.com)
- 🏪 [App Store](https://appstore.superoffice.com)

### Internal Documentation

- [Quick Start Guide](./QUICK_START.md)
- [API Guide](./SUPEROFFICE_API_GUIDE.md)
- [Architecture](./ARCHITECTURE.md)
- [Setup Checklist](./SUPEROFFICE_CHECKLIST.md)
- [Implementation Summary](./IMPLEMENTATION_SUMMARY.md)

---

## Summary

Developing for SuperOffice CRM Online involves:

1. **Register** at the Developer Portal
2. **Build** using OAuth 2.0 and RESTful APIs
3. **Test** thoroughly in SOD
4. **Validate** (custom) or **Certify** (standard)
5. **Deploy** to production infrastructure
6. **Monitor** and maintain

Following these guidelines ensures a smooth development experience and a secure, reliable integration for your customers.

---

**Last Updated**: December 17, 2025  
**Version**: 1.0  
**Target Audience**: Developers building SuperOffice integrations
