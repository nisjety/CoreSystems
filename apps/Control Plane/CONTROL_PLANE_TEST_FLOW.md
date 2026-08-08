# Control Plane Test Flow

This document outlines the expected testing sequence for the Control Plane once Docker services are running.
The Control Plane root `.env` and `.env.example` do not exist. Use
`./scripts/run-control-plane.sh` for every local Compose command; it layers the
six service-local env files and creates only a temporary interpolation file.

## Prerequisites

- ✅ Docker daemon healthy and running
- ✅ Docker images pulled by the runner
- ✅ Six core-local `.env`/`.env.example` contracts validated

## Startup Order & Health Checks

### 1. Infrastructure Services (must start first)

Command:
```bash
./scripts/run-control-plane.sh up -d controlplane-postgres controlplane-dragonfly controlplane-nats
```

Expected status after 15 seconds:
```
NAME                    CONTAINER ID   STATUS                     PORTS
controlplane-postgres   xxxxx          Up 15s (healthy)           127.0.0.1:5433->5432/tcp
controlplane-dragonfly  xxxxx          Up 15s (healthy)           127.0.0.1:6380->6379/tcp
controlplane-nats       xxxxx          Up 15s (healthy)           127.0.0.1:4223->4222/tcp
```

Verify health:
```bash
# PostgreSQL
psql -h localhost -U coresystem -d postgres -c "SELECT version();"

# Dragonfly (Redis protocol)
redis-cli -p 6380 PING
# Expected: PONG

# NATS
curl -s http://localhost:8222/healthz | jq .
# Expected: { "ok": true }
```

---

### 2. Control Plane Services (depend on infrastructure)

Command:
```bash
./scripts/run-control-plane.sh up -d auth-core user-core org-core
```

Expected status after 45 seconds:
```
NAME           CONTAINER ID   STATUS                     PORTS
auth-core      xxxxx          Up 45s (healthy: starting) 0.0.0.0:3011->3011/tcp, 0.0.0.0:50011->50011/tcp
user-core      xxxxx          Up 45s (healthy: starting) 0.0.0.0:3012->3012/tcp, 0.0.0.0:50012->50012/tcp
org-core       xxxxx          Up 45s (healthy: starting) 0.0.0.0:8080->8080/tcp, 0.0.0.0:9090->9090/tcp
```

---

## Service-to-Service Communication Tests

### 3. Auth Service → User Service (Internal gRPC)

Auth service calls user service via `user-core:50012` (internal Docker DNS).

Verify in auth-core logs:
```bash
./scripts/run-control-plane.sh logs auth-core | grep -i "user service\|grpc"
# Expected: "User service gRPC URL: user-core:50012"
```

### 4. Org Service → Auth Service (HTTP)

Org service calls auth service via `http://auth-core:3011` for policy validation.

Verify in org-core logs:
```bash
./scripts/run-control-plane.sh logs org-core | grep -i "auth\|http"
# Expected: "AUTH_SERVICE_URL=http://auth-core:3011"
```

### 5. NATS Event Stream Connectivity

All services publish/subscribe to NATS at `nats://controlplane-nats:4222`.

Verify NATS streams created:
```bash
docker exec controlplane-nats nats stream list -s nats://localhost:4222
# Expected streams:
# - USER_EVENTS
# - ORGANIZATION_EVENTS
```

---

## API Endpoint Tests

### 6. Health Checks (HTTP)

```bash
# Auth service health
curl -s http://localhost:3011/api/auth/get-session | jq .

# Org service health
curl -s http://localhost:8080/health | jq .

# User service gRPC health (requires grpcurl)
grpcurl -plaintext localhost:50012 grpc.health.v1.Health/Check
```

### 7. Auth Service - Sign Up Flow

```bash
# Create user
curl -X POST http://localhost:3011/api/auth/sign-up \
  -H "Content-Type: application/json" \
  -d '{
    "email": "test@example.com",
    "password": "SecurePassword123!",
    "name": "Test User"
  }'

# Expected response:
# {
#   "user": { "id": "...", "email": "test@example.com", "name": "Test User" },
#   "session": { "token": "...", "expiresAt": "..." }
# }
```

