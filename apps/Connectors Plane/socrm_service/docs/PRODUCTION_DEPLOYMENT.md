# 🚀 Production Deployment - System User Setup

Quick guide for setting up System User authentication on production server.

## Server Details
- **VPS**: 31.97.38.31 (Hostinger)
- **Container**: aquatiq-socrm-service
- **Environment**: Production

## Prerequisites Checklist

- [ ] Application registered as "Server to Server" in SuperOffice Developer Portal
- [ ] Admin has performed initial OAuth authorization
- [ ] System User Token extracted from id_token
- [ ] Private RSA key generated
- [ ] SuperOffice public certificate downloaded

## Step 1: Extract System User Token (One-time)

If not done yet, perform initial OAuth authorization:

1. Have a SuperOffice admin visit:
   ```
   https://sod.superoffice.com/login/common/oauth/authorize
     ?client_id=c994f385e7253f1fa1a57578aa17aea6
     &redirect_uri=https://tools.aquatiq.com/api/integrations/socrm/auth/callback
     &response_type=id_token token
     &scope=openid
     &state=setup123
     &nonce=random123
   ```

2. After authorization, copy the `id_token` from the callback URL

3. Extract tokens locally:
   ```bash
   python extract_system_user_token.py "your_id_token_here"
   ```

4. Save the output:
   - `SOCRM_SYSTEM_USER_TOKEN`
   - `SOCRM_CONTEXT_IDENTIFIER`

## Step 2: Generate Private Key (If Not Generated)

On your local machine or server:

```bash
# Generate RSA key (with password for security)
openssl genrsa -aes256 -out privatekey.pem 2048
# Enter a strong password when prompted
```

## Step 3: Download SuperOffice Certificate

For SOD environment:
```bash
curl -o cert.crt \
  https://sod.superoffice.com/login/common/sod2stage-superoffice-com.crt

openssl x509 -in cert.crt \
  -out SuperOfficeFederatedLogin.pem \
  -outform PEM
```

For Production environment (when ready):
```bash
curl -o cert.crt \
  https://online.superoffice.com/login/common/online-superoffice-com.crt

openssl x509 -in cert.crt \
  -out SuperOfficeFederatedLogin.pem \
  -outform PEM
```

## Step 4: Deploy to Production Server

### A. Upload Files to Server

```bash
# SSH to server
ssh root@31.97.38.31

# Create secrets directory
mkdir -p /opt/aquatiq/socrm_service/secrets
cd /opt/aquatiq/socrm_service/secrets

# From local machine, copy private key
scp privatekey.pem root@31.97.38.31:/opt/aquatiq/socrm_service/secrets/

# Download certificate directly on server (already done above)
```

### B. Set Proper Permissions

```bash
# On server
cd /opt/aquatiq/socrm_service/secrets

# Secure private key
chmod 600 privatekey.pem
chown root:root privatekey.pem

# Set certificate permissions
chmod 644 SuperOfficeFederatedLogin.pem
chown root:root SuperOfficeFederatedLogin.pem

# Verify
ls -la
# Should show:
# -rw-------  1 root root 1679 Feb 12 10:00 privatekey.pem
# -rw-r--r--  1 root root 1234 Feb 12 10:00 SuperOfficeFederatedLogin.pem
```

### C. Update .env.production

```bash
cd /opt/aquatiq/socrm_service

# Edit .env.production
nano .env.production
```

Add/update these values:
```bash
# System User Authentication
SOCRM_SYSTEM_USER_TOKEN="YourApp-abc123xyz456..."
SOCRM_CONTEXT_IDENTIFIER="Cust12345"
SOCRM_PRIVATE_KEY_PATH=/app/secrets/privatekey.pem
SOCRM_PRIVATE_KEY_PASSWORD="your_key_password_if_encrypted"
SOCRM_PUBLIC_CERT_PATH=/app/secrets/SuperOfficeFederatedLogin.pem
```

## Step 5: Test Connection

