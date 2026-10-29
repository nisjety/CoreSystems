#!/bin/bash
#
# Setup script for GitHub webhook auto-deployment on Aquatiq VPS
# This script:
# 1. Creates a webhook secret
# 2. Sets up the systemd service
# 3. Configures firewall rules
# 4. Instructs user to add GitHub webhook
#

set -e

WEBHOOK_PORT="9000"
WEBHOOK_SECRET_FILE="/opt/coresystem/.webhook-secret"
SERVICE_NAME="coresystem-webhook"

echo "=== Aquatiq Webhook Auto-Deployment Setup ==="

# Generate webhook secret
if [ ! -f "$WEBHOOK_SECRET_FILE" ]; then
    WEBHOOK_SECRET=$(openssl rand -hex 32)
    echo "$WEBHOOK_SECRET" > "$WEBHOOK_SECRET_FILE"
    chmod 600 "$WEBHOOK_SECRET_FILE"
    echo "✓ Generated webhook secret"
else
    WEBHOOK_SECRET=$(cat "$WEBHOOK_SECRET_FILE")
    echo "✓ Using existing webhook secret"
fi

# Copy webhook script to /opt/coresystem
if [ ! -f "/opt/coresystem/webhook-deploy.sh" ]; then
    echo "ERROR: webhook-deploy.sh not found in current directory"
    echo "Please run this script from the coresystem-root-container directory"
    exit 1
fi

chmod +x /opt/coresystem/webhook-deploy.sh
echo "✓ Made webhook script executable"

# Copy systemd service file
sudo cp coresystem-webhook.service "/etc/systemd/system/${SERVICE_NAME}.service"
sudo systemctl daemon-reload
echo "✓ Installed systemd service"

# Enable and start service
sudo systemctl enable "$SERVICE_NAME"
sudo systemctl start "$SERVICE_NAME"
echo "✓ Started webhook service"

# Configure firewall
if command -v ufw &> /dev/null; then
    # Only allow webhook from GitHub IP ranges
    sudo ufw allow from 140.82.112.0/20 to any port $WEBHOOK_PORT comment "GitHub Webhook"
    sudo ufw allow from 143.55.64.0/20 to any port $WEBHOOK_PORT comment "GitHub Webhook"
    sudo ufw allow from 192.30.252.0/22 to any port $WEBHOOK_PORT comment "GitHub Webhook"
    sudo ufw allow from 185.199.108.0/22 to any port $WEBHOOK_PORT comment "GitHub Webhook"
    echo "✓ Configured firewall for GitHub webhooks"
else
    echo "⚠ UFW not found, please configure firewall manually"
    echo "  Allow inbound on port $WEBHOOK_PORT from GitHub"
fi

# Check service status
echo ""
echo "=== Service Status ==="
sudo systemctl status "$SERVICE_NAME" --no-pager

# Display webhook information
echo ""
echo "=== GitHub Webhook Configuration ==="
echo "Webhook URL: http://<your-vps-ip>:$WEBHOOK_PORT"
echo "Webhook Secret: $WEBHOOK_SECRET"
echo ""
echo "To add webhook to GitHub:"
echo "1. Go to https://github.com/I-Dacosta/Aquatiq/settings/hooks"
echo "2. Click 'Add webhook'"
echo "3. Set Payload URL to: http://<your-vps-ip>:$WEBHOOK_PORT"
echo "4. Set Secret to: $WEBHOOK_SECRET"
echo "5. Select 'Let me select individual events'"
echo "6. Check: ✓ Push events"
echo "7. Check: ✓ Active"
echo "8. Click 'Add webhook'"
echo ""
echo "=== Logs ==="
echo "View webhook logs with:"
echo "  sudo journalctl -u $SERVICE_NAME -f"
