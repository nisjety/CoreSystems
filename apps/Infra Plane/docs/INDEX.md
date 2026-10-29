# Aquatiq Root Container - Documentation Index

Welcome to the Aquatiq Root Container documentation! This system provides a production-ready infrastructure foundation for all your applications.

## 📚 Documentation Structure

### Quick Start
- **[README.md](../README.md)** - Main documentation with setup, usage, and deployment guides

### Detailed Guides
- **[SECURITY.md](SECURITY.md)** - Complete security architecture, best practices, and incident response
- **[NETWORKING.md](NETWORKING.md)** - Network topology, service discovery, and connection examples
- **[ADDING_SUBDOMAINS.md](ADDING_SUBDOMAINS.md)** - Guide for adding new subdomains to Traefik
- **[SUPERSET_SUBDOMAIN_SETUP.md](SUPERSET_SUBDOMAIN_SETUP.md)** - Superset subdomain configuration
- **[CONTRACT_MANAGER_SUBDOMAIN_SETUP.md](CONTRACT_MANAGER_SUBDOMAIN_SETUP.md)** - Contract Manager subdomain configuration

### Configuration Files
- **[.env.example](../.env.example)** - Template for production environment variables
- **[.env.local](../.env.local)** - Local development credentials (simple passwords)

### Deployment
- **[deploy.sh](../deploy.sh)** - Automated VPS deployment script
- **[start-local.sh](../start-local.sh)** - Local development quick start
- **[generate-secrets.sh](../generate-secrets.sh)** - Production secret generation

---

## 🎯 Documentation by Role

### For Developers

