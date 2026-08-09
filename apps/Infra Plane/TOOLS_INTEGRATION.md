# Aquatiq Tools Integration with Root Infrastructure

This document explains how to connect the Aquatiq Tools deployment (`/opt/coresystem-tools`) to use the shared Traefik instance and Cloudflare origin certificates.

## Overview

- **Traefik Instance**: Running in `coresystem-root-container` (this repo)
- **SSL Certificates**: Cloudflare origin certificates (valid until Nov 2040)
- **Domain**: `tools.coresystem.com` → Routes to Aquatiq Tools frontend
- **Network**: Services must join the `internal` network to be discovered by Traefik

## Configuration Required

### 1. Update Aquatiq Tools docker-compose.prod.yml

The frontend service in `/opt/coresystem-tools/docker-compose.prod.yml` needs to:

1. **Join the external `internal` network**
2. **Remove any port bindings** (Traefik will handle routing)
3. **Add Traefik labels** (optional, as file provider is already configured)

#### Example Configuration:

```yaml
services:
  frontend:
    image: your-frontend-image
    container_name: coresystem-tools-frontend
    restart: unless-stopped
    networks:
      - default           # Keep your internal networks
      - internal          # Add external Traefik network
    # Remove port bindings like:
    # ports:
    #   - "3000:3000"
    
networks:
  default:
    name: coresystem-tools-network
    driver: bridge
  
  # Add external network reference
  internal:
    external: true
    name: internal
```

### 2. Alternative: Use Traefik Labels

If you prefer explicit Traefik labels instead of the file provider configuration:

```yaml
services:
  frontend:
    # ... other config ...
    labels:
      - "traefik.enable=true"
      - "traefik.docker.network=internal"
      - "traefik.http.routers.tools.rule=Host(`tools.coresystem.com`)"
      - "traefik.http.routers.tools.entrypoints=websecure"
      - "traefik.http.routers.tools.tls=true"
      - "traefik.http.routers.tools.tls.options=default@file"
      - "traefik.http.services.tools.loadbalancer.server.port=3000"
      # HTTP redirect
      - "traefik.http.routers.tools-http.rule=Host(`tools.coresystem.com`)"
      - "traefik.http.routers.tools-http.entrypoints=web"
      - "traefik.http.routers.tools-http.middlewares=redirect-to-https@file"
```

## Cloudflare DNS Configuration

Ensure Cloudflare DNS is configured:

1. **A Record**: `tools.coresystem.com` → `31.97.38.31` (VPS IP)
2. **Proxy Status**: 🟠 Proxied (Cloudflare CDN enabled)
3. **SSL/TLS Mode**: Full (strict) - Uses origin certificates

## Testing

1. **Restart Traefik** to load new configuration:
   ```bash
   cd /opt/coresystem-root-container
   docker compose restart traefik
   ```

2. **Restart Aquatiq Tools** frontend:
   ```bash
   cd /opt/coresystem-tools
   docker compose -f docker-compose.prod.yml restart frontend
   ```

3. **Test HTTPS**:
   ```bash
   curl -I https://tools.coresystem.com
   # Should return: HTTP/2 200
   ```

4. **Check Traefik logs**:
   ```bash
   docker logs coresystem-traefik --tail=50
   ```

## Troubleshooting

### Service Not Found
- Verify frontend container name is `coresystem-tools-frontend`
- Check container is on `internal` network: `docker network inspect internal`

### 525 SSL Error (Cloudflare)
- Verify Traefik is running: `docker ps | grep traefik`
- Check certificates are mounted: `docker exec coresystem-traefik ls /certs`
- Ensure Cloudflare SSL mode is "Full (strict)"

### Connection Refused
- Verify frontend port (default: 3000)
- Check frontend is running: `docker ps | grep frontend`
- Test internal connectivity: `docker exec coresystem-traefik wget -O- http://coresystem-tools-frontend:3000`

## Network Architecture

```
Cloudflare (CDN + WAF)
    ↓ HTTPS (Strict Mode)
Traefik (coresystem-root-container)
    ↓ HTTP (internal network)
Frontend (coresystem-tools)
```

- **External**: Cloudflare ↔ Traefik uses Cloudflare origin certificates
- **Internal**: Traefik ↔ Services uses plain HTTP (secure internal network)
