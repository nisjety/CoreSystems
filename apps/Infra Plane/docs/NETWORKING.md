# Aquatiq Root Container - Networking Guide

## 🌐 Complete Network Architecture Documentation

This guide explains the network architecture, connectivity patterns, and troubleshooting for the Aquatiq Root Container system.

---

## Table of Contents

1. [Network Overview](#network-overview)
2. [Network Topology](#network-topology)
3. [Service Discovery](#service-discovery)
4. [Connection Examples](#connection-examples)
5. [Multi-Network Design](#multi-network-design)
6. [Port Mappings](#port-mappings)
7. [External Access](#external-access)
8. [Troubleshooting](#troubleshooting)

---

## Network Overview

### Three-Tier Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      TIER 1: Edge Layer                      │
│                    (Cloudflare + Traefik)                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Public Internet → Cloudflare CDN → Traefik Reverse Proxy  │
│                                                              │
│  Functions:                                                  │
│  - SSL/TLS termination                                      │
│  - DDoS protection                                          │
│  - Rate limiting                                            │
│  - Domain routing                                           │
│                                                              │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                  TIER 2: Application Layer                   │
│                  (coresystem-backend network)                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐           │
│  │    n8n     │  │  Gateway   │  │  pgAdmin   │           │
│  │            │  │            │  │            │           │
│  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘           │
│        │               │               │                    │
│        └───────────────┴───────────────┘                    │
│                        │                                     │
└────────────────────────┼─────────────────────────────────────┘
                         │
┌────────────────────────▼─────────────────────────────────────┐
│                  TIER 3: Data Layer                          │
│                  (coresystem-backend network)                   │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  PostgreSQL  │  │    Redis     │  │     NATS     │      │
│  │    :5432     │  │    :6379     │  │    :4222     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

---

## Network Topology

### Production Networks

**internal** (Management/Infrastructure)
```yaml
network:
  name: internal
  driver: bridge
  internal: false

services:
  - traefik
  - docker-socket-proxy
  - ntp-server
  - prometheus
  - grafana
  - landing-page
```

**coresystem-backend** (Application + Data)
```yaml
network:
  name: coresystem-backend
  driver: bridge
  internal: false

services:
  - postgres
  - redis
  - nats
  - n8n
  - coresystem-gateway
  - pgadmin
  - redis-insight
```

### Development Networks

**coresystem-local** (Company Projects)
```yaml
network:
  name: coresystem-local
  driver: bridge
  external: false

# Shared services accessible from company projects
access:
  - postgres:5432
  - redis:6379
  - nats:4222
  - gateway:7500
```

**ima-local** (Personal Projects)
```yaml
network:
  name: ima-local
  driver: bridge
  external: false

# Same services, different namespace
access:
  - postgres:5432
  - redis:6379
  - nats:4222
  - gateway:7500
```

---

## Service Discovery

### DNS Resolution

**Within Docker Networks:**
- Services use **container names** as hostnames
- Docker's internal DNS resolves names automatically
- No need for IP addresses

**Example:**
```python
# Correct ✅
conn = psycopg2.connect(
    host="postgres",  # Container name
    port=5432
)

# Wrong ❌
conn = psycopg2.connect(
    host="172.18.0.5",  # IP can change!
    port=5432
)
```

### Service Name Resolution

| Service | Container Name | Hostname (DNS) | Port |
|---------|---------------|----------------|------|
| PostgreSQL | `coresystem-postgres` | `postgres` | 5432 |
| Redis | `coresystem-redis` | `redis` | 6379 |
| NATS | `coresystem-nats` | `nats` | 4222 |
| Gateway | `coresystem-gateway` | `coresystem-gateway` | 7500 |
| n8n | `coresystem-n8n` | `coresystem-n8n` | 5678 |

**Aliases:**
```yaml
# docker-compose.yml excerpt
networks:
  coresystem-backend:
    aliases:
      - postgres  # Short alias
      - db        # Alternative alias
```

---

## Connection Examples

### PostgreSQL

**Python (psycopg2):**
```python
import psycopg2

conn = psycopg2.connect(
    host="postgres",
    port=5432,
    database="coresystem_dev",
    user="coresystem",
    password="postgres"  # Use env var in production!
)
```

**Node.js (pg):**
```javascript
const { Pool } = require('pg');

const pool = new Pool({
  host: 'postgres',
  port: 5432,
  database: 'coresystem_dev',
  user: 'coresystem',
  password: process.env.POSTGRES_PASSWORD,
});
```

**Go (pgx):**
```go
import "github.com/jackc/pgx/v5"

conn, err := pgx.Connect(context.Background(),
    "postgres://coresystem:postgres@postgres:5432/coresystem_dev")
```

**Connection String Format:**
```
postgresql://[user]:[password]@[host]:[port]/[database]?sslmode=disable
```

### Redis

**Python (redis-py):**
```python
import redis

r = redis.Redis(
    host='redis',
    port=6379,
    password='redis',
    decode_responses=True
)
```

**Node.js (ioredis):**
```javascript
const Redis = require('ioredis');

const redis = new Redis({
  host: 'redis',
  port: 6379,
  password: process.env.REDIS_PASSWORD,
});
```

**Go (go-redis):**
```go
import "github.com/redis/go-redis/v9"

rdb := redis.NewClient(&redis.Options{
    Addr:     "redis:6379",
    Password: os.Getenv("REDIS_PASSWORD"),
    DB:       0,
})
```

**Connection String Format:**
```
redis://:[password]@[host]:[port]/[db]
```

### NATS

**Python (nats.py):**
```python
import nats

nc = await nats.connect(
    servers=["nats://nats:4222"],
    token="nats"
)
```

**Node.js (nats.js):**
```javascript
const { connect } = require('nats');

const nc = await connect({
  servers: 'nats://nats:4222',
  token: process.env.NATS_AUTH_TOKEN,
});
```

**Go (nats.go):**
```go
import "github.com/nats-io/nats.go"

nc, err := nats.Connect("nats://nats:4222",
    nats.Token(os.Getenv("NATS_AUTH_TOKEN")))
```

**Connection String Format:**
```
nats://[host]:[port]?token=[auth_token]
```

### Aquatiq Gateway API

**cURL:**
```bash
curl -H "X-API-Key: gateway" \
     http://coresystem-gateway:7500/health
```

**Python (requests):**
```python
import requests

response = requests.get(
    "http://coresystem-gateway:7500/health",
    headers={"X-API-Key": "gateway"}
)
```

**Node.js (fetch):**
```javascript
const response = await fetch('http://coresystem-gateway:7500/health', {
  headers: { 'X-API-Key': process.env.INTEGRATION_GATEWAY_API_KEY }
});
```

---

## Multi-Network Design

### Why Two Development Networks?

**Scenario:**
- **coresystem-local**: Company/team projects (billing system, CRM)
- **ima-local**: Personal experiments (learning projects, prototypes)

**Benefits:**
1. **Namespace isolation**: Prevents name collisions
2. **Resource sharing**: Both networks access same services
3. **Clean separation**: Company vs personal work
4. **Testing**: Test multi-tenant scenarios locally

### Connecting Your App

**Method 1: External Network**
```yaml
# your-app/docker-compose.yml
services:
  my-app:
    image: my-app:latest
    environment:
      DATABASE_URL: postgres://coresystem:postgres@postgres:5432/coresystem_dev
    networks:
      - coresystem-local  # or ima-local

networks:
  coresystem-local:
    external: true  # Must already exist
```

**Method 2: Multiple Networks**
```yaml
services:
  my-app:
    networks:
      - coresystem-local  # Access shared services
      - app-network    # Internal app network

networks:
  coresystem-local:
    external: true
  app-network:
    driver: bridge
```

### Network Communication Flow

```
┌──────────────────┐     ┌──────────────────┐
│  Your App        │     │  Your Other App  │
│  (coresystem-local) │     │  (ima-local)     │
└────────┬─────────┘     └────────┬─────────┘
         │                        │
         │                        │
         └────────┬───────────────┘
                  │
         ┌────────▼─────────┐
         │   PostgreSQL     │
         │   (both networks)│
         └──────────────────┘
```

Both apps can access PostgreSQL, but **cannot see each other** (isolated networks).

---

## Port Mappings

### Local Development (127.0.0.1 binding)

| Service | Internal Port | External Binding | Access |
|---------|--------------|------------------|--------|
| PostgreSQL | 5432 | 127.0.0.1:5432 | localhost only |
| Redis | 6379 | 127.0.0.1:6379 | localhost only |
| NATS | 4222 | 127.0.0.1:4222 | localhost only |
| NATS Monitor | 8222 | 127.0.0.1:8222 | localhost only |
| n8n | 5678 | 127.0.0.1:5678 | localhost only |
| pgAdmin | 5050 | 127.0.0.1:5050 | localhost only |
| RedisInsight | 5540 | 127.0.0.1:5540 | localhost only |
| Gateway REST | 7500 | 127.0.0.1:7500 | localhost only |
| Gateway gRPC | 50051 | 127.0.0.1:50051 | localhost only |

**Security:** `127.0.0.1` binding prevents external access from network/internet.

### Production (No direct port exposure)

| Service | Internal Port | External Access | Via |
|---------|--------------|-----------------|-----|
| n8n | 5678 | `n8n.coresystem.com` | Traefik |
| Gateway | 7500 | `admin.coresystem.com` | Traefik |
| pgAdmin | 5050 | `pgadmin.coresystem.com` | Traefik |
| RedisInsight | 5540 | `redis.coresystem.com` | Traefik |
| PostgreSQL | 5432 | Internal only | N/A |
| Redis | 6379 | Internal only | N/A |
| NATS | 4222 | Internal only | N/A |

**Security:** Only Traefik (ports 80/443) exposed. Firewall restricts to Cloudflare IPs.

---

## External Access

### Development (Direct Access)

```bash
# PostgreSQL
psql -h localhost -p 5432 -U coresystem -d coresystem_dev

# Redis
redis-cli -h localhost -p 6379 -a redis

# n8n Web UI
open http://localhost:5678

# Gateway API
curl http://localhost:7500/health
```

### Production (Domain-Based Access)

```bash
# n8n Web UI (via Cloudflare + Traefik)
open https://n8n.coresystem.com

# Gateway API (IP whitelisted)
curl -H "X-API-Key: $API_KEY" https://admin.coresystem.com/health

# No direct database access from internet (secure!)
```

---

## Troubleshooting

### Service Discovery Issues

**Problem:** `Connection refused` or `Name resolution failed`

**Solutions:**
```bash
# 1. Verify service is running
docker ps | grep postgres

# 2. Check network membership
docker inspect postgres | jq '.[0].NetworkSettings.Networks'

# 3. Test DNS resolution
docker exec my-app ping postgres

# 4. Verify network exists
docker network ls | grep coresystem-local

# 5. Connect app to network
docker network connect coresystem-local my-app-container
```

### Connection Timeouts

**Problem:** Services unreachable, connection hangs

**Solutions:**
```bash
# 1. Check service health
docker ps --filter "name=coresystem" --format "table {{.Names}}\t{{.Status}}"

# 2. Test connectivity
docker exec my-app nc -zv postgres 5432

# 3. Check firewall (production)
ufw status verbose

# 4. Verify credentials
docker exec -it postgres psql -U coresystem -d coresystem_dev
```

### Port Conflicts

**Problem:** `Address already in use`

**Solutions:**
```bash
# 1. Check what's using the port
lsof -i :5432

# 2. Stop conflicting service
brew services stop postgresql  # macOS example

# 3. Change port binding
# Edit docker-compose.local.yml:
ports:
  - "127.0.0.1:15432:5432"  # Use alternate port
```

### Network Isolation Issues

**Problem:** Services can't communicate across networks

**Expected Behavior:**
- Services on **different** user-defined networks cannot communicate (by design)
- Services on the **same** network can communicate

**Solution:**
```yaml
# Add service to multiple networks
services:
  my-app:
    networks:
      - coresystem-local  # Access shared services
      - app-network    # Internal app network
```

---

## Best Practices

### ✅ Do

- Use container names as hostnames
- Connect to external networks with `external: true`
- Use environment variables for credentials
- Bind development ports to `127.0.0.1` only
- Use Traefik for production external access

### ❌ Don't

- Hard-code IP addresses
- Expose database ports to 0.0.0.0 in development
- Use default bridge network
- Commit connection strings with passwords
- Allow direct database access from internet

---

## Network Diagrams

### Development Network Flow

```
┌─────────────────────────────────────────┐
│         Your Local Machine              │
├─────────────────────────────────────────┤
│                                          │
│  Browser/Client                          │
│       │                                  │
│       │ localhost:5678                   │
│       ▼                                  │
│  ┌─────────────────┐                    │
│  │   Docker Host   │                    │
│  │  127.0.0.1:*    │                    │
│  └────────┬────────┘                    │
│           │                              │
│           │ docker0 bridge               │
│           │                              │
│  ┌────────▼───────────────────┐         │
│  │   coresystem-local network    │         │
│  │                            │         │
│  │  ┌──────┐  ┌──────┐       │         │
│  │  │ App  │  │  DB  │       │         │
│  │  └──────┘  └──────┘       │         │
│  └────────────────────────────┘         │
│                                          │
└─────────────────────────────────────────┘
```

### Production Network Flow

```
┌────────────────────────────────────────┐
│           Internet Client              │
└────────────────┬───────────────────────┘
                 │
                 │ HTTPS
                 ▼
┌────────────────────────────────────────┐
│        Cloudflare CDN                  │
│  - DDoS Protection                     │
│  - SSL Termination                     │
│  - Edge Caching                        │
└────────────────┬───────────────────────┘
                 │
                 │ HTTPS (Origin Cert)
                 ▼
┌────────────────────────────────────────┐
│         VPS (31.97.38.31)              │
├────────────────────────────────────────┤
│                                         │
│  UFW Firewall (Cloudflare IPs only)   │
│           │                             │
│           ▼                             │
│  ┌──────────────────┐                  │
│  │     Traefik      │                  │
│  │   Port 80/443    │                  │
│  └────────┬─────────┘                  │
│           │                             │
│  ┌────────▼─────────────────┐          │
│  │  internal network        │          │
│  │                          │          │
│  │  ┌────────────────────┐ │          │
│  │  │ coresystem-backend    │ │          │
│  │  │                    │ │          │
│  │  │ ┌──────┐ ┌──────┐ │ │          │
│  │  │ │ App  │ │  DB  │ │ │          │
│  │  │ └──────┘ └──────┘ │ │          │
│  │  └────────────────────┘ │          │
│  └──────────────────────────┘          │
│                                         │
└─────────────────────────────────────────┘
```

---

**Last Updated:** November 25, 2025  
**Version:** 1.0  
**Maintained By:** Aquatiq Infrastructure Team
