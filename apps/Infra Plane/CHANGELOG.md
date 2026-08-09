# Changelog

All notable changes to the Aquatiq Root Container system will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2025-11-25

### 🚀 Major Changes

#### Security Enhancements
- **Traefik Middleware Migration**: Moved middleware definitions from Docker labels to file provider
  - All middlewares now in `/cloudflare-certs/middlewares.yml`
  - Auto-reload on file changes
  - References changed from `@docker` to `@file` suffix
- **IP Whitelist Updated**: Added trusted IP `77.106.153.146/32`
- **SSL/TLS Fixed**: Corrected Cloudflare origin certificate formatting
  - Removed extra line breaks causing parsing errors
  - Certificate now properly validated (valid until 2040)
- **Firewall Configuration**: UFW properly configured with Cloudflare IP ranges only

#### Service Updates
- **RedisInsight**: Replaced redis-commander with RedisInsight
  - Better performance and modern UI
  - ARM64 support
  - More reliable authentication
- **Traefik Health Check**: Added ping endpoint for proper health monitoring
  - `--ping=true` enabled
  - `--ping.entryPoint=traefik` configured

#### Documentation
- **SECURITY.md**: Complete security architecture documentation
  - Defense in depth strategy
  - Incident response procedures
  - Compliance guidelines
  - Security checklist
- **NETWORKING.md**: Comprehensive networking guide
  - Service discovery patterns
  - Connection examples for all languages
  - Multi-network design explained
  - Troubleshooting guides
- **docs/INDEX.md**: Documentation navigation hub
- **Enhanced README**: Updated with complete feature set

### ✅ Fixed
- Traefik middleware "does not exist" errors resolved
- Origin certificate parsing issues fixed
- Healthcheck failures corrected
- Configuration drift between local and production eliminated

### 🔒 Security
- All services now behind proper rate limiting
- IP whitelisting active on admin endpoints
- Cloudflare Full (Strict) SSL mode operational
- Docker secrets properly configured with correct permissions

### 📦 Infrastructure
- All 13 services running healthy
- Zero errors in production logs
- Middleware auto-reload working
- Networks properly segmented

---

## [1.5.0] - 2025-11-23

### Added
- Local development environment with simple credentials
- `.env.local` file for development passwords
- `start-local.sh` script for quick local setup
- Multiple database support (n8n, coresystem_risk, coresystem_dev)

### Changed
- Separated development and production configurations
- Port bindings restricted to 127.0.0.1 for local security
- Network naming: `coresystem-local` and `ima-local`

### Security
- Production secrets moved to Docker secrets
- Development uses simple passwords (documented)
- No production credentials in repository

---

## [1.0.0] - 2025-11-14

### Added
- Initial production deployment
- Core services: PostgreSQL, Redis, NATS, n8n
- Aquatiq Gateway (REST/gRPC API)
- Management tools: pgAdmin, Redis Commander
- Docker socket proxy for security
- Cloudflare integration
- Traefik reverse proxy
- Rate limiting middleware
- Audit logging
- Multi-database PostgreSQL setup

### Infrastructure
- `docker-compose.yml` - Production (Full Strict SSL)
- `docker-compose.flexible.yml` - Production (Flexible SSL)
- UFW firewall configuration
- Prometheus monitoring setup
- NTP time synchronization

### Security
- IP whitelisting
- Rate limiting (100 req/min global, 20 req/min admin)
- Docker secrets for credentials
- Read-only Docker socket access
- Security headers (HSTS, XSS protection)

---

## Development Roadmap

### Planned for 2.1.0
- [ ] API documentation for Gateway endpoints
- [ ] Automated backup system for PostgreSQL
- [ ] Grafana dashboard configurations
- [ ] Health check monitoring dashboard
- [ ] Log aggregation with ELK stack

### Planned for 3.0.0
- [ ] Kubernetes deployment manifests
- [ ] Multi-region setup documentation
- [ ] High availability configuration
- [ ] Disaster recovery procedures
- [ ] Performance optimization guide

---

## Migration Guides

### Migrating from 1.x to 2.0

**Middleware Configuration:**
```bash
# Old: Middlewares in Traefik labels with @docker suffix
# New: Middlewares in file provider with @file suffix

# No action needed - already deployed
```

**RedisInsight Migration:**
```bash
# Redis Commander is replaced by RedisInsight
# Old data is preserved in Redis

docker stop coresystem-redis-commander
docker rm coresystem-redis-commander
docker compose up -d redis-insight
```

**Certificate Fix:**
```bash
# If you have custom origin certificates, ensure proper formatting
# No extra line breaks after BEGIN/END markers
```

---

## Support

- **Security Issues**: security@coresystem.com
- **Documentation**: See `docs/INDEX.md`
- **Bug Reports**: Create issue in repository

---

**Maintained By:** Aquatiq Infrastructure Team  
**Repository:** Internal  
**License:** Proprietary
