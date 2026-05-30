# Quarry Deployment Guide

**Version:** 0.1.0  
**Last Updated:** 2026-02-18

---

## Table of Contents

1. [Requirements](#requirements)
2. [Docker Deployment](#docker-deployment)
3. [Kubernetes Deployment](#kubernetes-deployment)
4. [Configuration](#configuration)
5. [Monitoring](#monitoring)
6. [Troubleshooting](#troubleshooting)

---

## Requirements

### Minimum System Requirements

| Resource | Development | Production |
|----------|-------------|------------|
| **CPU** | 2 cores | 4+ cores |
| **RAM** | 4GB | 8GB+ |
| **Disk** | 10GB | 50GB+ SSD |
| **Network** | 10Mbps | 100Mbps+ |

### Software Requirements

- **Docker**: 20.10+ and Docker Compose 2.0+
- **Go**: 1.24+ (for building from source)
- **Chromium**: Included in Docker image
- **(Optional) Kubernetes**: 1.25+

---

## Docker Deployment

### Quick Start

```bash
# Clone repository
git clone https://github.com/triodelab/quarry.git
cd quarry

# Configure environment
cp configs/.env.example configs/.env
# Edit configs/.env with your settings

# Start stack
docker compose up -d

# Check health
curl http://localhost:8090/health
```

### Docker Compose Services

The stack includes:
- **quarry-api**: Main API server (port 8090)
- **quarry-worker**: Temporal workflow worker
- **quarry-temporal**: Temporal server (port 7234)
- **quarry-temporal-ui**: Temporal UI (port 8089)
- **quarry-redis**: Redis cache (port 6380)
- **quarry-postgres**: PostgreSQL database (port 5434)
- **quarry-qdrant**: Vector database (port 6335, 6336)
- **quarry-nats**: NATS messaging (port 4223, 8223)

### Environment Variables

**Required:**
```env
# API Configuration
PORT=8090
QUARRY_API_KEY=your-secure-api-key-here

# ai-core Integration (external service)
AI_CORE_GRPC_ADDR=host.docker.internal:50851
ENABLE_AI_EXTRACTION=true

# Cache Backend
CACHE_BACKEND=redis
REDIS_URL=redis://redis:6379/0

# Job Store
JOB_STORE_BACKEND=postgres
QUARRY_POSTGRES_DSN=postgres://quarry:quarry@postgres:5432/quarry?sslmode=disable

# Temporal
TEMPORAL_ENABLED=true
TEMPORAL_ADDRESS=temporal:7233
TEMPORAL_TASK_QUEUE=quarry-task-queue
```

**Optional:**
```env
# Rate Limiting
RATE_LIMIT_MAX=100
RATE_LIMIT_WINDOW_SEC=60

# Request Timeout
REQUEST_TIMEOUT_SEC=30

# Browser Pooling
BROWSER_POOL_SIZE=5
HEADLESS_BROWSER=true

# Human Delay Simulation
HUMAN_DELAY_ENABLED=false
HUMAN_DELAY_MIN_MS=25
HUMAN_DELAY_MAX_MS=150

# Proxy Rotation
PROXY_ENABLED=false
PROXY_POOL=http://proxy1.com:8080,http://proxy2.com:8080

# Retry Configuration
RETRY_ENABLED=true
RETRY_MAX_ATTEMPTS=3
RETRY_BACKOFF_MS=250

# Job Monitoring
JOB_MONITORING_ENABLED=true
```

### Building from Source

```bash
# Build binary
go build -o ./bin/quarry-api ./cmd/api
go build -o ./bin/quarry-worker ./cmd/worker

# Build Docker image
docker build -t quarry:latest .

# Run container
docker run -p 8090:8090 \
  -e QUARRY_API_KEY=your-key \
  quarry:latest
```

---

## Kubernetes Deployment

### Prerequisites

```bash
# Install kubectl
# Install helm
helm repo add bitnami https://charts.bitnami.com/bitnami
helm repo update
```

### Deploy Dependencies

**1. PostgreSQL:**
```bash
helm install quarry-postgres bitnami/postgresql \
  --set auth.username=quarry \
  --set auth.password=quarry \
  --set auth.database=quarry \
  --set persistence.size=20Gi
```

**2. Redis:**
```bash
helm install quarry-redis bitnami/redis \
  --set auth.enabled=false \
  --set master.persistence.size=10Gi
```

**3. Temporal:**
```bash
helm install quarry-temporal temporalio/temporal \
  --set server.replicaCount=3 \
  --set ui.enabled=true
```

### Deploy Quarry API

**quarry-api-deployment.yaml:**
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: quarry-api
  labels:
    app: quarry-api
spec:
  replicas: 3
  selector:
    matchLabels:
      app: quarry-api
  template:
    metadata:
      labels:
        app: quarry-api
    spec:
      containers:
      - name: quarry-api
        image: triodelab/quarry:latest
        ports:
        - containerPort: 8090
          name: http
        env:
        - name: PORT
          value: "8090"
        - name: QUARRY_API_KEY
          valueFrom:
            secretKeyRef:
              name: quarry-secrets
              key: api-key
        - name: CACHE_BACKEND
          value: "redis"
        - name: REDIS_URL
          value: "redis://quarry-redis-master:6379/0"
        - name: JOB_STORE_BACKEND
          value: "postgres"
        - name: QUARRY_POSTGRES_DSN
          valueFrom:
            secretKeyRef:
              name: quarry-secrets
              key: postgres-dsn
        - name: TEMPORAL_ENABLED
          value: "true"
        - name: TEMPORAL_ADDRESS
          value: "quarry-temporal-frontend:7233"
        - name: AI_CORE_GRPC_ADDR
         value: "ai-core-service:50851"
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8090
          initialDelaySeconds: 10
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8090
          initialDelaySeconds: 5
          periodSeconds: 5
---
apiVersion: v1
kind: Service
metadata:
  name: quarry-api-service
spec:
  selector:
    app: quarry-api
  ports:
  - protocol: TCP
    port: 80
    targetPort: 8090
  type: LoadBalancer
```

Apply:
```bash
kubectl apply -f k8s/quarry-api-deployment.yaml
```

### Deploy Quarry Worker

**quarry-worker-deployment.yaml:**
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: quarry-worker
  labels:
    app: quarry-worker
spec:
  replicas: 5
  selector:
    matchLabels:
      app: quarry-worker
  template:
    metadata:
      labels:
        app: quarry-worker
    spec:
      containers:
      - name: quarry-worker
        image: triodelab/quarry:latest
        command: ["/usr/local/bin/quarry-worker"]
        env:
        - name: TEMPORAL_ENABLED
          value: "true"
        - name: TEMPORAL_ADDRESS
          value: "quarry-temporal-frontend:7233"
        - name: CACHE_BACKEND
          value: "redis"
        - name: REDIS_URL
          value: "redis://quarry-redis-master:6379/0"
        - name: AI_CORE_GRPC_ADDR
          value: "ai-core-service:50851"
        resources:
          requests:
            memory: "1Gi"
            cpu: "1000m"
          limits:
            memory: "4Gi"
            cpu: "4000m"
```

Apply:
```bash
kubectl apply -f k8s/quarry-worker-deployment.yaml
```

### Horizontal Pod Autoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: quarry-api-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: quarry-api
  minReplicas: 3
  maxReplicas: 20
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
```

---

## Configuration

### Configuration Priority (highest to lowest)

1. **Environment Variables**: Override all
2. **Config File**: `configs/prod.yaml` or `configs/dev.yaml`
3. **Defaults**: Hardcoded fallbacks

### Security Configuration

**Create API Key:**
```bash
# Generate secure API key
openssl rand -hex 32

# Set in environment
export QUARRY_API_KEY=your-generated-key
```

**Store Secrets (Kubernetes):**
```bash
kubectl create secret generic quarry-secrets \
  --from-literal=api-key=your-generated-key \
  --from-literal=postgres-dsn='postgres://user:pass@host:5432/db?sslmode=require'
```

### ai-core Integration

Quarry requires an external ai-core gRPC service for intelligent extraction.

**Local Development:**
```env
AI_CORE_GRPC_ADDR=localhost:50851
ENABLE_AI_EXTRACTION=true
```

**Production (Kubernetes):**
```yaml
- name: AI_CORE_GRPC_ADDR
  value: "ai-core-service.ai-core-namespace.svc.cluster.local:50851"
```

If ai-core is unavailable, Quarry automatically falls back to heuristic extraction.

---

## Monitoring

### Metrics Endpoint

```bash
curl http://localhost:8090/metrics
```

**Export to Prometheus:**
```yaml
scrape_configs:
  - job_name: 'quarry'
    static_configs:
      - targets: ['quarry-api-service:8090']
    metrics_path: '/metrics'
```

### Health Checks

**Docker:**
```bash
docker exec quarry-api wget -qO- http://localhost:8090/health
```

**Kubernetes:**
```bash
kubectl exec -it quarry-api-pod -- wget -qO- http://localhost:8090/ready
```

### Logging

**View Logs (Docker):**
```bash
docker logs -f quarry-api
docker logs -f quarry-worker
```

**View Logs (Kubernetes):**
```bash
kubectl logs -f deployment/quarry-api
kubectl logs -f deployment/quarry-worker
```

**Log Aggregation:**
- **Fluentd/Fluent Bit**: Forward to Elasticsearch
- **Loki**: Grafana Loki for log aggregation
- **CloudWatch**: AWS CloudWatch Logs

---

## Troubleshooting

### Common Issues

#### **1. Browser Initialization Fails**

**Symptom:**
```
panic: can't find a browser binary for your OS
```

**Solution:**
- Ensure `ROD_BROWSER_BIN` or `ROD_CHROMIUM_BIN` is set
- Docker: Already configured in Dockerfile
- Local: Install Chromium and set path

```env
export ROD_BROWSER_BIN=/usr/bin/chromium-browser
```

#### **2. ai-core Connection Refused**

**Symptom:**
```json
{
  "ai_health_healthy": false,
  "last_error": "connection refused"
}
```

**Solution:**
- Verify ai-core is running: `docker ps | grep ai-core`
- Check network connectivity: `telnet ai-core-host 50851`
- Update `AI_CORE_GRPC_ADDR` in environment
- Quarry will fall back to heuristic extraction

#### **3. Redis Connection Fails**

**Symptom:**
```
readiness check failed: redis unreachable
```

**Solution:**
- Check Redis is running: `docker ps | grep redis`
- Test connection: `redis-cli -h quarry-redis ping`
- Update `REDIS_URL` in environment
- Fallback: Use `CACHE_BACKEND=memory`

#### **4. Temporal Workflows Not Starting**

**Symptom:**
```
failed to start workflow: rpc error: code = Unavailable
```

**Solution:**
- Check Temporal server: `docker ps | grep temporal`
- Verify `TEMPORAL_ADDRESS` is correct
- Check worker is running: `docker logs quarry-worker`
- Restart worker: `docker restart quarry-worker`

#### **5. Rate Limit Issues**

**Symptom:**
```json
{
  "success": false,
  "error": "rate limit exceeded"
}
```

**Solution:**
- Increase rate limit in config:
```env
RATE_LIMIT_MAX=200
RATE_LIMIT_WINDOW_SEC=60
```
- Use API key for higher limits (per-key vs per-IP)
- Distribute requests over time

### Debug Mode

Enable verbose logging:
```env
LOG_LEVEL=debug
```

### Performance Debugging

**Check container stats:**
```bash
docker stats quarry-api quarry-worker
```

**Memory profiling:**
```bash
# Expose pprof endpoint (development only)
curl http://localhost:8090/debug/pprof/heap > heap.prof
go tool pprof heap.prof
```

**CPU profiling:**
```bash
curl http://localhost:8090/debug/pprof/profile?seconds=30 > cpu.prof
go tool pprof cpu.prof
```

---

## Backup & Recovery

### Database Backup (PostgreSQL)

```bash
# Backup
docker exec quarry-postgres pg_dump -U quarry quarry > backup.sql

# Restore
docker exec -i quarry-postgres psql -U quarry quarry < backup.sql
```

### Redis Backup

```bash
# Trigger save
docker exec quarry-redis redis-cli BGSAVE

# Copy RDB file
docker cp quarry-redis:/data/dump.rdb ./redis-backup.rdb
```

---

## Security Best Practices

1. **Use HTTPS**: Always use TLS in production (Cloudflare, Let's Encrypt)
2. **Rotate API Keys**: Regular rotation (90 days)
3. **Network Isolation**: Firewall rules, VPC/security groups
4. **Secret Management**: Use Kubernetes Secrets or AWS Secrets Manager
5. **Update Dependencies**: Regular `go mod tidy && go get -u`
6. **Monitor Logs**: Alert on errors, security events

---

## Scaling Guidelines

### Vertical Scaling (Single Instance)

- Increase CPU/RAM resources
- Increase `BROWSER_POOL_SIZE`
- Optimize cache TTL values

### Horizontal Scaling (Multiple Instances)

**API Servers:**
- Add more replicas
- Use load balancer (nginx, HAProxy, cloud LB)
- Shared Redis cache

**Workers:**
- Scale based on queue depth
- Monitor Temporal task queue backlog
- Ideal: 1 worker per 100 pending workflows

**Recommended Ratio:**
- **API:Worker** = 1:2 (2 workers per API instance)

---

## Upgrade Process

```bash
# Pull latest image
docker pull triodelab/quarry:latest

# Stop old containers
docker compose down

# Start new containers
docker compose up -d

# Verify health
curl http://localhost:8090/health
```

**Zero-downtime (Kubernetes):**
```bash
kubectl set image deployment/quarry-api \
  quarry-api=triodelab/quarry:v0.2.0

kubectl rollout status deployment/quarry-api
```

---

## Support

- **Documentation**: [docs.quarry.example.com](https://docs.quarry.example.com)
- **GitHub Issues**: [github.com/triodelab/quarry/issues](https://github.com/triodelab/quarry/issues)
- **Slack**: Coming Soon

---

**Last Updated:** 2026-02-18
