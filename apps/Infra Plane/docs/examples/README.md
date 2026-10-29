# Docker Compose Examples for Traefik Integration

This directory contains example Docker Compose configurations showing how to integrate external services with Aquatiq's Traefik reverse proxy.

## Available Examples

### 1. Generic Service Template
**File:** [`generic-service-template.yml`](./generic-service-template.yml)

A complete template for any service that needs subdomain routing through Traefik. Includes:
- Basic Traefik labels
- Health check configuration
- Optional middleware examples
- Deployment checklist

**Use this when:** Starting a new service from scratch

---

### 2. Superset Integration
**File:** [`superset-docker-compose.yml`](./superset-docker-compose.yml)

Complete configuration for Apache Superset with Traefik routing.

**Features:**
- Subdomain: `superset.coresystem.com`
- Port: 8088
- Health checks
- Auto HTTPS redirect
- PostgreSQL and Redis integration

**Documentation:** [SUPERSET_SUBDOMAIN_SETUP.md](../SUPERSET_SUBDOMAIN_SETUP.md)

---

### 3. Contract Manager Integration
**File:** [`contract-manager-docker-compose.yml`](./contract-manager-docker-compose.yml)

Django-based Contract Manager with Traefik routing.

**Features:**
- Subdomain: `contract.coresystem.com`
- Port: 8001
- Django-specific configuration (ALLOWED_HOSTS, CSRF)
- Cloudflare proxy settings
- Health checks
- Static files configuration

**Documentation:** [CONTRACT_MANAGER_SUBDOMAIN_SETUP.md](../CONTRACT_MANAGER_SUBDOMAIN_SETUP.md)

---

## Quick Start

### 1. Choose an Example

```bash
# Copy the template you need
cp docs/examples/generic-service-template.yml /opt/your-service/docker-compose.yml
```

### 2. Customize Configuration

Edit the copied file and replace:
- `your-service` → your actual service name
- `yourservice.coresystem.com` → your subdomain
- `8080` → your service's internal port
- Environment variables, volumes, etc.

### 3. Add DNS Record

In Cloudflare:
- **Type:** A
- **Name:** yourservice
- **IPv4:** Your VPS IP (31.97.38.31)
- **Proxy:** ✅ Enabled

### 4. Deploy

```bash
cd /opt/your-service
docker compose up -d
```

### 5. Verify

```bash
# Check Traefik detected the service
docker logs coresystem-traefik | grep your-service

# Test access
curl -I https://yourservice.coresystem.com
```

---

## Label Reference

### Required Labels

```yaml
# Enable Traefik routing
- "traefik.enable=true"

# Define routing rule (subdomain)
- "traefik.http.routers.SERVICE.rule=Host(`subdomain.coresystem.com`)"

# Use HTTPS entrypoint
- "traefik.http.routers.SERVICE.entrypoints=websecure"

# Enable TLS/SSL
- "traefik.http.routers.SERVICE.tls=true"

# Specify container port
- "traefik.http.services.SERVICE.loadbalancer.server.port=8080"
```

### HTTP to HTTPS Redirect

```yaml
- "traefik.http.routers.SERVICE-http.rule=Host(`subdomain.coresystem.com`)"
- "traefik.http.routers.SERVICE-http.entrypoints=web"
- "traefik.http.routers.SERVICE-http.middlewares=redirect-to-https@file"
```

### Optional Security

```yaml
# Rate limiting
- "traefik.http.routers.SERVICE.middlewares=admin-ratelimit@file"

# IP whitelist
- "traefik.http.routers.SERVICE.middlewares=dynamic-ipwhitelist@file"

# Multiple middlewares
- "traefik.http.routers.SERVICE.middlewares=dynamic-ipwhitelist@file,admin-ratelimit@file"
```

---

## Network Requirements

All services must be connected to Traefik's network. Choose one:

### Option 1: Use External Network

```yaml
networks:
  coresystem_default:
    external: true
    name: coresystem_default
```

Or:

```yaml
networks:
  internal:
    external: true
    name: internal
```

### Option 2: Connect Manually

```bash
docker network connect internal your-container-name
```

To check which network Traefik uses:

```bash
docker inspect coresystem-traefik | grep -A 10 Networks
```

---

## Common Ports by Service Type

| Service Type | Typical Port | Example |
|--------------|--------------|---------|
| Web Application | 8080, 3000, 5000 | Next.js, React, Flask |
| Django | 8000, 8001 | Contract Manager |
| FastAPI | 8000 | Python APIs |
| Node.js API | 3000, 5000 | Express.js |
| BI Tools | 8088, 8000 | Superset, Metabase |
| Monitoring | 3000, 9090 | Grafana, Prometheus |
| Databases | 5432, 3306, 6379 | Postgres, MySQL, Redis |
| Message Queues | 4222, 5672 | NATS, RabbitMQ |

---

## Troubleshooting

### Service Not Accessible

```bash
# Check container is running
docker ps | grep your-service

# Check Traefik logs
docker logs coresystem-traefik | grep your-service

# Check network connectivity
docker network inspect internal | grep -A 5 your-service
```

### DNS Not Resolving

```bash
# Check DNS propagation
dig subdomain.coresystem.com
nslookup subdomain.coresystem.com
```

### 502 Bad Gateway

```bash
# Check container logs
docker logs your-container

# Test service directly
curl -I http://localhost:SERVICE_PORT

# Ping from Traefik
docker exec coresystem-traefik ping your-container
```

---

## Additional Resources

- [Adding Subdomains Guide](../ADDING_SUBDOMAINS.md) - Comprehensive guide
- [Networking Documentation](../NETWORKING.md) - Network architecture
- [Security Best Practices](../SECURITY.md) - Security configuration
- [Traefik Documentation](https://doc.traefik.io/traefik/) - Official docs

---

## Need Help?

1. Review the [full subdomain guide](../ADDING_SUBDOMAINS.md)
2. Check service-specific guides:
   - [Superset Setup](../SUPERSET_SUBDOMAIN_SETUP.md)
   - [Contract Manager Setup](../CONTRACT_MANAGER_SUBDOMAIN_SETUP.md)
3. Check troubleshooting sections in each guide
4. Review Traefik and container logs

---

**Version:** 1.0  
**Last Updated:** February 12, 2026  
**Maintained By:** Aquatiq Infrastructure Team
