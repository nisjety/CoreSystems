# 🔒 Cloudflare Strict Mode Configuration

## Overview

Your Aquatiq infrastructure is now configured for **Cloudflare Strict Mode (Flexible SSL → Strict SSL)**:

- **Cloudflare** (cdn.coresystem.com) ← HTTPS encrypted
- **Origin Server** (31.97.38.31) ← HTTPS encrypted with Cloudflare Origin Certificate
- **End-to-end encryption** ✅

## Configuration Files

### 1. `dynamic.yml` - Traefik TLS Configuration

Traefik is configured to use the Cloudflare Origin Certificate:

```yaml
tls:
  certificates:
    - certFile: /certs/origin-cert.pem
      keyFile: /certs/origin-key.pem
```

**What this does:**
- Routes all HTTPS traffic through the Cloudflare origin certificate
- Enforces TLS 1.2+ for all connections
- Uses strong cipher suites recommended by Cloudflare
- Redirects HTTP → HTTPS

### 2. `middlewares.yml` - Security Headers

Configured with Cloudflare-specific security headers:

- **HSTS** (HTTP Strict Transport Security)
- **X-Content-Type-Options: nosniff**
- **X-Frame-Options: SAMEORIGIN** (clickjacking protection)
- **X-XSS-Protection** (XSS filter)
- **Real IP detection** from Cloudflare headers

### 3. Certificates

| File | Purpose | Expires |
|------|---------|---------|
| `origin-cert.pem` | Cloudflare Origin CA certificate | 2040-11-17 |
| `origin-key.pem` | Private key for origin certificate | 2040-11-17 |

## How Cloudflare Strict Mode Works

```
┌─────────────────────────────────────────────────────────────┐
│  User's Browser                                             │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼ HTTPS (User → Cloudflare)
        ┌──────────────────────────┐
        │   Cloudflare CDN         │
        │   SSL/TLS Encrypted      │
        │   (Cloudflare Cert)      │
        └──────────────┬───────────┘
                       │
                       ▼ HTTPS (Cloudflare → Origin)
        ┌──────────────────────────────────────────┐
        │  Traefik (Reverse Proxy)                 │
        │  SSL/TLS Encrypted                       │
        │  (Cloudflare Origin Certificate)         │
        │  31.97.38.31:443                         │
        └──────────────┬───────────────────────────┘
                       │
                       ▼ Internal (Docker Network)
        ┌──────────────────────────┐
        │  13 Docker Services      │
        │  - PostgreSQL            │
        │  - Redis                 │
        │  - N8N                   │
        │  - Etc.                  │
        └──────────────────────────┘
```

## Cloudflare DNS Configuration

Your DNS records should be configured like this in Cloudflare:

```
Type    | Name        | Content           | Proxy Status
--------|-------------|-------------------|---------------
A       | coresystem.com | 31.97.38.31       | ✓ Proxied (Cloudflare)
A       | *.coresystem.com | 31.97.38.31     | ✓ Proxied (Cloudflare)
MX      | @           | [your MX]         | [not proxied]
TXT     | @           | [your TXT]        | [not proxied]
```

## Traefik Configuration

The following is already configured in `docker-compose.flexible.yml`:

```yaml
traefik:
  command:
    # Trust Cloudflare IPs (for real client IP)
    - "--entrypoints.web.forwardedHeaders.trustedIPs=173.245.48.0/20,..."
    - "--entrypoints.websecure.forwardedHeaders.trustedIPs=173.245.48.0/20,..."
    
    # File provider for dynamic.yml and middlewares.yml
    - "--providers.file.directory=/certs"
    - "--providers.file.watch=true"
    
    # Mount Cloudflare certificates
    volumes:
      - ./cloudflare-certs:/certs:ro
```

## Verification Checklist

### 1. Certificate Files Exist

```bash
ls -la cloudflare-certs/
# Should show:
# -rw-r--r-- origin-cert.pem
# -rw-r--r-- origin-key.pem
# -rw-r--r-- dynamic.yml
# -rw-r--r-- middlewares.yml
```

