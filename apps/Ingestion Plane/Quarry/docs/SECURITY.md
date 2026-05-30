# Quarry Security Documentation

**Version:** 0.1.0  
**Last Updated:** 2026-02-18

---

## Table of Contents

1. [Security Overview](#security-overview)
2. [Threat Model](#threat-model)
3. [Authentication & Authorization](#authentication--authorization)
4. [URL Security Scanning](#url-security-scanning)
5. [Rate Limiting](#rate-limiting)
6. [Input Validation](#input-validation)
7. [Webhook Security](#webhook-security)
8. [Network Security](#network-security)
9. [Data Protection](#data-protection)
10. [Audit Logging](#audit-logging)
11. [Security Best Practices](#security-best-practices)

---

## Security Overview

Quarry implements defense-in-depth security architecture with multiple layers of protection:

```
┌─────────────────────────────────────────┐
│  1. API Key Authentication (Tier 1)    │
├─────────────────────────────────────────┤
│  2. Rate Limiting (Tier 2)              │
├─────────────────────────────────────────┤
│  3. Input Validation (Tier 3)           │
├─────────────────────────────────────────┤
│  4. URL Security Scanning (Tier 4)      │
├─────────────────────────────────────────┤
│  5. Content Security (Tier 5)           │
└─────────────────────────────────────────┘
```

**Security-First Design Principles:**
- Deny by default
- Least privilege
- Defense in depth
- Fail securely
- Complete mediation

---

## Threat Model

### Identified Threats

| Threat | Severity | Mitigation |
|--------|----------|------------|
| **Malicious URL Injection** | Critical | 5-provider security scanning |
| **API Key Theft** | High | HTTPS-only, constant-time comparison |
| **DDoS Attack** | High | Rate limiting, proxy rotation |
| **Data Exfiltration** | Medium | API key scoping, audit logging |
| **Timing Attacks** | Medium | Constant-time operations |
| **Webhook Spam** | Low | HMAC signature verification |

### Attack Surfaces

1. **HTTP API**: All public endpoints
2. **Webhook Delivery**: Outbound HTTP requests
3. **URL Fetching**: Arbitrary user-provided URLs
4. **Dependencies**: Third-party libraries

---

## Authentication & Authorization

### API Key Authentication

**Implementation:**
```go
// Constant-time comparison prevents timing attacks
func validateAPIKey(provided, expected string) bool {
    providedBytes := []byte(strings.TrimSpace(provided))
    expectedBytes := []byte(strings.TrimSpace(expected))
    return subtle.ConstantTimeCompare(providedBytes, expectedBytes) == 1
}
```

**Header:**
```bash
X-API-Key: your-api-key-here
```

**Key Generation:**
```bash
# Generate cryptographically secure API key (256-bit)
openssl rand -hex 32
```

**Best Practices:**
- **Length**: Minimum 32 characters
- **Entropy**: Use cryptographic RNG
- **Storage**: Environment variables, never in code
- **Transmission**: HTTPS only
- **Rotation**: Every 90 days
- **Scope**: Different keys for different environments (dev, staging, prod)

---

## URL Security Scanning

### Multi-Provider Security Architecture

Quarry uses **5 independent security providers** with consensus-based threat detection:

#### **1. URLhaus (Malware URLs)**
- **Provider**: abuse.ch
- **Coverage**: Malware distribution URLs
- **API**: https://urlhaus-api.abuse.ch
- **Caching**: 15min TTL

#### **2. PhishTank (Phishing URLs)**
- **Provider**: OpenDNS/Cisco
- **Coverage**: Phishing websites
- **API**: phishtank.org
- **Caching**: 15min TTL

#### **3. Google Safe Browsing**
- **Provider**: Google
- **Coverage**: Malware, phishing, unwanted software, social engineering
- **API**: safebrowsing.googleapis.com
- **Caching**: 30min TTL

#### **4. AbuseIPDB (Malicious IPs)**
- **Provider**: AbuseIPDB
- **Coverage**: Reported malicious IP addresses
- **API**: abuseipdb.com/api/v2
- **Fallback**: DNS-based checks

#### **5. Heuristic Analysis (Pattern-Based)**
- **Pattern Matching**: Suspicious URL patterns
- **Signatures**:
  - Known malware file extensions
  - Suspicious TLDs (.tk, .ml, .ga)
  - IP-based URLs
  - Abnormally long URLs (>2000 chars)

### Security Scan Flow

```
URL → Cache Check → Hit?
         ↓ [No]      ↓ [Yes]
  Parallel Provider Checks → Return Cached
   ├─ URLhaus
   ├─ PhishTank
   ├─ Safe Browsing  → Any Threat?
   ├─ AbuseIPDB             ↓ [Yes]
   └─ Heuristics      Block Request
         ↓ [No Threats]      ↓
    Cache Result (15min)  Log & Return HTTP 403
         ↓
    Allow Request
```

**Response Time:**
- Cache hit: 2-5ms
- Cache miss: 200-500ms (parallel checks)

---

## Rate Limiting

### Two-Tier Rate Limiting

#### **1. Per-API-Key Limiting**
```
Authenticated requests: 100 requests/minute per key
```

#### **2. Per-IP Limiting**
```
Unauthenticated requests: 20 requests/minute per IP
```

### Rate Limit Response

**HTTP 429 Too Many Requests:**
```json
{
  "success": false,
  "error": "rate limit exceeded",
  "requestId": "req_abc123"
}
```

**Headers:**
```
X-RateLimit-Limit: 100
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1645123200
Retry-After: 60
```

---

## Input Validation

### URL Validation

**Requirements:**
1. Absolute URL (must include scheme)
2. HTTP or HTTPS only
3. Must include host
4. Max length: 2000 characters

**Implementation:**
```go
func validateAbsoluteHTTPURL(rawURL string) error {
    u, err := url.Parse(rawURL)
    if err != nil {
        return errors.New("invalid URL format")
    }
    if !u.IsAbs() {
        return errors.New("URL must be absolute")
    }
    if u.Scheme != "http" && u.Scheme != "https" {
        return errors.New("only HTTP/HTTPS URLs allowed")
    }
    if u.Host == "" {
        return errors.New("URL must include host")
    }
    return nil
}
```

### Parameter Validation

**Bounds Checking:**
- `maxPages`: 1-1000
- `maxDepth`: 1-10
- `limit` (map): 1-1000
- `limit` (search): 1-200
- `batchURLs`: 1-200
- `waitFor`: 0-30000ms

---

## Webhook Security

### HMAC Signature Verification

**Signature Generation:**
```python
import hmac
import hashlib

signature = hmac.new(
    secret.encode('utf-8'),
    payload.encode('utf-8'),
    hashlib.sha256
).hexdigest()
```

**Header:**
```
X-Webhook-Signature: <hex-encoded-hmac-sha256>
```

**Verification (Receiver):**
```python
def verify_webhook(payload, signature, secret):
    expected = hmac.new(
        secret.encode('utf-8'),
        payload.encode('utf-8'),
        hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(signature, expected)
```

---

## Audit Logging

### Structured Logging

**Format:** JSON (zerolog)

**Fields:**
- `time`: ISO 8601 timestamp
- `level`: debug|info|warn|error|fatal
- `message`: Log message
- `request_id`: Unique request identifier
- `method`: HTTP method
- `path`: Request path
- `status`: HTTP status code
- `latency_ms`: Request duration
- `ip`: Client IP address

**Example:**
```json
{
  "time": "2026-02-18T16:00:00Z",
  "level": "info",
  "message": "request completed",
  "request_id": "req_abc123",
  "method": "POST",
  "path": "/v1/scrape",
  "status": 200,
  "latency_ms": 145,
  "ip": "192.0.2.1"
}
```

---

## Security Best Practices

### For Operators

1. **Use Strong API Keys**: 256-bit random, rotate every 90 days
2. **Enable HTTPS**: TLS 1.2+ only, valid certificates
3. **Network Isolation**: Firewall rules, VPC/security groups
4. **Secrets Management**: Kubernetes Secrets, AWS Secrets Manager
5. **Monitor Logs**: Centralized logging (ELK, Splunk, CloudWatch)
6. **Regular Updates**: Apply security patches promptly

### For Developers

1. **Input Validation**: Never trust user input
2. **Output Encoding**: Prevent XSS in responses
3. **Dependency Scanning**: `go mod audit`, Dependabot
4. **Code Review**: Security-focused peer review
5. **Static Analysis**: Use `gosec`, `staticcheck`

### For API Consumers

1. **Protect API Keys**: Never commit to git, use env vars
2. **Use HTTPS**: Always use encrypted connections
3. **Verify Webhooks**: Validate `X-Webhook-Signature`
4. **Rate Limiting**: Respect rate limits, implement backoff

---

## Security Contact

**Report Security Vulnerabilities:**
- Email: security@quarry.example.com
- Bug Bounty: Coming Soon

**Response SLA:**
- Acknowledgment: 24 hours
- Initial Assessment: 72 hours
- Fix Timeline: Based on severity (Critical: 7 days, High: 30 days)

---

**Last Updated:** 2026-02-18
