# Traefik Subdomain Quick Reference Card

## 🚀 5-Minute Setup

### 1. Add DNS (Cloudflare)
```
Type: A
Name: yourservice
IP: 31.97.38.31
Proxy: ✅ Enabled
```

### 2. Add Labels to docker-compose.yml
```yaml
labels:
  - "traefik.enable=true"
  - "traefik.http.routers.SERVICE.rule=Host(`yourservice.coresystem.com`)"
  - "traefik.http.routers.SERVICE.entrypoints=websecure"
  - "traefik.http.routers.SERVICE.tls=true"
  - "traefik.http.services.SERVICE.loadbalancer.server.port=8080"
  - "traefik.http.routers.SERVICE-http.rule=Host(`yourservice.coresystem.com`)"
  - "traefik.http.routers.SERVICE-http.entrypoints=web"
  - "traefik.http.routers.SERVICE-http.middlewares=redirect-to-https@file"
  - "traefik.docker.network=internal"

networks:
  internal:
    external: true
    name: internal
```

### 3. Deploy
```bash
docker compose up -d
```

### 4. Verify
```bash
docker logs coresystem-traefik | grep SERVICE
curl -I https://yourservice.coresystem.com
```

---

## 🎯 Current Subdomains

| Subdomain | Service | Port | Location |
|-----------|---------|------|----------|
| app.coresystem.com | Landing | 80 | root-container |
| n8n.coresystem.com | N8N | 5678 | root-container |
| admin.coresystem.com | Gateway | 7500 | root-container |
| pgadmin.coresystem.com | pgAdmin | 80 | root-container |
| redis.coresystem.com | Redis | 5540 | root-container |
| prometheus.coresystem.com | Prometheus | 9090 | root-container |
| grafana.coresystem.com | Grafana | 3000 | root-container |
| traefik.coresystem.com | Traefik | - | root-container |
| superset.coresystem.com | Superset | 8088 | /opt/coresystem-superset |
| contract.coresystem.com | Contract Mgr | 8001 | /opt/coresystem-contract-manager |

---

## 🔧 Replacements Required

Replace these in labels:
- `SERVICE` → Unique service identifier (e.g., `superset`, `contract-manager`)
- `yourservice.coresystem.com` → Your actual subdomain
- `8080` → Your service's internal port
- `internal` → Network name (check: `docker network ls`)

---

## 🌐 Network Names

Choose based on where Traefik is:
- `internal` - For internal services (most common)
- `coresystem_default` - Alternative network name
- `coresystem-backend` - Backend services only

Check Traefik's network:
```bash
docker inspect coresystem-traefik | grep -A 5 Networks
```

---

## 🛡️ Optional Security Middlewares

Add to router labels:

### Rate Limiting
```yaml
- "traefik.http.routers.SERVICE.middlewares=admin-ratelimit@file"
```

### IP Whitelist
```yaml
- "traefik.http.routers.SERVICE.middlewares=dynamic-ipwhitelist@file"
```

### Basic Auth
```yaml
- "traefik.http.routers.SERVICE.middlewares=traefik-auth@file"
```

### Chain Multiple
```yaml
- "traefik.http.routers.SERVICE.middlewares=dynamic-ipwhitelist@file,admin-ratelimit@file"
```

---

## 📋 Label Anatomy

```yaml
# Enable routing
traefik.enable=true

# Router name and rule
traefik.http.routers.{ROUTER-NAME}.rule=Host(`subdomain.coresystem.com`)

# Entry point (443)
traefik.http.routers.{ROUTER-NAME}.entrypoints=websecure

# Enable SSL
traefik.http.routers.{ROUTER-NAME}.tls=true

# Container port
traefik.http.services.{SERVICE-NAME}.loadbalancer.server.port=8080

# HTTP redirect
traefik.http.routers.{ROUTER-NAME}-http.rule=Host(`subdomain.coresystem.com`)
traefik.http.routers.{ROUTER-NAME}-http.entrypoints=web
traefik.http.routers.{ROUTER-NAME}-http.middlewares=redirect-to-https@file
```

---

## ✅ Pre-Deploy Checklist

- [ ] DNS A record added in Cloudflare (proxied)
- [ ] Unique router name (no conflicts)
- [ ] Correct subdomain in rule
- [ ] Correct internal port
- [ ] Container on same network as Traefik
- [ ] Network name specified in labels
- [ ] Health check configured (optional)
- [ ] HTTP to HTTPS redirect added

---

## 🔍 Troubleshooting Commands

```bash
# Check Traefik detected service
docker logs coresystem-traefik | grep SERVICE-NAME

# Check container status
docker ps | grep container-name

# Check container logs
docker logs container-name

# Test DNS
dig subdomain.coresystem.com

# Test service directly
curl -I http://localhost:PORT

# Check network
docker network inspect internal | grep -A 5 container

# Ping from Traefik
docker exec coresystem-traefik ping container-name

# Restart Traefik
docker compose restart traefik
```

---

## 📚 Full Documentation

- [Adding Subdomains Guide](ADDING_SUBDOMAINS.md)
- [Superset Setup](SUPERSET_SUBDOMAIN_SETUP.md)
- [Contract Manager Setup](CONTRACT_MANAGER_SUBDOMAIN_SETUP.md)
- [Docker Compose Examples](examples/)
- [Generic Template](examples/generic-service-template.yml)

---

## 🚨 Common Issues

### 502 Bad Gateway
→ Container not running or wrong port

### 404 Not Found
→ Traefik didn't detect labels, restart Traefik

### DNS Not Resolving
→ Wait 2-5 mins for Cloudflare propagation

### SSL Error
→ Check Cloudflare SSL mode (Full or Strict)

### Connection Refused
→ Containers not on same network

---

**Quick Tip:** Use `docs/examples/generic-service-template.yml` as starting point!

**Version:** 1.0 | **Updated:** Feb 12, 2026
