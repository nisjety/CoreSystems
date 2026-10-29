# ✅ AQUATIQ AUTO-DEPLOYMENT - SIMPLIFIED TO 10 CREDENTIALS

## What Changed

✅ **Reduced from 23 to 10 credentials**
✅ **Removed all optional integrations** not in docker-compose.flexible.yml
✅ **Much simpler setup** - only asks for what's needed
✅ **Cleaner documentation** - easier to understand

## The 10 Essential Credentials

| Service | Credential Name | Example |
|---------|-----------------|---------|
| Global | `DOMAIN` | `coresystem.com` |
| PostgreSQL | `POSTGRES_PASSWORD` | `secure_password` |
| Redis | `REDIS_PASSWORD` | `secure_password` |
| NATS | `NATS_AUTH_TOKEN` | `secure_token` |
| n8n | `N8N_ENCRYPTION_KEY` | `openssl rand -base64 32` |
| Aquatiq Gateway | `INTEGRATION_GATEWAY_API_KEY` | `openssl rand -base64 32` |
| pgAdmin | `PGADMIN_EMAIL` | `admin@coresystem.com` |
| pgAdmin | `PGADMIN_PASSWORD` | `secure_password` |
| Grafana | `GRAFANA_PASSWORD` | `secure_password` |
| Traefik Dashboard | `TRAEFIK_DASHBOARD_AUTH` | `htpasswd -nb admin pass` |

## Removed Credentials

These are not in docker-compose.flexible.yml, so they were removed:

- ✗ SuperOffice OAuth2 (CLIENT_ID, CLIENT_SECRET)
- ✗ Visma OAuth2 (CLIENT_ID, CLIENT_SECRET)
- ✗ Risk Agent (URL, API_KEY)
- ✗ Microsoft Teams (TEAM_ID, CHANNEL_ID)
- ✗ Email Alerts (EMAIL_TO, EMAIL_FROM)
- ✗ OAuth2 Encryption Key
- ✗ Cloudflare (API_TOKEN, ZONE_ID)

**Note:** These can be added later if you need them for future integrations.

## How to Set Up

```bash
# 1. Install GitHub CLI
brew install gh

# 2. Authenticate
gh auth login

# 3. Run the setup script
chmod +x setup-github-secrets.sh
./setup-github-secrets.sh

# 4. Push to trigger deployment
git push origin main
```

That's it! The workflow will automatically:
1. Build Docker image
2. Push to GitHub Container Registry
3. Deploy to VPS with all 10 credentials injected
4. Restart all 13 services

## Files Changed

- ✓ `setup-github-secrets.sh` - Only prompts for 10 credentials
- ✓ `CREDENTIALS_SETUP.md` - Simplified documentation
- ✓ `.env.production.template` - Only 10 variables
- ✓ `.github/workflows/build-and-deploy.yml` - Only injects 10 secrets

Committed: `7404da3`
