# Deployment Script for Aquatiq Root Container
# This script automates the deployment process

#!/bin/bash
set -e

echo "🚀 Aquatiq Root Container Deployment"
echo "======================================"
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to generate secrets
generate_secrets() {
    echo -e "${BLUE}🔐 Generating secrets...${NC}"
    
    # Create secrets directory if it doesn't exist
    mkdir -p secrets
    
    # Generate PostgreSQL password
    if [ ! -f secrets/postgres_password.txt ]; then
        openssl rand -base64 32 > secrets/postgres_password.txt
        echo -e "${GREEN}✓ Generated postgres_password.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/postgres_password.txt already exists, skipping${NC}"
    fi
    
    # Generate Redis password
    if [ ! -f secrets/redis_password.txt ]; then
        openssl rand -base64 32 > secrets/redis_password.txt
        echo -e "${GREEN}✓ Generated redis_password.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/redis_password.txt already exists, skipping${NC}"
    fi
    
    # Generate n8n encryption key
    if [ ! -f secrets/n8n_encryption_key.txt ]; then
        openssl rand -base64 32 > secrets/n8n_encryption_key.txt
        echo -e "${GREEN}✓ Generated n8n_encryption_key.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/n8n_encryption_key.txt already exists, skipping${NC}"
    fi
    
    # Generate NATS auth token
    if [ ! -f secrets/nats_auth_token.txt ]; then
        openssl rand -base64 32 > secrets/nats_auth_token.txt
        echo -e "${GREEN}✓ Generated nats_auth_token.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/nats_auth_token.txt already exists, skipping${NC}"
    fi
    
    # Generate Traefik dashboard auth
    if [ ! -f secrets/traefik_dashboard_auth.txt ]; then
        echo -e "${YELLOW}Enter username for Traefik dashboard (default: admin):${NC}"
        read -r username
        username=${username:-admin}
        
        echo -e "${YELLOW}Enter password for Traefik dashboard:${NC}"
        read -s password
        echo ""
        
        if command -v htpasswd >/dev/null 2>&1; then
            htpasswd -nb "$username" "$password" > secrets/traefik_dashboard_auth.txt
            echo -e "${GREEN}✓ Generated traefik_dashboard_auth.txt${NC}"
        else
            echo -e "${RED}❌ htpasswd not found. Please install apache2-utils:${NC}"
            echo "  Ubuntu/Debian: sudo apt-get install apache2-utils"
            echo "  macOS: brew install httpd"
            exit 1
        fi
    else
        echo -e "${YELLOW}⚠ secrets/traefik_dashboard_auth.txt already exists, skipping${NC}"
    fi
    
    # Generate Root Manager API key
    if [ ! -f secrets/root_manager_api_key.txt ]; then
        echo "Generating Root Manager API key..."
        openssl rand -base64 32 > secrets/root_manager_api_key.txt
        echo -e "${GREEN}✓ Created secrets/root_manager_api_key.txt${NC}"
        echo -e "${BLUE}ℹ Root Manager API Key:${NC} $(cat secrets/root_manager_api_key.txt)"
        echo ""
    else
        echo -e "${YELLOW}⚠ secrets/root_manager_api_key.txt already exists, skipping${NC}"
    fi
    
    # Generate pgAdmin credentials
    if [ ! -f secrets/pgadmin_email.txt ]; then
        echo "Enter pgAdmin email:"
        read -r pgadmin_email
        echo "$pgadmin_email" > secrets/pgadmin_email.txt
        echo -e "${GREEN}✓ Created secrets/pgadmin_email.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/pgadmin_email.txt already exists, skipping${NC}"
    fi
    
    if [ ! -f secrets/pgadmin_password.txt ]; then
        echo "Enter pgAdmin password:"
        read -rs pgadmin_password
        echo "$pgadmin_password" > secrets/pgadmin_password.txt
        echo -e "${GREEN}✓ Created secrets/pgadmin_password.txt${NC}"
    else
        echo -e "${YELLOW}⚠ secrets/pgadmin_password.txt already exists, skipping${NC}"
    fi
    
    # Set restrictive permissions
    chmod 600 secrets/*.txt
    echo -e "${GREEN}✓ Set restrictive permissions on secret files${NC}"
    echo ""
}

# Function to setup UFW firewall
setup_firewall() {
    echo -e "${BLUE}🔥 Setting up UFW firewall...${NC}"
    
    if ! command -v ufw >/dev/null 2>&1; then
        echo -e "${YELLOW}⚠ UFW not found. Skipping firewall setup.${NC}"
        echo "  Install with: sudo apt-get install ufw"
        return
    fi
    
    echo -e "${YELLOW}This will configure UFW to allow only ports 22, 80, 443, and 123.${NC}"
    echo -e "${YELLOW}Continue? (y/N):${NC}"
    read -r response
    
    if [[ ! "$response" =~ ^[Yy]$ ]]; then
        echo "Skipping firewall setup."
        return
    fi
    
    # Enable UFW if not already enabled
    sudo ufw --force enable
    
    # Set default policies
    sudo ufw default deny incoming
    sudo ufw default allow outgoing
    
    # Allow SSH (important!)
    sudo ufw allow 22/tcp comment "SSH"
    
    # Allow HTTP/HTTPS
    sudo ufw allow 80/tcp comment "HTTP"
    sudo ufw allow 443/tcp comment "HTTPS"
    
    # Allow NTP (public)
    sudo ufw allow 123/udp comment "NTP"
    
    # Reload UFW
    sudo ufw reload
    
    echo -e "${GREEN}✓ Firewall configured${NC}"
    echo ""
    sudo ufw status verbose
    echo ""
}

# Check command line arguments
if [ "$1" = "generate-secrets" ]; then
    generate_secrets
    exit 0
fi

if [ "$1" = "setup-firewall" ]; then
    setup_firewall
    exit 0
fi

if [ "$1" = "help" ] || [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    echo "Usage: ./deploy.sh [command]"
    echo ""
    echo "Commands:"
    echo "  (none)           - Full deployment"
    echo "  generate-secrets - Generate all secret files"
    echo "  setup-firewall   - Configure UFW firewall rules"
    echo "  help             - Show this help message"
    echo ""
    exit 0
fi

# Check if secrets exist
echo "🔍 Checking secrets..."
REQUIRED_SECRETS=("postgres_password.txt" "redis_password.txt" "n8n_encryption_key.txt" "traefik_dashboard_auth.txt" "nats_auth_token.txt")
MISSING_SECRETS=()

for secret in "${REQUIRED_SECRETS[@]}"; do
    if [ ! -f "secrets/$secret" ]; then
        MISSING_SECRETS+=("$secret")
    fi
done

if [ ${#MISSING_SECRETS[@]} -ne 0 ]; then
    echo -e "${RED}❌ Error: Missing required secret files:${NC}"
    for secret in "${MISSING_SECRETS[@]}"; do
        echo "  - secrets/$secret"
    done
    echo ""
    echo -e "${YELLOW}Run './deploy.sh generate-secrets' to generate them automatically${NC}"
    exit 1
fi

echo -e "${GREEN}✓ All secrets found${NC}"
echo ""

# Check if .env exists
if [ ! -f .env ]; then
    echo -e "${YELLOW}⚠ .env file not found, using environment variables only${NC}"
    echo ""
else
    # Load environment variables
    source .env
    echo -e "${GREEN}✓ Environment configuration loaded${NC}"
    echo ""
fi

# Check DNS configuration if N8N_DOMAIN is set
if [ ! -z "$N8N_DOMAIN" ]; then
    echo "🔍 Checking DNS configuration for $N8N_DOMAIN..."
    DNS_IP=$(dig +short $N8N_DOMAIN | tail -n1)

    if [ -z "$DNS_IP" ]; then
        echo -e "${YELLOW}⚠ Warning: DNS not configured for $N8N_DOMAIN${NC}"
        echo "Please add an A record in your DNS provider:"
        echo "  Type: A"
        echo "  Host: n8n"
        echo "  Value: $(curl -s ifconfig.me)"
        echo ""
        read -p "Continue anyway? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    else
        echo -e "${GREEN}✓ DNS configured: $N8N_DOMAIN → $DNS_IP${NC}"
    fi
    echo ""
fi

# Make init script executable
echo "📝 Setting permissions..."
chmod +x init-multi-db.sh
echo -e "${GREEN}✓ Permissions set${NC}"
echo ""

# Pull latest images
echo "📦 Pulling Docker images..."
docker-compose pull
echo -e "${GREEN}✓ Images pulled${NC}"
echo ""

# Start services
echo "🎯 Starting services..."
docker-compose up -d
echo -e "${GREEN}✓ Services started${NC}"
echo ""

# Wait for services to be healthy
echo "⏳ Waiting for services to be healthy..."
sleep 10

# Check service status
echo ""
echo "📊 Service Status:"
docker-compose ps
echo ""

# Check if n8n is accessible
echo "🔍 Checking n8n accessibility..."
sleep 5

if docker-compose ps n8n | grep -q "Up"; then
    echo -e "${GREEN}✓ n8n is running${NC}"
else
    echo -e "${RED}❌ n8n is not running${NC}"
    echo "Check logs with: docker-compose logs n8n"
    exit 1
fi

# Check if Traefik is running
if docker-compose ps traefik | grep -q "Up"; then
    echo -e "${GREEN}✓ Traefik is running${NC}"
else
    echo -e "${RED}❌ Traefik is not running${NC}"
    echo "Check logs with: docker-compose logs traefik"
    exit 1
fi

echo ""
echo "======================================"
echo -e "${GREEN}🎉 Deployment Complete!${NC}"
echo "======================================"
echo ""
if [ ! -z "$N8N_DOMAIN" ]; then
    echo "📱 Access n8n at: https://$N8N_DOMAIN"
    echo "📊 Traefik dashboard at: https://traefik.coresystem.com"
    echo "📱 Landing page at: https://app.coresystem.com"
else
    echo "📱 Access services via configured domains in docker-compose.yml"
fi
echo ""
echo "📚 Useful commands:"
echo "  - View logs: docker-compose logs -f"
echo "  - View specific service: docker-compose logs -f n8n"
echo "  - Restart services: docker-compose restart"
echo "  - Stop services: docker-compose down"
echo "  - Setup firewall: ./deploy.sh setup-firewall"
echo ""
echo "🔐 Security recommendations:"
echo "  - Configure UFW firewall: ./deploy.sh setup-firewall"
echo "  - Update IP whitelist in docker-compose.yml for Traefik dashboard"
echo "  - Rotate secrets regularly"
echo "  - Monitor Traefik access logs in traefik_logs volume"
echo ""
echo "⚠️  Note: SSL certificate generation may take a few minutes"
echo "   You can check progress with: docker-compose logs -f traefik"
echo ""
