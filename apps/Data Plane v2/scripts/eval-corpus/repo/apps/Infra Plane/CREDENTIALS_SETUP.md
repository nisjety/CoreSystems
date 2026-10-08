# 🔐 Aquatiq Credentials Setup Guide

Simple setup for the 10 credentials needed by docker-compose.flexible.yml services.

## Overview

Credentials are stored securely in **GitHub Secrets** and automatically injected to VPS on each deployment.

## ✅ Quick Setup

### 1. Install GitHub CLI

```bash
brew install gh    # macOS
# or: https://cli.github.com/
```

### 2. Authenticate

```bash
gh auth login
```

### 3. Run Setup Script

```bash
chmod +x setup-github-secrets.sh
./setup-github-secrets.sh
```

## 📋 10 Required Credentials

| Name | Service | Example |
|------|---------|---------|
| `DOMAIN` | Traefik/Global | `coresystem.com` |
| `POSTGRES_PASSWORD` | PostgreSQL | `secure_password_here` |
| `REDIS_PASSWORD` | Redis | `secure_password_here` |
| `NATS_AUTH_TOKEN` | NATS | `secure_token_here` |
| `N8N_ENCRYPTION_KEY` | n8n | Base64 string (32 bytes) |
| `INTEGRATION_GATEWAY_API_KEY` | Aquatiq Gateway | Base64 string (32 bytes) |
| `PGADMIN_EMAIL` | pgAdmin | `admin@coresystem.com` |
| `PGADMIN_PASSWORD` | pgAdmin | `secure_password_here` |
| `GRAFANA_PASSWORD` | Grafana | `secure_password_here` |
| `TRAEFIK_DASHBOARD_AUTH` | Traefik Dashboard | htpasswd hash |

## 🔑 Generate Secure Values

```bash
# Generate base64 encryption keys (32 bytes)
openssl rand -base64 32

# Generate Traefik dashboard auth
htpasswd -nb admin password_here
# Copy entire output: admin:$apr1$...
```

## 🚀 Automatic Deployment

Once secrets are set up:

```bash
git push origin main
# Workflow automatically:
# 1. Builds Docker image
# 2. Injects all 10 secrets to VPS
# 3. Creates secrets/*.txt files
# 4. Restarts all 13 services
```

## ✅ Verify

```bash
# List your secrets
gh secret list --repo I-Dacosta/Aquatiq-root

# Check VPS deployment
ssh root@31.97.38.31
cat /opt/coresystem/.env.production
ls -la /opt/coresystem/secrets/
docker compose ps
```
