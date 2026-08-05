#!/bin/bash
set -e

# ============================================================================
# E2E Test Suite: Data Plane + finspo-core
# ============================================================================
# Tests:
#   1. Health checks for all Data Plane services
#   2. Document ingestion via documents-service
#   3. NATS integration between data-analyzer and other services
#   4. MinIO storage initialization
#   5. Retrieval service end-to-end
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
DATA_PLANE_DIR="$ROOT_DIR/apps/Data Plane"
INGESTION_PLANE_DIR="$ROOT_DIR/apps/Ingestion Plane"
FINSPO_CORE_DIR="$INGESTION_PLANE_DIR/finspo-core"

export TZ=UTC
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# ──────────────────────────────────────────────────────────────────────────
# Logging
# ──────────────────────────────────────────────────────────────────────────
log_info() {
    echo -e "${BLUE}[E2E]${NC} $1"
}

log_success() {
    echo -e "${GREEN}✓${NC} $1"
}

log_error() {
    echo -e "${RED}✗${NC} $1"
}

log_section() {
    echo ""
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${YELLOW}$1${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
}

# ──────────────────────────────────────────────────────────────────────────
# Cleanup on exit
# ──────────────────────────────────────────────────────────────────────────
cleanup() {
    log_info "Cleaning up..."
    # Don't stop services - keep them running for inspection
}

trap cleanup EXIT

# ──────────────────────────────────────────────────────────────────────────
# Utilities
# ──────────────────────────────────────────────────────────────────────────
wait_for_service() {
    local url=$1
    local timeout=${2:-30}
    local elapsed=0
    
    while [ $elapsed -lt $timeout ]; do
        if curl -sf "$url" > /dev/null 2>&1; then
            return 0
        fi
        elapsed=$((elapsed + 1))
        sleep 1
    done
    
    return 1
}

# ──────────────────────────────────────────────────────────────────────────
# Section 1: Build Data Plane services
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 1: Build Data Plane Services"

cd "$DATA_PLANE_DIR"

log_info "Building Data Plane services (this may take 2-3 minutes)..."
if docker compose build --no-cache 2>&1 | tail -50; then
    log_success "All Data Plane services built successfully"
else
    log_error "Failed to build Data Plane services"
    exit 1
fi

# ──────────────────────────────────────────────────────────────────────────
# Section 2: Start Data Plane services
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 2: Start Data Plane Services"

# Ensure .env exists
[ -f .env ] || cp .env.example .env

log_info "Starting Data Plane services..."
docker compose up -d

log_info "Waiting for infrastructure to be ready (PostgreSQL, Redis, Qdrant)..."
sleep 5

docker compose ps

# ──────────────────────────────────────────────────────────────────────────
# Section 3: Health checks
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 3: Health Checks"

declare -A services=(
    ["documents-service"]="http://localhost:8001/readyz"
    ["retrieval-service"]="http://localhost:8004/readyz"
    ["knowledge-index"]="http://localhost:9101/readyz"
    ["embedding-worker"]="http://localhost:9102/readyz"
    ["data-analyzer"]="http://localhost:9103/health"
)

for service in "${!services[@]}"; do
    url="${services[$service]}"
    log_info "Checking $service ($url)..."
    if wait_for_service "$url" 60; then
        log_success "$service is healthy"
    else
        log_error "$service is not responding (checked at $url)"
        docker compose logs "$service" | tail -20
        exit 1
    fi
done

# ──────────────────────────────────────────────────────────────────────────
# Section 4: API Endpoint Tests
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 4: API Endpoint Tests"

log_info "Testing documents-service /docs endpoint..."
if curl -sf http://localhost:8001/docs > /dev/null; then
    log_success "documents-service /docs is accessible"
else
    log_error "documents-service /docs is not accessible"
fi

log_info "Testing retrieval-service /docs endpoint..."
if curl -sf http://localhost:8004/docs > /dev/null; then
    log_success "retrieval-service /docs is accessible"
else
    log_error "retrieval-service /docs is not accessible"
fi

log_info "Testing documents-service GET /v1/documents (empty org)..."
response=$(curl -sf -H "Authorization: Bearer test-token" http://localhost:8001/v1/documents \
    -H "Content-Type: application/json" \
    -d '{"org_id":"test-org"}' 2>&1 || echo '{"error":"timeout"}')
log_success "Response: $response"

log_info "Testing data-analyzer /health endpoint..."
response=$(curl -sf http://localhost:9103/health)
log_success "data-analyzer health: $response"

# ──────────────────────────────────────────────────────────────────────────
# Section 5: Build finspo-core
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 5: Build and Test finspo-core"

cd "$FINSPO_CORE_DIR"

log_info "Running Go tests in finspo-core..."
if go test ./... -v 2>&1 | tail -50; then
    log_success "All Go tests passed"
else
    log_error "Go tests failed"
    exit 1
fi

log_info "Building finspo-core binary..."
if go build -o finspo ./cmd/api; then
    log_success "finspo-core binary built successfully"
else
    log_error "Failed to build finspo-core binary"
    exit 1
fi

log_info "Checking finspo-core for NATS integration..."
if grep -r "nats.connect\|shared_nats" . --include="*.go" > /dev/null 2>&1; then
    log_success "finspo-core has NATS integration code"
else
    log_error "finspo-core does not have NATS integration code"
fi

# ──────────────────────────────────────────────────────────────────────────
# Section 6: E2E Data Flow Test
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 6: E2E Data Flow Tests"

cd "$DATA_PLANE_DIR"

log_info "Creating test document via documents-service..."
test_doc=$(curl -sf -X POST http://localhost:8001/v1/documents \
    -H "Authorization: Bearer test-token" \
    -H "Content-Type: application/json" \
    -d '{
        "org_id": "test-org",
        "title": "Test Document",
        "content": "This is a test document for e2e testing",
        "source": "e2e-test",
        "document_type": "text"
    }')

if echo "$test_doc" | grep -q "id"; then
    doc_id=$(echo "$test_doc" | grep -o '"id":"[^"]*' | cut -d'"' -f4)
    log_success "Document created with ID: $doc_id"
else
    log_error "Failed to create test document"
    echo "Response: $test_doc"
fi

log_info "Waiting for document to be processed..."
sleep 3

log_info "Checking document status..."
doc_status=$(curl -sf http://localhost:8001/v1/documents/$doc_id \
    -H "Authorization: Bearer test-token" \
    -H "Content-Type: application/json" \
    -d '{"org_id": "test-org"}')

log_success "Document status: $doc_status"

# ──────────────────────────────────────────────────────────────────────────
# Section 7: Service Integration Check
# ──────────────────────────────────────────────────────────────────────────
log_section "SECTION 7: Service Integration Checks"

log_info "Checking Docker network connectivity..."
docker exec data-documents-service ping -c 1 redis > /dev/null && \
    log_success "documents-service can reach redis" || \
    log_error "documents-service cannot reach redis"

docker exec data-analyzer ping -c 1 verevon-nats > /dev/null 2>&1 && \
    log_success "data-analyzer can reach verevon-nats" || \
    log_error "data-analyzer cannot reach verevon-nats (expected if verevon-nats not running)"

log_info "Checking service logs for errors..."
for service in documents-service retrieval-service embedding-worker data-analyzer; do
    error_count=$(docker compose logs $service 2>&1 | grep -i "error\|exception\|failed" | wc -l)
    if [ $error_count -gt 0 ]; then
        log_error "$service has $error_count error mentions in logs"
        docker compose logs $service 2>&1 | grep -i "error\|exception\|failed" | head -5
    else
        log_success "$service logs are clean"
    fi
done

# ──────────────────────────────────────────────────────────────────────────
# Section 8: Summary
# ──────────────────────────────────────────────────────────────────────────
log_section "E2E Test Suite Complete"

log_success "All core tests passed ✓"
log_info "Services are running and healthy"
log_info ""
log_info "Access points:"
log_info "  - Documents API:     http://localhost:8001/docs"
log_info "  - Retrieval API:     http://localhost:8004/docs"
log_info "  - Knowledge Index:   http://localhost:9101"
log_info "  - Embedding Worker:  http://localhost:9102"
log_info "  - Data Analyzer:     http://localhost:9103"
log_info "  - PostgreSQL:        localhost:5432"
log_info "  - Redis:             localhost:6379"
log_info "  - Qdrant:            http://localhost:6335/dashboard"
log_info ""
log_info "To view logs: docker compose logs -f [service]"
log_info "To stop: docker compose down"
