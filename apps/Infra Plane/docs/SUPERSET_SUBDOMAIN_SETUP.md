# Superset Subdomain Setup Guide

## Overview
Configure `superset.coresystem.com` to route through Traefik in `/opt/coresystem/coresystem-root-container` to your Superset instance at `/opt/coresystem-superset`.

## Prerequisites
- Traefik running in `/opt/coresystem/coresystem-root-container`
- Superset container in `/opt/coresystem-superset`
- Both connected to `coresystem_default` or `internal` network
- Cloudflare DNS configured

---

## Step 1: Configure Cloudflare DNS

1. Log in to Cloudflare dashboard
2. Select domain: `coresystem.com`
3. Add DNS record:
   - **Type**: `A`
   - **Name**: `superset`
   - **IPv4 address**: `31.97.38.31` (your VPS IP)
   - **Proxy status**: ✅ Proxied (orange cloud)
   - **TTL**: Auto

---

## Step 2: Add Traefik Labels to Superset

Edit `/opt/coresystem-superset/docker-compose.yml` (or `docker-compose.prod.yml`):

```yaml
services:
  superset:
    image: apache/superset:4.0.1
    container_name: coresystem-superset-app
    restart: unless-stopped
    
    # ... your existing environment, volumes, etc. ...
    
    networks:
      - coresystem_default  # Must match Traefik's network
    
    labels:
      # Enable Traefik routing
      - "traefik.enable=true"
      
      # HTTPS router
      - "traefik.http.routers.superset.rule=Host(`superset.coresystem.com`)"
      - "traefik.http.routers.superset.entrypoints=websecure"
      - "traefik.http.routers.superset.tls=true"
      - "traefik.http.services.superset.loadbalancer.server.port=8088"
      
      # HTTP to HTTPS redirect
      - "traefik.http.routers.superset-http.rule=Host(`superset.coresystem.com`)"
      - "traefik.http.routers.superset-http.entrypoints=web"
      - "traefik.http.routers.superset-http.middlewares=redirect-to-https@file"
      
      # Specify which network Traefik should use
      - "traefik.docker.network=coresystem_default"

networks:
  coresystem_default:
    external: true
    name: coresystem_default
```

> **Important**: If your Traefik is on the `internal` network, change `coresystem_default` to `internal`.

---

## Step 3: Verify Network Connectivity

Check that both containers share a network:

```bash
# List Traefik networks
docker inspect coresystem-traefik | grep -A 10 Networks

# List Superset networks
docker inspect coresystem-superset-app | grep -A 10 Networks
```

If they don't share a network, connect Superset to Traefik's network:

```bash
# Connect to internal network (if that's where Traefik is)
docker network connect internal coresystem-superset-app

# Or connect to coresystem_default
docker network connect coresystem_default coresystem-superset-app
```

---

## Step 4: Deploy Configuration

```bash
# SSH to VPS
ssh root@31.97.38.31

# Navigate to Superset directory
cd /opt/coresystem-superset

# Restart Superset with new labels
docker compose down
docker compose up -d

# Verify container is running
docker ps | grep superset
```

Traefik will automatically detect the new labels within 30 seconds.

---

## Step 5: Verify Deployment

### Check Traefik Detection

```bash
docker logs coresystem-traefik | grep superset
```

Expected output:
```
level=info msg="Creating Router superset@docker"
level=info msg="Creating Service superset@docker"
```

### Test Access

```bash
# From VPS
curl -I http://localhost:8088

# From external (after DNS propagation)
curl -I https://superset.coresystem.com
```

### Browser Test

Open: `https://superset.coresystem.com`

You should see the Superset login page with valid SSL.

---

## Troubleshooting

### Issue: 502 Bad Gateway

**Check container status:**
```bash
docker ps | grep superset
docker logs coresystem-superset-app
```

**Check network connectivity:**
```bash
docker network inspect internal | grep -A 5 superset
docker exec coresystem-traefik ping coresystem-superset-app
```

### Issue: DNS Not Resolving

**Wait for propagation:**
```bash
dig superset.coresystem.com
nslookup superset.coresystem.com
```

Cloudflare propagation typically takes 2-5 minutes.

### Issue: Container Not Detected by Traefik

**Restart Traefik:**
```bash
cd /opt/coresystem/coresystem-root-container
docker compose restart traefik
```

**Check Docker socket proxy:**
```bash
docker logs coresystem-docker-proxy
```

---

## Security Considerations

- ✅ Cloudflare proxy provides DDoS protection
- ✅ Automatic SSL via Cloudflare (strict mode)
- ✅ Container isolated behind Traefik reverse proxy
- ✅ Not directly exposed to internet

### Optional: Add IP Whitelist

If you want to restrict access to specific IPs, add this middleware:

```yaml
labels:
  - "traefik.http.routers.superset.middlewares=dynamic-ipwhitelist@file"
```

Then configure the whitelist in `/opt/coresystem/coresystem-root-container/cloudflare-certs/middlewares.yml`.

---

## Configuration Summary

| Component | Value |
|-----------|-------|
| **Subdomain** | `superset.coresystem.com` |
| **Container** | `coresystem-superset-app` |
| **Port** | `8088` |
| **Network** | `coresystem_default` or `internal` |
| **SSL** | Cloudflare (automatic) |
| **Router Name** | `superset` |

---

## Related Documentation

- [Main Traefik Setup](./NETWORKING.md)
- [Adding More Subdomains](./ADDING_SUBDOMAINS.md)
- [Security Configuration](./SECURITY.md)