Before deploying, test locally:

```bash
# On local machine with .env configured
python SYSTEM_USER_EXAMPLES.py
# Choose option 1: Basic Authentication
```

## Step 6: Deploy Container

```bash
# On server
cd /opt/aquatiq/socrm_service

# Pull latest code
git pull origin main

# Rebuild container with new dependencies
docker-compose build --no-cache

# Stop existing container
docker-compose down

# Start with new configuration
docker-compose up -d

# Check logs
docker-compose logs -f socrm-service
```

## Step 7: Verify System User Authentication

```bash
# Enter container
docker exec -it aquatiq-socrm-service bash

# Test connection
python3 << 'EOF'
from config.settings import Settings
from auth.system_user_handler import SystemUserHandler
from clients.socrm_system_user_client import SuperOfficeSystemUserClient

settings = Settings()
handler = SystemUserHandler(settings)

print("Testing System User authentication...")
ticket = handler.get_system_user_ticket(
    system_user_token=settings.system_user_token,
    context_identifier=settings.context_identifier
)
print(f"✓ Ticket obtained: {ticket[:30]}...")

client = SuperOfficeSystemUserClient(ticket, settings)
if client.health_check():
    principal = client.get_current_principal()
    print(f"✓ Authenticated as: {principal['FullName']}")
    print("✓ System User authentication working!")
else:
    print("✗ Health check failed")
EOF
```

## Troubleshooting

### Error: "Private key not loaded"
```bash
# Check file exists and permissions
docker exec -it aquatiq-socrm-service ls -la /app/secrets/
docker exec -it aquatiq-socrm-service cat /app/secrets/privatekey.pem
```

### Error: "Token signing failed"
```bash
# Verify key format
openssl rsa -in privatekey.pem -check -passin pass:your_password
```

### Error: "Ticket exchange failed"
```bash
# Verify environment variables are set
docker exec -it aquatiq-socrm-service env | grep SOCRM_SYSTEM
```

### Error: "401 Unauthorized"
```bash
# Ticket expired, invalidate cache and retry
docker-compose restart socrm-service
```

## Environment-Specific Configuration

### SOD (Development/Testing)
```bash
SOCRM_TENANT=sod
SOCRM_PUBLIC_CERT_PATH=/app/secrets/SuperOfficeFederatedLogin_sod.pem
```

### Production
```bash
SOCRM_TENANT=online
SOCRM_PUBLIC_CERT_PATH=/app/secrets/SuperOfficeFederatedLogin_online.pem
```

## Security Checklist

- [ ] Private key stored with 600 permissions
- [ ] Private key is encrypted (password-protected)
- [ ] Private key password stored in environment variable
- [ ] Secrets directory mounted read-only in Docker
- [ ] Environment variables not logged
- [ ] System User Token not hardcoded in code
- [ ] Regular key rotation schedule established

## Monitoring

After deployment, monitor:

```bash
# Check logs for authentication
docker-compose logs -f socrm-service | grep "System User"

# Check for errors
docker-compose logs socrm-service | grep -i error

# Check ticket caching
# Should see "Using cached System User ticket" for subsequent calls
```

## Next Steps

1. **Schedule Background Jobs**
   - Set up cron jobs or Celery tasks
   - Use System User client for non-interactive API access

2. **Implement Data Sync**
   - Create scheduled sync jobs
   - Use ticket caching for efficiency

3. **Set Up Monitoring**
   - Monitor ticket expiration
   - Alert on authentication failures
   - Track API usage

## Support

- See [SYSTEM_USER_IMPLEMENTATION.md](SYSTEM_USER_IMPLEMENTATION.md) for detailed docs
- See [SYSTEM_USER_SETUP_GUIDE.md](SYSTEM_USER_SETUP_GUIDE.md) for complete setup
- See [SYSTEM_USER_EXAMPLES.py](SYSTEM_USER_EXAMPLES.py) for code examples

---

**Status**: Ready for deployment after completing all prerequisites