### 2. Traefik is Running

```bash
ssh root@31.97.38.31
cd /opt/coresystem && docker compose ps | grep traefik

# Should show: coresystem-traefik is running
```

### 3. Certificates are Loaded

```bash
docker compose logs traefik | grep -i "cert\|tls"

# Should show certificate loading messages
```

### 4. Test HTTPS Connection

```bash
# Test from local machine
curl -v https://coresystem.com

# Should show:
# ✓ TLS connection established
# ✓ Certificate: *.coresystem.com
# ✓ Issuer: Cloudflare Origin CA
```

### 5. Check Certificate Details

```bash
openssl x509 -in cloudflare-certs/origin-cert.pem -text -noout | grep -A 5 "Subject:\|Issuer:\|Validity"

# Should show:
# Subject: CN=*.coresystem.com
# Issuer: CN=Cloudflare Origin CA
# Not After: Nov 17 00:00:00 2040
```

## Services Now Accessible

Once deployed, these services will be accessible via HTTPS:

| Service | URL | Auth |
|---------|-----|------|
| Traefik Dashboard | https://traefik.coresystem.com | Basic Auth (Cloudflare) |
| pgAdmin | https://pgadmin.coresystem.com | Email + Password |
| Grafana | https://grafana.coresystem.com | Admin + Password |
| n8n | https://n8n.coresystem.com | n8n setup |
| App | https://app.coresystem.com | None (public) |
| Aquatiq Gateway API | https://admin.coresystem.com | API Key |

## Security Features Enabled

✅ **End-to-End HTTPS Encryption**
- Browser → Cloudflare (encrypted)
- Cloudflare → Origin (encrypted with origin certificate)

✅ **TLS 1.2+ Only**
- No outdated SSL/TLS versions

✅ **Strong Cipher Suites**
- AES-256-GCM preferred
- ECDHE for forward secrecy

✅ **Security Headers**
- HSTS (31536000 seconds = 1 year)
- X-Content-Type-Options: nosniff
- X-Frame-Options: SAMEORIGIN
- Referrer-Policy: strict-origin-when-cross-origin

✅ **DDoS Protection**
- Cloudflare handles DDoS/WAF
- Rate limiting on API endpoints

✅ **Real Client IP Detection**
- Traefik trusts Cloudflare headers
- Proper logging of real IPs

## Troubleshooting

### Certificate Not Loading

```bash
ssh root@31.97.38.31
cd /opt/coresystem

# Check if files exist
ls -la cloudflare-certs/

# Check Traefik logs
docker compose logs traefik --tail 50

# Restart Traefik
docker compose restart traefik
```

### HTTPS Connection Fails

```bash
# Test certificate validity
openssl s_client -connect 31.97.38.31:443 -servername coresystem.com

# Should show certificate details without errors
```

### Cloudflare Still Shows SSL Error

In Cloudflare Dashboard:
1. Go to SSL/TLS
2. Ensure **Full (Strict)** mode is selected
3. Verify DNS A record is proxied (orange cloud)
4. Check Origin Server IP (31.97.38.31)

## Certificate Renewal

The Cloudflare Origin Certificate is valid until **November 17, 2040**.

When renewal is needed:
1. Log in to Cloudflare Dashboard
2. Go to SSL/TLS → Origin Server
3. Create new certificate
4. Replace `origin-cert.pem` and `origin-key.pem`
5. Restart Traefik container

## Next Steps

1. **Verify deployment**: `ssh root@31.97.38.31 && docker compose ps`
2. **Test HTTPS**: `curl -v https://coresystem.com`
3. **Monitor logs**: `docker compose logs -f traefik`
4. **Configure DNS**: Ensure all domains point to 31.97.38.31 and are proxied in Cloudflare

---

**Status**: ✅ Cloudflare Strict Mode configured and ready for production!