### 8. User Service - Get Current User Profile

```bash
# Using JWT token from sign-up
JWT_TOKEN="<token from step 7>"

curl -s http://localhost:3012/api/v1/users/me \
  -H "Authorization: Bearer $JWT_TOKEN" | jq .

# Expected response:
# {
#   "id": "...",
#   "email": "test@example.com",
#   "name": "Test User",
#   "preferences": { ... }
# }
```

### 9. Org Service - Create Organization

```bash
# Org service requires internal request context (user ID)
# This typically flows through API gateway or auth-to-org relay

curl -X POST http://localhost:8080/api/organizations \
  -H "Content-Type: application/json" \
  -H "X-User-ID: <user-id-from-sign-up>" \
  -d '{
    "name": "Test Org",
    "slug": "test-org",
    "plan": "starter"
  }'

# Expected response:
# {
#   "id": "...",
#   "name": "Test Org",
#   "slug": "test-org",
#   "owner": { "id": "...", "email": "test@example.com" }
# }
```

---

## Database Initialization Verification

### 10. Check Postgres Databases Created

```bash
# Connect to postgres
psql -h localhost -U coresystem -d postgres -c "
  SELECT datname 
  FROM pg_database 
  WHERE datname IN ('auth_service', 'user_service', 'org_core')
  ORDER BY datname;
"

# Expected output:
#   datname
# ---------------
#  auth_service
#  org_core
#  user_service
# (3 rows)
```

### 11. Check Auth Service Schema

```bash
psql -h localhost -U coresystem -d auth_service -c "
  SELECT tablename 
  FROM pg_tables 
  WHERE schemaname='public' 
  LIMIT 5;
"

# Expected tables:
#         tablename
# ________________________
#  account
#  session
#  organization
#  organization_members
#  invite (or similar)
```

---

## Cross-Boundary Test: Event Publishing

### 12. Verify Auth Events Published to NATS

When user signs up, auth-core publishes to NATS subjects:
- `user.created`
- `auth.session.started`

Subscribe and monitor:
```bash
# Terminal 1: Subscribe to user events
docker exec controlplane-nats \
  nats sub "user.*" \
  -s nats://localhost:4222 \
  --raw

# Terminal 2: Trigger sign-up (step 7 above)

# Expected output in Terminal 1:
# [user.created] {
#   "userId": "...",
#   "email": "test@example.com",
#   "timestamp": "2026-02-19T...",
#   "traceId": "...",
#   "correlationId": "..."
# }
```

---

## Cleanup

Once testing is complete:

```bash
./scripts/run-control-plane.sh down --remove-orphans
```

---

## Expected Behavior Summary

| Layer | Service | Port | Role | Expected Health |
|-------|---------|------|------|-----------------|
| Infra | PostgreSQL | 5432 | Shared data store | healthy ✓ |
| Infra | Dragonfly | 6379 | Session/cache | healthy ✓ |
| Infra | NATS | 4222 | Event stream | healthy ✓ |
| Control | auth-core | 3011/50011 | Identity & sessions | healthy ✓ |
| Control | user-core | 3012/50012 | User profiles & keys | healthy ✓ |
| Control | org-core | 8080/9090/9091 | Org metadata & policy | healthy ✓ |

---

## Troubleshooting

### Service fails to connect to PostgreSQL
- Check: `./scripts/run-control-plane.sh logs user-core` for `hostname resolving error`
- Verify: `controlplane-net` and `inter-plane-bus` exist: `docker network ls | grep -E 'controlplane-net|inter-plane-bus'`
- Restart: `./scripts/run-control-plane.sh up -d`

### NATS connection refused
- Verify: `docker exec controlplane-nats nc -zv localhost 4222`
- Check: `./scripts/run-control-plane.sh config --quiet`

### gRPC call fails
- Verify grpcurl installed: `grpcurl -version`
- Use plaintext: `grpcurl -plaintext localhost:50012 list`

### Dragonfly auth fails
- Verify the service-local Dragonfly credential and rerun the runner
- Test: `redis-cli -h localhost -p 6380 ping`
