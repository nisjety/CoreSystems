# Aquatiq Root Container - Security Documentation

## 🔒 Security Overview

This document details the security architecture, best practices, and compliance measures implemented in the Aquatiq Root Container system.

---

## Table of Contents

1. [Security Architecture](#security-architecture)
2. [Network Security](#network-security)
3. [Secrets Management](#secrets-management)
4. [Access Control](#access-control)
5. [SSL/TLS Configuration](#ssltls-configuration)
6. [Firewall Rules](#firewall-rules)
7. [Rate Limiting](#rate-limiting)
8. [Audit Logging](#audit-logging)
9. [Security Checklist](#security-checklist)
10. [Incident Response](#incident-response)

---

## Security Architecture

### Defense in Depth

```
┌─────────────────────────────────────────────────────────┐
│                    Cloudflare CDN                       │
│  - DDoS Protection    - SSL/TLS Termination            │
│  - Bot Management     - Rate Limiting (Edge)           │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                   UFW Firewall (VPS)                    │
│  - Cloudflare IP Whitelist Only                        │
│  - SSH: Port 22 (Key-based only)                       │
│  - HTTP/HTTPS: Cloudflare IPs only                     │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                  Traefik Reverse Proxy                  │
│  - Middleware: Rate Limiting                           │
│  - Middleware: IP Allow List                           │
│  - Middleware: Security Headers                        │
│  - Middleware: Basic Auth (Admin endpoints)            │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│              Docker Socket Proxy                        │
│  - Read-Only Docker API Access                         │
│  - Limited Operations (CONTAINERS=1, IMAGES=1)         │
│  - No Privileged Operations                            │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                  Application Services                   │
│  - PostgreSQL (Internal network only)                  │
│  - Redis (Password-protected)                          │
│  - NATS (Token authentication)                         │
│  - Gateway (API key required)                          │
└─────────────────────────────────────────────────────────┘
```

---

## Network Security

### Network Segmentation

**Production Networks:**
```yaml
internal:          # Management layer
  - traefik
  - docker-socket-proxy
  - monitoring services

coresystem-backend:   # Application layer
  - postgres
  - redis
  - nats
  - n8n
  - gateway
```

**Development Networks:**
```yaml
coresystem-local:     # Company projects
ima-local:         # Personal projects
```

### Network Policies

1. **No Direct External Access**: Only Traefik exposes ports 80/443
2. **Service Isolation**: Databases not accessible from internet
3. **Read-Only Docker Socket**: Limited API operations via proxy
4. **Network Segregation**: Dev networks isolated from production

---

## Secrets Management

### Docker Secrets (Production)

All sensitive credentials stored as Docker secrets:

```bash
# Secrets are mounted as read-only files
/run/secrets/
├── postgres_password      # 32+ char random
├── redis_password         # 32+ char random
├── nats_auth_token       # 32+ char random
├── n8n_encryption_key    # 32+ char random
└── traefik_dashboard_auth # htpasswd format
```

**Generation:**
```bash
# Strong password generation
openssl rand -base64 32 > secrets/postgres_password.txt
openssl rand -base64 32 > secrets/redis_password.txt

# Basic auth for Traefik dashboard
htpasswd -nb admin <password> > secrets/traefik_dashboard_auth.txt
```

**Permissions:**
```bash
# Secrets must be readable by Docker daemon
chmod 644 secrets/*.txt

# But directory should be restricted
chmod 700 secrets/
```

### Local Development Credentials

**Simple passwords for development ONLY:**
```env
POSTGRES_PASSWORD=postgres
REDIS_PASSWORD=redis
NATS_AUTH_TOKEN=nats
```

⚠️ **Never use development credentials in production!**

### Credential Rotation

**Schedule:**
- Database passwords: Every 90 days
- API keys: Every 60 days
- Service tokens: Every 30 days

**Process:**
1. Generate new credential
2. Update secret file
3. Recreate affected containers
4. Verify service health
5. Revoke old credential

---

## Access Control

### IP Whitelisting

**Traefik Middleware** (`middlewares.yml`):
```yaml
dynamic-ipwhitelist:
  ipAllowList:
    sourceRange:
      - "127.0.0.1/32"          # Localhost
      - "77.106.153.146/32"     # Your IP
      # Add trusted IPs here
```

**Protected Endpoints:**
- `admin.coresystem.com` - Gateway API
- `traefik.coresystem.com` - Traefik Dashboard
- `pgadmin.coresystem.com` - Database Admin
- `redis.coresystem.com` - Redis Admin

### Authentication Layers

1. **Cloudflare**: Edge authentication (optional)
2. **Traefik**: IP whitelist + Basic Auth
3. **Application**: API keys, session tokens
4. **Database**: Password authentication
5. **n8n**: User accounts with 2FA support

### Service Authentication Matrix

| Service | Auth Method | Credential Location |
|---------|-------------|---------------------|
| PostgreSQL | Password | Docker secret |
| Redis | Password | Docker secret |
| NATS | Token | Docker secret |
| n8n | User account + encryption key | Docker secret |
| Gateway API | API key | Environment variable |
| Traefik Dashboard | Basic Auth | Docker secret |

---

## SSL/TLS Configuration

### Cloudflare Full (Strict) Mode

**Requirements:**
- Valid Cloudflare Origin Certificate
- Private key for certificate
- Certificate valid for `*.coresystem.com`

**Certificate Details:**
```
Subject: CloudFlare Origin Certificate
Issuer: CloudFlare Origin SSL Certificate Authority
Valid: Nov 21 2025 - Nov 17 2040 (15 years)
SANs: *.coresystem.com, coresystem.com
```

**Traefik Configuration:**
```yaml
# cloudflare-certs/dynamic.yml
tls:
  certificates:
    - certFile: /certs/origin-cert.pem
      keyFile: /certs/origin-key.pem
  stores:
    default:
      defaultCertificate:
        certFile: /certs/origin-cert.pem
        keyFile: /certs/origin-key.pem
```

### SSL/TLS Best Practices

✅ **Enabled:**
- TLS 1.2+ only
- Strong cipher suites
- HSTS (max-age=31536000)
- Certificate pinning via Cloudflare

❌ **Disabled:**
- SSLv3, TLS 1.0, TLS 1.1
- Weak ciphers (RC4, DES)
- Self-signed certificates in production

---

## Firewall Rules

### UFW Configuration (Ubuntu)

**Allow Cloudflare IPs Only:**
```bash
# Enable UFW
ufw enable

# Allow SSH
ufw allow 22/tcp comment "SSH access"

# Allow Cloudflare IPs for HTTP/HTTPS
ufw allow from 173.245.48.0/20 to any port 80 proto tcp comment "Cloudflare HTTP"
ufw allow from 173.245.48.0/20 to any port 443 proto tcp comment "Cloudflare HTTPS"

# Repeat for all Cloudflare IP ranges
# (See complete list in deployment docs)

# Allow NTP
ufw allow 123/udp comment "NTP server"

# Deny all other incoming
ufw default deny incoming
ufw default allow outgoing
```

**Verify:**
```bash
ufw status verbose
```

### Port Security

| Port | Binding | Access |
|------|---------|--------|
| 22 | 0.0.0.0 | SSH (key-based) |
| 80 | 0.0.0.0 | Cloudflare IPs only |
| 443 | 0.0.0.0 | Cloudflare IPs only |
| 5432 | Internal | Docker network |
| 6379 | Internal | Docker network |
| 4222 | Internal | Docker network |

---

## Rate Limiting

### Traefik Middleware Configuration

**Global Rate Limit:**
```yaml
global-ratelimit:
  rateLimit:
    average: 100      # Requests per period
    period: "1m"      # Time window
    burst: 50         # Burst capacity
```

**Admin Endpoints:**
```yaml
admin-ratelimit:
  rateLimit:
    average: 20       # Stricter limit
    period: "1m"
    burst: 10
```

### Rate Limit Response

**Headers:**
```
X-Rate-Limit: 100
X-Rate-Limit-Remaining: 45
X-Rate-Limit-Reset: 1732547200
```

**Exceeded Response:**
```
HTTP/1.1 429 Too Many Requests
Retry-After: 30
Content-Type: application/json

{"error": "Rate limit exceeded"}
```

---

## Audit Logging

### Gateway Audit Log

**Captured Events:**
- API requests (method, path, IP, timestamp)
- Database operations (create, read, update, delete)
- Docker operations (start, stop, inspect)
- Authentication attempts (success/failure)
- IP whitelist changes

**Log Format:**
```json
{
  "timestamp": "2025-11-25T14:32:15Z",
  "level": "info",
  "event": "api_request",
  "method": "POST",
  "path": "/api/v1/database/create",
  "client_ip": "77.106.153.146",
  "user_agent": "curl/7.88.1",
  "status_code": 200,
  "duration_ms": 42
}
```

### Traefik Access Logs

**Location:** `/var/log/traefik/access.log`

**Format:** JSON
```json
{
  "ClientHost": "77.106.153.146",
  "RequestMethod": "GET",
  "RequestPath": "/health",
  "OriginStatus": 200,
  "RequestProtocol": "HTTP/2.0",
  "Duration": 1245983
}
```

### Log Retention

| Log Type | Retention | Location |
|----------|-----------|----------|
| Application | 30 days | Docker logs |
| Access logs | 90 days | `/var/log/traefik/` |
| Audit logs | 1 year | Gateway container |
| Security events | 2 years | External SIEM |

---

## Security Checklist

### Initial Setup
- [ ] Generate strong secrets (32+ characters)
- [ ] Configure UFW firewall with Cloudflare IPs
- [ ] Install Cloudflare origin certificates
- [ ] Set up Docker secrets
- [ ] Configure IP whitelist
- [ ] Enable Traefik basic auth
- [ ] Test SSL/TLS configuration

### Regular Maintenance
- [ ] Review audit logs weekly
- [ ] Rotate secrets quarterly
- [ ] Update Cloudflare IP ranges
- [ ] Check for container image updates
- [ ] Verify backup integrity
- [ ] Test disaster recovery
- [ ] Review access logs for anomalies

### Before Production
- [ ] Change all default passwords
- [ ] Remove development credentials
- [ ] Enable production logging
- [ ] Configure monitoring/alerts
- [ ] Document emergency procedures
- [ ] Test rate limiting
- [ ] Verify firewall rules

---

## Incident Response

### Security Event Severity

**P0 - Critical (Immediate response)**
- Unauthorized database access
- Credential compromise
- DDoS attack
- Data breach

**P1 - High (Response within 1 hour)**
- Brute force attempts
- Suspicious API usage
- Failed authentication spikes
- Service unavailability

**P2 - Medium (Response within 4 hours)**
- Rate limit violations
- Unusual traffic patterns
- Configuration drift

**P3 - Low (Response within 24 hours)**
- General security questions
- Non-critical policy violations

### Response Procedures

**1. Credential Compromise:**
```bash
# Immediately rotate affected credential
openssl rand -base64 32 > secrets/postgres_password.txt

# Update .env file
vim .env

# Recreate affected services
docker compose up -d --force-recreate postgres

# Review access logs
docker logs coresystem-gateway | grep "authentication"
```

**2. DDoS Attack:**
```bash
# Check Cloudflare dashboard for attack metrics
# Enable "Under Attack" mode in Cloudflare
# Review rate limit effectiveness
docker logs coresystem-traefik | grep "429"

# Tighten rate limits temporarily
vim cloudflare-certs/middlewares.yml
# Traefik auto-reloads configuration
```

**3. Unauthorized Access:**
```bash
# Immediately revoke access
# Update IP whitelist
vim cloudflare-certs/middlewares.yml

# Block offending IP at firewall
ufw deny from <IP> to any

# Rotate all credentials
./generate-secrets.sh

# Review all logs for impact
docker logs coresystem-gateway --since 24h
```

### Contact Information

**Security Team:**
- Email: security@coresystem.com
- Emergency: +47 XXX XX XXX

**Escalation:**
1. Infrastructure lead
2. CTO
3. CEO

---

## Compliance

### Data Protection
- GDPR compliant (EU data residency)
- Encryption at rest (Docker secrets)
- Encryption in transit (TLS 1.3)
- Audit logging (1+ year retention)

### Security Standards
- OWASP Top 10 mitigations
- CIS Docker Benchmark compliance
- ISO 27001 aligned practices

---

**Last Updated:** November 25, 2025  
**Version:** 1.0  
**Maintained By:** Aquatiq Security Team