**Getting Started:**
1. Read [Quick Start (Local Development)](../README.md#-getting-started)
2. Review [Network Architecture](NETWORKING.md#network-overview)
3. Check [Connection Examples](NETWORKING.md#connection-examples)

**Common Tasks:**
- [Connect your app](NETWORKING.md#connecting-your-app)
- [Access shared services](../README.md#-usage-scenarios)
- [Troubleshoot connections](NETWORKING.md#troubleshooting)

### For DevOps/SRE

**Deployment:**
1. Review [Security Architecture](SECURITY.md#security-architecture)
2. Follow [Production Deployment](../README.md#-production-deployment-docker-composeyml-or-docker-composeflexibleyml)
3. Configure [Firewall Rules](SECURITY.md#firewall-rules)

**Operations:**
- [Secrets Management](SECURITY.md#secrets-management)
- [SSL/TLS Configuration](SECURITY.md#ssltls-configuration)
- [Incident Response](SECURITY.md#incident-response)
- [Adding Subdomains](ADDING_SUBDOMAINS.md)
- [Superset Setup](SUPERSET_SUBDOMAIN_SETUP.md)
- [Contract Manager Setup](CONTRACT_MANAGER_SUBDOMAIN_SETUP.md)

### For Security Team

**Security Features:**
- [Defense in Depth](SECURITY.md#defense-in-depth)
- [Access Control](SECURITY.md#access-control)
- [Audit Logging](SECURITY.md#audit-logging)
- [Security Checklist](SECURITY.md#security-checklist)

**Compliance:**
- [Data Protection](SECURITY.md#compliance)
- [Security Standards](SECURITY.md#compliance)

---

## 📖 Documentation by Topic

### Architecture
- [System Architecture](../README.md#-architecture)
- [Network Topology](NETWORKING.md#network-topology)
- [Security Architecture](SECURITY.md#security-architecture)

### Configuration
- [Environment Variables](../README.md#environment-files)
- [Secrets Management](SECURITY.md#secrets-management)
- [Port Configuration](../README.md#-port-configuration)

### Security
- [Network Security](SECURITY.md#network-security)
- [SSL/TLS Setup](SECURITY.md#ssltls-configuration)
- [Rate Limiting](SECURITY.md#rate-limiting)
- [IP Whitelisting](SECURITY.md#ip-whitelisting)

### Networking
- [Service Discovery](NETWORKING.md#service-discovery)
- [Multi-Network Design](NETWORKING.md#multi-network-design)
- [External Access](NETWORKING.md#external-access)
- [Adding Subdomains](ADDING_SUBDOMAINS.md)
- [Traefik Configuration](ADDING_SUBDOMAINS.md#label-reference)

### Operations
- [Deployment](../README.md#-deployment)
- [Troubleshooting](../README.md#-troubleshooting)
- [Performance Benchmarks](../README.md#-performance--benchmarks)

---

## 🔧 Quick Reference

### Default Credentials (Local Development)

| Service | Username | Password | Connection |
|---------|----------|----------|------------|
| PostgreSQL | `coresystem` | `postgres` | `postgres://coresystem:postgres@localhost:5432/coresystem_dev` |
| Redis | - | `redis` | `redis://:redis@localhost:6379` |
| NATS | - | `nats` | `nats://localhost:4222?token=nats` |
| pgAdmin | `admin@coresystem.com` | `admin` | http://localhost:5050 |
| Gateway | - | `gateway` (API key) | http://localhost:7500 |

⚠️ **Never use these in production!**

### Service Ports

**Local Development:**
- PostgreSQL: `127.0.0.1:5432`
- Redis: `127.0.0.1:6379`
- NATS: `127.0.0.1:4222`
- n8n: `127.0.0.1:5678`
- pgAdmin: `127.0.0.1:5050`
- RedisInsight: `127.0.0.1:5540`
- Gateway REST: `127.0.0.1:7500`
- Gateway gRPC: `127.0.0.1:50051`

**Production:**
- All services accessed via domain names through Traefik
- No direct port exposure to internet

### Useful Commands

```bash
# Start local stack
./start-local.sh

# Stop local stack
docker compose -f docker-compose.local.yml --env-file .env.local down

# View logs
docker compose -f docker-compose.local.yml logs -f [service]

# Check service health
docker ps --filter "name=coresystem"

# Deploy to production
./deploy.sh root@your-vps

# Generate production secrets
./generate-secrets.sh

# Test database connection
docker exec -it coresystem-postgres psql -U coresystem -d coresystem_dev
```

---

## 🆘 Getting Help

### Troubleshooting Guides
- [Service Discovery Issues](NETWORKING.md#service-discovery-issues)
- [Connection Timeouts](NETWORKING.md#connection-timeouts)
- [Port Conflicts](NETWORKING.md#port-conflicts)
- [General Troubleshooting](../README.md#-troubleshooting)

### Support Channels
- **Documentation Issues**: Create PR with improvements
- **Security Concerns**: security@coresystem.com
- **Technical Support**: Open issue in repository

---

## 📝 Contributing to Documentation

### Documentation Standards

**File Structure:**
```
docs/
├── INDEX.md           # This file
├── SECURITY.md        # Security documentation
└── NETWORKING.md      # Network documentation
```

**Writing Guidelines:**
- Use clear, concise language
- Include code examples
- Add diagrams where helpful
- Keep credentials out of docs
- Update version/date at bottom

**Example Section:**
````markdown
## Section Title

Brief introduction paragraph.

### Subsection

Explanation with example:

```language
code example
```

**Key Points:**
- Point one
- Point two
````

### Updating Documentation

1. Make changes in appropriate file
2. Update "Last Updated" date
3. Add to CHANGELOG if significant
4. Test all code examples
5. Submit for review

---

## 🗺️ Documentation Roadmap

### Planned Documentation
- [ ] API Reference Guide (Gateway REST/gRPC)
- [ ] Database Migration Guide
- [ ] Backup & Recovery Procedures
- [ ] Monitoring & Alerting Setup
- [ ] Kubernetes Deployment Guide
- [ ] Multi-Region Setup
- [ ] Disaster Recovery Plan

### Recent Updates
- ✅ Subdomain configuration guides (Feb 2026)
- ✅ Superset integration guide (Feb 2026)
- ✅ Contract Manager integration guide (Feb 2026)
- ✅ Complete security documentation (Nov 2025)
- ✅ Comprehensive networking guide (Nov 2025)
- ✅ Enhanced README with credentials (Nov 2025)
- ✅ Production deployment procedures (Nov 2025)

---

## 📄 License

Internal Aquatiq infrastructure documentation. All rights reserved.

---

**Version:** 1.1  
**Last Updated:** February 12, 2026  
**Maintained By:** Aquatiq Infrastructure Team
