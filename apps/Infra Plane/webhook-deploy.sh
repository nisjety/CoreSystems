#!/bin/bash
#
# Aquatiq Root Container Auto-Deployment Webhook Receiver
# Listens for GitHub webhook events and triggers deployment
#

set -e

REPO_DIR="/opt/coresystem"
LOG_FILE="/var/log/coresystem-webhook.log"
BRANCH="main"
WEBHOOK_SECRET="${GITHUB_WEBHOOK_SECRET:-}"
PORT="${WEBHOOK_PORT:-9000}"

# Logging function
log() {
    echo "[$(date +'%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"
}

# Verify webhook signature
verify_signature() {
    local payload="$1"
    local signature="$2"
    
    if [ -z "$WEBHOOK_SECRET" ]; then
        log "WARNING: GITHUB_WEBHOOK_SECRET not set, skipping signature verification"
        return 0
    fi
    
    local expected_sig="sha256=$(echo -n "$payload" | openssl dgst -sha256 -hmac "$WEBHOOK_SECRET" | cut -d' ' -f2)"
    
    if [ "$signature" != "$expected_sig" ]; then
        log "ERROR: Invalid webhook signature"
        return 1
    fi
    
    return 0
}

# Deploy function
deploy() {
    log "Starting deployment..."
    
    cd "$REPO_DIR"
    
    # Fetch latest changes
    log "Fetching latest changes from $BRANCH branch..."
    git fetch origin "$BRANCH"
    
    # Get current and new commit hashes
    CURRENT_COMMIT=$(git rev-parse HEAD 2>/dev/null || echo "")
    NEW_COMMIT=$(git rev-parse "origin/$BRANCH")
    
    if [ "$CURRENT_COMMIT" = "$NEW_COMMIT" ]; then
        log "Already up to date with commit $NEW_COMMIT"
        return 0
    fi
    
    log "Updating from $CURRENT_COMMIT to $NEW_COMMIT"
    
    # Stash any local changes
    git stash push -m "auto-stash-$(date +%s)" || true
    
    # Checkout latest
    git checkout "$BRANCH"
    git reset --hard "origin/$BRANCH"
    
    # Sync certificates and configs
    log "Syncing certificates and configs..."
    [ -d "cloudflare-certs" ] && chmod 644 cloudflare-certs/* 2>/dev/null || true
    
    # Fix permissions on executable scripts
    chmod +x *.sh 2>/dev/null || true
    chmod +x coresystem-gateway/*.sh 2>/dev/null || true
    
    # Rebuild and restart affected services
    log "Redeploying services..."
    docker compose -f docker-compose.yml build --no-cache 2>&1 | grep -E "(Building|Successfully)" || true
    docker compose -f docker-compose.yml up -d 2>&1 | grep -E "(Creating|Recreate|Starting|Started)" || true
    
    log "Deployment completed successfully"
    
    # Verify health
    log "Verifying service health..."
    sleep 10
    docker compose -f docker-compose.yml ps --format "table {{.Service}}\t{{.Status}}"
    
    return 0
}

# Handle webhook requests
handle_webhook() {
    local payload="${1}"
    local signature="${2}"
    
    log "Received webhook event"
    
    # Verify signature
    if ! verify_signature "$payload" "$signature"; then
        return 1
    fi
    
    # Extract branch from payload
    local ref=$(echo "$payload" | grep -o '"ref":"[^"]*' | cut -d'"' -f4)
    
    log "Webhook ref: $ref"
    
    # Only deploy on root branch pushes
    if [ "$ref" != "refs/heads/$BRANCH" ]; then
        log "Skipping: webhook is for branch $ref (expected refs/heads/$BRANCH)"
        return 0
    fi
    
    # Trigger deployment
    deploy
}

# Simple HTTP server to receive webhooks
start_webhook_server() {
    log "Starting webhook server on port $PORT"
    
    while true; do
        {
            read -r method path protocol
            
            if [ "$method" != "POST" ]; then
                echo "HTTP/1.1 405 Method Not Allowed"
                echo "Content-Type: text/plain"
                echo ""
                echo "Only POST is supported"
                continue
            fi
            
            # Read headers
            declare -A headers
            while read -r line; do
                line=$(echo "$line" | tr -d '\r')
                if [ -z "$line" ]; then
                    break
                fi
                key="${line%%:*}"
                value="${line#*: }"
                headers["$key"]="$value"
            done
            
            # Read body
            content_length=${headers["Content-Length"]:-0}
            payload=""
            if [ "$content_length" -gt 0 ]; then
                read -N "$content_length" payload
            fi
            
            # Extract signature
            signature="${headers["X-Hub-Signature-256"]:-}"
            
            # Process webhook
            if handle_webhook "$payload" "$signature"; then
                echo "HTTP/1.1 200 OK"
                echo "Content-Type: application/json"
                echo ""
                echo '{"status":"success","message":"Deployment triggered"}'
            else
                echo "HTTP/1.1 400 Bad Request"
                echo "Content-Type: application/json"
                echo ""
                echo '{"status":"error","message":"Webhook verification failed"}'
            fi
        } | nc -l -p "$PORT" -q 1
    done
}

# Main
mkdir -p "$(dirname "$LOG_FILE")"

case "${1:-server}" in
    server)
        start_webhook_server
        ;;
    deploy)
        deploy
        ;;
    *)
        echo "Usage: $0 {server|deploy}"
        exit 1
        ;;
esac
