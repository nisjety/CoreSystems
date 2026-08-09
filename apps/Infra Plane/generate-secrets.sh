#!/bin/bash

# Generate Secrets Script for Aquatiq Root Container
# Run this before deploying to generate all required secrets

set -e

SECRETS_DIR="secrets"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=================================================="
echo "   Aquatiq Root Container - Secrets Generator"
echo "=================================================="
echo ""

# Check if secrets directory exists
if [ ! -d "$SECRETS_DIR" ]; then
    echo -e "${RED}Error: secrets/ directory not found!${NC}"
    exit 1
fi

# Function to generate a random secret
generate_secret() {
    openssl rand -base64 32 | tr -d '\n'
}

# Function to create secret file
create_secret_file() {
    local filename=$1
    local value=$2
    
    echo "$value" > "$SECRETS_DIR/$filename"
    chmod 600 "$SECRETS_DIR/$filename"
    echo -e "${GREEN}✓${NC} Created: $filename"
}

echo "Generating secrets..."
echo ""

# 1. PostgreSQL password
if [ ! -f "$SECRETS_DIR/postgres_password.txt" ]; then
    create_secret_file "postgres_password.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: postgres_password.txt (already exists)"
fi

# 2. Redis password
if [ ! -f "$SECRETS_DIR/redis_password.txt" ]; then
    create_secret_file "redis_password.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: redis_password.txt (already exists)"
fi

# 3. n8n encryption key
if [ ! -f "$SECRETS_DIR/n8n_encryption_key.txt" ]; then
    create_secret_file "n8n_encryption_key.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: n8n_encryption_key.txt (already exists)"
fi

# 4. NATS auth token
if [ ! -f "$SECRETS_DIR/nats_auth_token.txt" ]; then
    create_secret_file "nats_auth_token.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: nats_auth_token.txt (already exists)"
fi

# 5. Root Manager API Key
if [ ! -f "$SECRETS_DIR/root_manager_api_key.txt" ]; then
    create_secret_file "root_manager_api_key.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: root_manager_api_key.txt (already exists)"
fi

# 6. pgAdmin email
if [ ! -f "$SECRETS_DIR/pgadmin_email.txt" ]; then
    echo "admin@coresystem.net" > "$SECRETS_DIR/pgadmin_email.txt"
    chmod 600 "$SECRETS_DIR/pgadmin_email.txt"
    echo -e "${GREEN}✓${NC} Created: pgadmin_email.txt"
else
    echo -e "${YELLOW}⊗${NC} Skipped: pgadmin_email.txt (already exists)"
fi

# 7. pgAdmin password
if [ ! -f "$SECRETS_DIR/pgadmin_password.txt" ]; then
    create_secret_file "pgadmin_password.txt" "$(generate_secret)"
else
    echo -e "${YELLOW}⊗${NC} Skipped: pgadmin_password.txt (already exists)"
fi

# 8. Traefik dashboard basic auth
if [ ! -f "$SECRETS_DIR/traefik_basic_auth.txt" ]; then
    echo ""
    echo "Setting up Traefik dashboard authentication..."
    
    # Check if htpasswd is available
    if ! command -v htpasswd &> /dev/null; then
        echo -e "${RED}Error: htpasswd not found!${NC}"
        echo "Install with: brew install httpd (macOS) or apt install apache2-utils (Linux)"
        echo ""
        echo "Alternatively, create manually with:"
        echo "  htpasswd -nb admin your_password > secrets/traefik_basic_auth.txt"
        echo ""
    else
        read -p "Enter Traefik dashboard username [admin]: " TRAEFIK_USER
        TRAEFIK_USER=${TRAEFIK_USER:-admin}
        
        read -s -p "Enter Traefik dashboard password: " TRAEFIK_PASS
        echo ""
        
        if [ -z "$TRAEFIK_PASS" ]; then
            echo -e "${RED}Error: Password cannot be empty${NC}"
        else
            htpasswd -nb "$TRAEFIK_USER" "$TRAEFIK_PASS" > "$SECRETS_DIR/traefik_basic_auth.txt"
            chmod 600 "$SECRETS_DIR/traefik_basic_auth.txt"
            echo -e "${GREEN}✓${NC} Created: traefik_basic_auth.txt (user: $TRAEFIK_USER)"
        fi
    fi
else
    echo -e "${YELLOW}⊗${NC} Skipped: traefik_basic_auth.txt (already exists)"
fi

echo ""
echo "=================================================="
echo "   Secrets Generation Complete!"
echo "=================================================="
echo ""

# List all secrets
echo "Generated secrets:"
ls -lh "$SECRETS_DIR"/*.txt 2>/dev/null | awk '{print "  - " $9 " (" $5 ")"}'

echo ""
echo "⚠️  IMPORTANT SECURITY NOTES:"
echo "  1. These files are ignored by git (.gitignore)"
echo "  2. Backup these files securely (encrypted backup)"
echo "  3. Never share these files publicly"
echo "  4. Rotate secrets regularly (every 90 days recommended)"
echo "  5. When deploying, ensure these files are copied to VPS"
echo ""

# Display Root Manager API Key for convenience
if [ -f "$SECRETS_DIR/root_manager_api_key.txt" ]; then
    echo "📝 Your Root Manager API Key:"
    echo "   $(cat $SECRETS_DIR/root_manager_api_key.txt)"
    echo ""
    echo "   Save this somewhere safe! You'll need it for API calls."
    echo ""
fi

echo "Next steps:"
echo "  1. Review the generated secrets in secrets/"
echo "  2. Backup secrets securely"
echo "  3. Run: ./generate-secrets.sh to regenerate if needed"
echo "  4. Deploy: docker-compose up -d"
echo ""
