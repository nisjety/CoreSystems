# 🔐 Secrets Setup for Production

This directory should contain the cryptographic keys and certificates required for SuperOffice System User authentication.

## Required Files

Place the following files in the `secrets/` directory:

### 1. `privatekey.pem`
Your partner's private RSA key for signing System User tokens.

**Generate:**
```bash
# Without password (easier for development)
openssl genrsa -out privatekey.pem 2048

# With password (recommended for production)
openssl genrsa -aes256 -out privatekey.pem 2048
```

**Permissions:**
```bash
chmod 600 privatekey.pem
```

### 2. `SuperOfficeFederatedLogin.pem`
SuperOffice's public certificate for validating JWT responses.

**Download for SOD (Development):**
```bash
curl -o SuperOfficeFederatedLogin.crt \
  https://sod.superoffice.com/login/common/sod2stage-superoffice-com.crt

# Convert to PEM format
openssl x509 -in SuperOfficeFederatedLogin.crt \
  -out SuperOfficeFederatedLogin.pem \
  -outform PEM
```

**Download for Production:**
```bash
curl -o SuperOfficeFederatedLogin.crt \
  https://online.superoffice.com/login/common/online-superoffice-com.crt

openssl x509 -in SuperOfficeFederatedLogin.crt \
  -out SuperOfficeFederatedLogin.pem \
  -outform PEM
```

## Directory Structure

```
secrets/
├── privatekey.pem                   # Your private RSA key (DO NOT COMMIT)
└── SuperOfficeFederatedLogin.pem   # SuperOffice public cert
```

## Security Notes

### ⚠️ CRITICAL
- **NEVER commit these files to Git**
- Already added to `.gitignore`
- Private keys should be encrypted at rest
- Use environment-specific secrets managers in production

### Docker
The `docker-compose.yml` mounts this directory as **read-only** (`ro`):
```yaml
volumes:
  - ./secrets:/app/secrets:ro
```

### Production Deployment
For production on VPS (31.97.38.31), secure these files:

```bash
# On server
mkdir -p /opt/aquatiq/socrm_service/secrets
cd /opt/aquatiq/socrm_service/secrets

# Copy or generate keys
scp your-local-machine:privatekey.pem .
# Download certificate
curl -o cert.crt https://online.superoffice.com/login/common/online-superoffice-com.crt
openssl x509 -in cert.crt -out SuperOfficeFederatedLogin.pem -outform PEM

# Set proper permissions
chmod 600 privatekey.pem
chmod 644 SuperOfficeFederatedLogin.pem
chown -R root:root .
```

## Verification

Test that files are properly formatted:

```bash
# Verify private key
openssl rsa -in privatekey.pem -check

# Verify certificate
openssl x509 -in SuperOfficeFederatedLogin.pem -text -noout
```

## References

- See [SYSTEM_USER_SETUP_GUIDE.md](../SYSTEM_USER_SETUP_GUIDE.md) for complete setup
- See [SYSTEM_USER_IMPLEMENTATION.md](../SYSTEM_USER_IMPLEMENTATION.md) for usage
