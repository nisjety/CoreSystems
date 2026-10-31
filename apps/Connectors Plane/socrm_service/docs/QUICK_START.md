# SuperOffice Integration - Quick Start Guide

## Prerequisites

- Python 3.12+
- PostgreSQL database
- Redis server
- SuperOffice CRM account (Online or Onsite)
- Microsoft Azure AD application (for Microsoft 365 integration)

## Setup Steps

### 1. Clone and Install Dependencies

```bash
cd /Volumes/Lagring/Aquatiq/Aquatiq\ integrasjonen\ /apps/socrm_service

# Create virtual environment
python3 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt
```

### 2. Configure Environment Variables

```bash
# Copy example environment file
cp .env.example .env

# Edit .env with your credentials
nano .env  # or use your preferred editor
```

Required configuration:
- `SOCRM_CLIENT_ID` - SuperOffice application client ID
- `SOCRM_CLIENT_SECRET` - SuperOffice application secret
- `SOCRM_TENANT` - Environment (sod/stage/online)
- `MICROSOFT_CLIENT_ID` - Azure AD application ID
- `MICROSOFT_CLIENT_SECRET` - Azure AD application secret

### 3. Register SuperOffice Application

1. Visit [SuperOffice App Store](https://appstore.superoffice.com/)
2. Navigate to Developer section
3. Create new application:
   - **Name**: Aquatiq Integration
   - **Redirect URI**: `http://localhost:5006/api/auth/callback`
   - **Scopes**: openid, offline_access
4. Save Client ID and Client Secret to `.env`

### 4. Register Azure AD Application

1. Visit [Azure Portal](https://portal.azure.com)
2. Go to Azure Active Directory → App registrations
3. Create new registration:
   - **Name**: Aquatiq SuperOffice Sync
   - **Redirect URI**: `http://localhost:5006/api/auth/microsoft/callback`
4. Add API permissions:
   - Microsoft Graph → Delegated permissions
   - Add: User.Read, Contacts.Read, Calendars.Read, offline_access
5. Create client secret in "Certificates & secrets"
6. Save Application ID and secret to `.env`

### 5. Start Required Services

```bash
# Start PostgreSQL (if not running)
brew services start postgresql  # macOS
# or
sudo service postgresql start  # Linux

# Start Redis (if not running)
brew services start redis  # macOS
# or
sudo service redis-server start  # Linux
```

### 6. Run the Service

```bash
# Development mode
python app.py

# Production mode with Gunicorn
gunicorn --bind 0.0.0.0:5006 --workers 4 app:app
```

## Testing the Integration

### 1. Health Check

```bash
curl http://localhost:5006/health
```

Expected response:
```json
{
  "status": "healthy",
  "service": "socrm_service"
}
```

### 2. Authenticate with SuperOffice

```bash
# Get authorization URL
curl http://localhost:5006/api/auth/authorize

# Open the returned URL in browser and complete OAuth flow
```

### 3. Test API Endpoints

After authentication, test the SuperOffice client:

```python
from clients.socrm_client import SuperOfficeClient
from config.settings import Settings

settings = Settings()
access_token = "your_access_token_from_oauth"

client = SuperOfficeClient(access_token, settings)

# Test connectivity
if client.health_check():
    print("✓ Connected to SuperOffice API")

# Get current user
user = client.get_current_user()
print(f"✓ Authenticated as: {user['FullName']}")

# Get contacts
contacts = client.get_contacts(page_size=10)
print(f"✓ Retrieved {len(contacts)} contacts")

# Get companies
companies = client.get_companies(page_size=10)
print(f"✓ Retrieved {len(companies)} companies")
```

## Common API Operations

### Create a Contact

```python
new_contact = client.create_contact({
    "Firstname": "John",
    "Lastname": "Doe",
    "Email": "john.doe@example.com",
    "MobilePhone": "+47 123 45 678",
    "ContactId": 123  # Company ID
})
print(f"Created contact with ID: {new_contact['PersonId']}")
```

### Create a Company

```python
new_company = client.create_company({
    "Name": "Acme Corporation",
    "OrgNr": "123456789",
    "Country": {"Id": 578},  # Norway
    "Category": {"Id": 1}
})
print(f"Created company with ID: {new_company['ContactId']}")
```

### Create a Sale

```python
new_sale = client.create_sale({
    "Heading": "Enterprise License",
    "Amount": 150000,
    "Currency": "NOK",
    "Contact": {"Id": 123},
    "Person": {"Id": 456},
    "SaleDate": "2025-02-01",
    "Probability": 75,
    "Status": {"Id": 1}  # Open
})
print(f"Created sale with ID: {new_sale['SaleId']}")
```

### Search Using Archive Provider

```python
# Search for contacts
results = client.search_archive(
    provider_name="ContactPersonDynamic",
    columns=["personId", "fullName", "email", "contactId", "contactName"],
    filters={"email": "@acme.com"},
    page_size=50
)

for result in results:
    print(f"{result['fullName']} ({result['email']}) - {result['contactName']}")
```

### Setup Webhook

```python
webhook = client.create_webhook(
    name="Contact Changes",
    events=["contact.created", "contact.changed"],
    target_url="https://your-server.com/webhooks/contacts",
    secret="your_webhook_secret"
)
print(f"Created webhook with ID: {webhook['WebhookId']}")
```

## Docker Deployment

### Build and Run with Docker

```bash
# Build image
docker build -t socrm-service .

# Run container
docker run -d \
  --name socrm-service \
  -p 5006:5006 \
  --env-file .env \
  socrm-service
```

### Docker Compose

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f socrm-service

# Stop services
docker-compose down
```

## Troubleshooting

### Issue: "Authentication failed"

**Solution:**
1. Verify CLIENT_ID and CLIENT_SECRET are correct
2. Check redirect URI matches exactly
3. Ensure token hasn't expired (refresh automatically handled)
4. Verify SOCRM_TENANT is set correctly

### Issue: "Connection refused"

**Solution:**
1. Check if service is running: `curl http://localhost:5006/health`
2. Verify port 5006 is not in use: `lsof -i :5006`
3. Check firewall settings
4. Ensure PostgreSQL and Redis are running

### Issue: "Resource not found (404)"

**Solution:**
1. Verify entity ID exists in SuperOffice
2. Check API endpoint is correct
3. Ensure user has permissions to access resource
4. Confirm SOCRM_TENANT matches your SuperOffice environment

### Issue: "Rate limit exceeded (429)"

**Solution:**
1. Implement exponential backoff
2. Reduce request frequency
3. Use bulk operations where possible
4. Cache frequently accessed data

### Issue: "Token expired"

**Solution:**
- Token refresh is automatic - if it fails:
  1. Re-authenticate using OAuth flow
  2. Check refresh token is stored correctly
  3. Verify token storage in database/cache

## Monitoring and Logging

### View Service Logs

```bash
# Development
tail -f logs/socrm_service.log

# Production (systemd)
journalctl -u socrm-service -f

# Docker
docker logs -f socrm-service
```

### Enable Debug Logging

```bash
# In .env file
LOG_LEVEL=DEBUG

# Restart service
```

### Monitor API Usage

```python
# Check health and stats
response = requests.get('http://localhost:5006/api/stats')
print(response.json())
```

## Development Tips

### 1. Use SOD Environment

Always use SOD (SuperOffice Development) for testing:
```bash
SOCRM_TENANT=sod
```

### 2. Enable Auto-Reload

```bash
# Development mode with auto-reload
FLASK_DEBUG=true python app.py
```

### 3. Test OAuth Flow

```bash
# Start ngrok for local webhook testing
ngrok http 5006

# Update redirect URI in .env to ngrok URL
SOCRM_REDIRECT_URI=https://your-id.ngrok.io/api/auth/callback
```

### 4. Cache Reference Data

```python
# Cache MDO lists for better performance
categories = client.get_list("category")
# Store in Redis with appropriate TTL
```

## Security Checklist

- [ ] CLIENT_SECRET stored securely
- [ ] .env file not in version control
- [ ] HTTPS enabled in production
- [ ] Token storage encrypted
- [ ] Webhook signatures verified
- [ ] Rate limiting implemented
- [ ] Input validation on all endpoints
- [ ] Error messages don't leak sensitive data
- [ ] Logging doesn't include credentials
- [ ] Regular security audits scheduled

## Next Steps

1. **Read Documentation**: See [SUPEROFFICE_API_GUIDE.md](./SUPEROFFICE_API_GUIDE.md) for detailed API reference
2. **Implement Sync Logic**: Check sync modules in `sync/` directory
3. **Setup Webhooks**: Configure real-time synchronization
4. **Configure Monitoring**: Set up alerts for errors and performance issues
5. **Load Testing**: Test with production-like data volumes

## Support

- **SuperOffice SDK**: sdk@superoffice.com
- **Documentation**: https://docs.superoffice.com/
- **Community**: https://community.superoffice.com/
- **Project Issues**: [GitHub Issues](https://github.com/your-org/aquatiq/issues)

## Additional Resources

- [SuperOffice API Guide](./SUPEROFFICE_API_GUIDE.md) - Comprehensive API documentation
- [Integration Status](./IMPLEMENTATION_STATUS.md) - Current implementation status
- [Frontend Integration](./FRONTEND_INTEGRATION.md) - Frontend integration guide
- [Smart Integration](./SMART_INTEGRATION.md) - Advanced integration patterns
