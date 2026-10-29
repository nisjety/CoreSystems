# SuperOffice System User - Setup Guide

## Quick Start Checklist

Follow these steps to set up System User authentication for your SuperOffice integration.

## Step 1: Register Application (If Not Done)

1. Go to [SuperOffice Developer Portal](https://dev.superoffice.com/)
2. Sign in or create developer account
3. Click "Create Application"
4. **Important**: Select **"Server to Server"** application type
5. Fill in application details:
   - **Name**: Your application name
   - **Description**: Brief description
   - **Redirect URLs**: Your OAuth callback URLs
6. Click "Create"
7. Save your credentials:
   - ✅ **Client ID** (Application ID)
   - ✅ **Client Secret** (Application Token)

## Step 2: Generate Private RSA Key

Generate your private key for signing System User tokens:

```bash
# Option 1: Generate without password (easier for development)
openssl genrsa -out privatekey.pem 2048

# Option 2: Generate with password (more secure for production)
openssl genrsa -aes256 -out privatekey.pem 2048
# Enter password when prompted

# Set proper permissions (Unix/Linux/Mac)
chmod 600 privatekey.pem
```

**Store securely**:
- Don't commit to git (add to `.gitignore`)
- For production, use secrets manager (AWS Secrets Manager, Azure Key Vault, etc.)
- For development, store in project root or config folder

## Step 3: Download SuperOffice Public Certificate

Download the appropriate certificate for your environment:

### SOD (Development/Sandbox)
```bash
curl -o SuperOfficeFederatedLogin.crt \
  https://sod.superoffice.com/login/common/sod2stage-superoffice-com.crt
```

### Stage
```bash
curl -o SuperOfficeFederatedLogin.crt \
  https://stage.superoffice.com/login/common/stage-superoffice-com.crt
```

### Production
```bash
curl -o SuperOfficeFederatedLogin.crt \
  https://online.superoffice.com/login/common/online-superoffice-com.crt
```

### Convert to PEM format:
```bash
openssl x509 -in SuperOfficeFederatedLogin.crt \
  -out SuperOfficeFederatedLogin.pem \
  -outform PEM
```

## Step 4: Initial OAuth Authorization (Get System User Token)

You need to perform an initial OAuth authorization to extract the System User Token.

### A. Create OAuth Authorization URL

```python
# create_auth_url.py
import os
from urllib.parse import urlencode

client_id = "YOUR_CLIENT_ID"
redirect_uri = "http://localhost:5005/api/auth/callback"
tenant = "sod"  # or "stage" or "online"

params = {
    "client_id": client_id,
    "redirect_uri": redirect_uri,
    "response_type": "id_token token",  # Important: request both!
    "scope": "openid",
    "state": "setup123",
    "nonce": "random123"
}

auth_url = f"https://{tenant}.superoffice.com/login/common/oauth/authorize?{urlencode(params)}"
print(f"\n🔗 Authorization URL:\n{auth_url}\n")
print("👉 Open this URL in browser as SuperOffice admin")
```

Run:
```bash
python create_auth_url.py
```

### B. Authorize as Admin

1. Open the URL in your browser
2. Log in as SuperOffice administrator
3. Review and approve permissions
4. You'll be redirected to your callback URL with tokens in the URL fragment

### C. Extract System User Token from Callback

Your redirect URL will look like:
```
http://localhost:5005/api/auth/callback#
  access_token=xxx...
  &id_token=eyJhbGc...
  &token_type=Bearer
  &expires_in=3600
```

Extract and decode the `id_token`:

```python
# extract_system_user_token.py
import jwt
import sys

if len(sys.argv) < 2:
    print("Usage: python extract_system_user_token.py <id_token>")
    sys.exit(1)

id_token = sys.argv[1]

# Decode without verification (we just need the claims)
decoded = jwt.decode(
    id_token,
    options={"verify_signature": False}
)

# Extract System User Token
system_user_token = decoded.get(
    "http://schemes.superoffice.net/identity/system_user_token"
)

# Extract Context Identifier (Tenant ID)
context_identifier = decoded.get(
    "http://schemes.superoffice.net/identity/ctx"
)

print("\n✅ SAVE THESE VALUES:\n")
print(f"System User Token: {system_user_token}")
print(f"Context Identifier: {context_identifier}\n")
print("Add to .env file:")
print(f'SOCRM_SYSTEM_USER_TOKEN="{system_user_token}"')
print(f'SOCRM_CONTEXT_IDENTIFIER="{context_identifier}"')
```

Run:
```bash
python extract_system_user_token.py "eyJhbGc..."
```

## Step 5: Configure Environment Variables

Create or update your `.env` file:

```bash
# SuperOffice OAuth Configuration
SOCRM_CLIENT_ID="your_client_id_here"
SOCRM_CLIENT_SECRET="your_client_secret_here"
SOCRM_TENANT="sod"  # or "stage" or "online"

# System User Authentication (Server-to-Server)
SOCRM_SYSTEM_USER_TOKEN="YourApp-abc123xyz456..."
SOCRM_CONTEXT_IDENTIFIER="Cust12345"
SOCRM_PRIVATE_KEY_PATH="/absolute/path/to/privatekey.pem"
SOCRM_PRIVATE_KEY_PASSWORD="your_password"  # Only if key is encrypted
SOCRM_PUBLIC_CERT_PATH="/absolute/path/to/SuperOfficeFederatedLogin.pem"
```

### Important Notes:

- Use **absolute paths** for key/cert files
- If using Docker, ensure files are mounted or copied into container
- For production, use environment-specific secrets management

## Step 6: Install Dependencies

```bash
pip install cryptography
# or if using requirements.txt:
pip install -r requirements.txt
```

## Step 7: Test Connection

Run the example script to verify everything works:

```bash
python SYSTEM_USER_EXAMPLES.py
```

Choose option 1 (Basic Authentication) to test:
- ✅ Key loading
- ✅ Token signing
- ✅ Ticket exchange
- ✅ API connectivity

## Troubleshooting

### Error: "Private key not loaded"

**Cause**: File path incorrect or file doesn't exist

**Solution**:
```bash
# Check file exists
ls -la /path/to/privatekey.pem

# Verify path in .env is absolute
echo $SOCRM_PRIVATE_KEY_PATH

# Test reading file
cat /path/to/privatekey.pem
```

### Error: "Token signing failed"

**Cause**: Invalid key format or corrupted file

**Solution**:
```bash
# Verify key format
openssl rsa -in privatekey.pem -check

# If password-protected, test password
openssl rsa -in privatekey.pem -check -passin pass:your_password
```

### Error: "JWT validation failed"

**Cause**: Wrong public certificate or format issue

**Solution**:
```bash
# Verify certificate format
openssl x509 -in SuperOfficeFederatedLogin.pem -text -noout

# Re-download for your environment
# SOD:
curl -o cert.crt https://sod.superoffice.com/login/common/sod2stage-superoffice-com.crt
openssl x509 -in cert.crt -out SuperOfficeFederatedLogin.pem -outform PEM
```

### Error: "Ticket exchange failed"

**Cause**: Invalid System User Token or Context Identifier

**Solution**:
- Re-check tokens extracted from id_token
- Ensure tokens are from the correct tenant
- Verify admin properly authorized the application
- Try re-authorizing to get fresh tokens

### Error: "401 Unauthorized"

**Cause**: Ticket expired or invalid

**Solution**:
```python
# Force refresh ticket
handler.invalidate_ticket(context_identifier)
ticket = handler.get_system_user_ticket(token, context, force_refresh=True)
```

## Security Best Practices

### Development
- ✅ Use `.gitignore` for keys and `.env` files
- ✅ Use SOD environment for testing
- ✅ Rotate keys periodically

### Production
- ✅ Use secrets manager (AWS Secrets Manager, Azure Key Vault, etc.)
- ✅ Encrypt keys at rest
- ✅ Use environment-specific configurations
- ✅ Implement audit logging
- ✅ Monitor for unauthorized access
- ✅ Set up key rotation schedule

### Example `.gitignore`
```
# Environment and secrets
.env
.env.*
*.pem
*.key
*.crt

# Key files
privatekey.pem
SuperOfficeFederatedLogin.pem
```

## Docker Configuration

If using Docker, mount keys as volumes:

```yaml
# docker-compose.yml
services:
  socrm_service:
    image: socrm_service
    volumes:
      - ./secrets/privatekey.pem:/app/secrets/privatekey.pem:ro
      - ./secrets/SuperOfficeFederatedLogin.pem:/app/secrets/SuperOfficeFederatedLogin.pem:ro
    environment:
      - SOCRM_PRIVATE_KEY_PATH=/app/secrets/privatekey.pem
      - SOCRM_PUBLIC_CERT_PATH=/app/secrets/SuperOfficeFederatedLogin.pem
      - SOCRM_SYSTEM_USER_TOKEN=${SOCRM_SYSTEM_USER_TOKEN}
      - SOCRM_CONTEXT_IDENTIFIER=${SOCRM_CONTEXT_IDENTIFIER}
```

## Multiple Environments

Manage different environments:

```bash
# .env.development
SOCRM_TENANT=sod
SOCRM_SYSTEM_USER_TOKEN="YourApp-dev-token..."
SOCRM_CONTEXT_IDENTIFIER="Cust12345"
SOCRM_PUBLIC_CERT_PATH="/path/to/sod-cert.pem"

# .env.production
SOCRM_TENANT=online
SOCRM_SYSTEM_USER_TOKEN="YourApp-prod-token..."
SOCRM_CONTEXT_IDENTIFIER="Cust67890"
SOCRM_PUBLIC_CERT_PATH="/path/to/online-cert.pem"
```

Load appropriate environment:
```python
from dotenv import load_dotenv
import os

env = os.getenv("ENV", "development")
load_dotenv(f".env.{env}")
```

## Next Steps

Once setup is complete:

1. ✅ Test with example scripts
2. ✅ Implement your background jobs/sync logic
3. ✅ Set up monitoring and logging
4. ✅ Configure scheduled tasks (cron, celery, etc.)
5. ✅ Implement error handling and retry logic
6. ✅ Document your integration flow

## Support

- 📚 [Official Documentation](https://docs.superoffice.com/en/api/netserver/authentication/online/auth-application/)
- 🔧 [Developer Portal](https://dev.superoffice.com/)
- 📖 See `SYSTEM_USER_IMPLEMENTATION.md` for detailed usage
- 💻 See `SYSTEM_USER_EXAMPLES.py` for working code examples

## Summary

You should now have:
- ✅ Application registered as "Server to Server"
- ✅ Private RSA key for signing
- ✅ SuperOffice public certificate for validation
- ✅ System User Token extracted from OAuth
- ✅ Environment variables configured
- ✅ Dependencies installed
- ✅ Connection tested successfully

**You're ready for server-to-server authentication! 🎉**
