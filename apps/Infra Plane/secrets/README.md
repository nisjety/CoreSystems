# Secrets Directory

This directory contains sensitive credentials stored as Docker secrets.

## Files

Create these files before deployment:

- `postgres_password.txt` - PostgreSQL password
- `redis_password.txt` - Redis password
- `n8n_encryption_key.txt` - n8n encryption key (base64 encoded)
- `traefik_dashboard_auth.txt` - Traefik dashboard basic auth hash (htpasswd format)
- `nats_auth_token.txt` - NATS authentication token

## Generate Secrets

Run the `./deploy.sh generate-secrets` command to automatically generate all secrets.

Or manually:

```bash
# PostgreSQL password
openssl rand -base64 32 > secrets/postgres_password.txt

# Redis password
openssl rand -base64 32 > secrets/redis_password.txt

# n8n encryption key
openssl rand -base64 32 > secrets/n8n_encryption_key.txt

# NATS auth token
openssl rand -base64 32 > secrets/nats_auth_token.txt

# Traefik dashboard auth (replace 'admin' and 'your_password')
htpasswd -nb admin your_password > secrets/traefik_dashboard_auth.txt
```

## Security

⚠️ **IMPORTANT**: 
- Never commit these files to git (they are ignored by `.gitignore`)
- Set restrictive permissions: `chmod 600 secrets/*.txt`
- Backup these files securely
- Rotate secrets regularly
