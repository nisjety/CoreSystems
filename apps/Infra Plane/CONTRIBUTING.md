# Contributing to Aquatiq Root Container

Thank you for considering contributing to the Aquatiq infrastructure! This document provides guidelines and workflows for contributing to this project.

## 📋 Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Commit Guidelines](#commit-guidelines)
- [Documentation Guidelines](#documentation-guidelines)
- [Testing Requirements](#testing-requirements)
- [Security Guidelines](#security-guidelines)
- [Review Process](#review-process)

---

## Code of Conduct

### Our Standards

- **Be Respectful**: Treat all contributors with respect and professionalism
- **Be Collaborative**: Work together to achieve common goals
- **Be Constructive**: Provide helpful feedback and suggestions
- **Be Security-Conscious**: Never commit secrets, always follow security best practices

### Reporting Issues

Report security vulnerabilities privately to: `security@coresystem.com`

For other issues, create a detailed bug report with:
- Description of the issue
- Steps to reproduce
- Expected vs actual behavior
- Environment details (OS, Docker version, etc.)

---

## Getting Started

### Prerequisites

```bash
# Required tools
- Docker 24.0+
- Docker Compose v2
- Git
- Text editor (VS Code recommended)

# For Gateway development
- Go 1.22+
- Make

# For testing
- curl or httpie
- jq (JSON processing)
```

### Initial Setup

```bash
# 1. Clone the repository (internal)
git clone <repository-url>
cd coresystem-root-container

# 2. Generate local secrets
./generate-secrets.sh dev

# 3. Start local environment
./start-local.sh

# 4. Verify services
docker compose -f docker-compose.local.yml ps
```

---

## Development Workflow

### Branch Strategy

```
main
├── develop (integration branch)
├── feature/* (new features)
├── fix/* (bug fixes)
├── hotfix/* (urgent production fixes)
└── docs/* (documentation updates)
```

### Creating a Feature

```bash
# 1. Create feature branch from develop
git checkout develop
git pull
git checkout -b feature/my-feature

# 2. Make changes
# ... edit files ...

# 3. Test locally
docker compose -f docker-compose.local.yml up -d
# ... verify changes ...

# 4. Commit with conventional format
git add .
git commit -m "feat: add new feature description"

# 5. Push and create PR
git push origin feature/my-feature
```

### Working on the Gateway

```bash
cd coresystem-gateway

# Build
make build

# Run tests
make test

# Format code
make fmt

# Lint
make lint

# Generate protobuf (if modified)
make proto
```

---

## Commit Guidelines

We follow [Conventional Commits](https://www.conventionalcommits.org/) specification.

### Commit Format

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style/formatting (no logic changes)
- **refactor**: Code refactoring (no feature changes)
- **perf**: Performance improvements
- **test**: Adding or updating tests
- **chore**: Maintenance tasks (dependencies, etc.)
- **ci**: CI/CD pipeline changes
- **security**: Security improvements

### Examples

```bash
# Feature
git commit -m "feat(gateway): add rate limiting to admin endpoints"

# Bug fix
git commit -m "fix(traefik): correct middleware references to use @file"

# Documentation
git commit -m "docs(security): add incident response procedures"

# Security
git commit -m "security(redis): enforce password authentication"

# Breaking change
git commit -m "feat(api)!: change authentication method to OAuth2

BREAKING CHANGE: API now requires OAuth2 tokens instead of API keys"
```

---

## Documentation Guidelines

### When to Update Documentation

- Adding new features
- Changing configuration
- Modifying security settings
- Adding/removing services
- Updating dependencies
- Fixing bugs that affect usage

### Documentation Structure

```
docs/
├── INDEX.md              # Documentation hub
├── SECURITY.md           # Security architecture & procedures
├── NETWORKING.md         # Network topology & connections
└── [feature].md          # Feature-specific docs
```

### Writing Style

- **Clear and Concise**: Use simple language
- **Examples**: Provide code examples for complex concepts
- **Visual**: Use diagrams where appropriate (ASCII or Mermaid)
- **Up-to-Date**: Keep documentation synchronized with code

### Documentation Template

```markdown
# Feature Name

## Overview
Brief description of the feature.

## Prerequisites
- Requirement 1
- Requirement 2

## Configuration
```yaml
# Example configuration
```

## Usage
```bash
# Example commands
```

## Troubleshooting
Common issues and solutions.

## References
- [Related Doc](link)
```

---

## Testing Requirements

### Local Testing

Before submitting a PR:

```bash
# 1. Start local environment
./start-local.sh

# 2. Verify all services healthy
docker compose -f docker-compose.local.yml ps

# 3. Check logs for errors
docker compose -f docker-compose.local.yml logs

# 4. Test your changes
curl http://localhost:8080/health
# ... specific tests for your changes ...

# 5. Check for regressions
# Test existing functionality still works
```

### Gateway Testing

```bash
cd coresystem-gateway

# Run all tests
make test

# Run with coverage
go test -v -cover ./...

# Test specific package
go test -v ./internal/handlers

# Integration tests
make test-integration
```

### Configuration Testing

```bash
# Validate docker-compose files
docker compose -f docker-compose.yml config
docker compose -f docker-compose.local.yml config
docker compose -f docker-compose.flexible.yml config

# Validate Traefik config
docker run --rm -v $(pwd)/cloudflare-certs:/certs \
  traefik:v3.6 --configFile=/certs/dynamic.yml
```

---

## Security Guidelines

### Critical Rules

1. **NEVER commit secrets**
   - No passwords, API keys, tokens, certificates
   - Use `.env.example` for templates
   - Keep secrets in `/secrets/` directory (gitignored)

2. **NEVER commit private keys**
   - Cloudflare origin certificates
   - TLS certificates
   - SSH keys

3. **Review changes carefully**
   ```bash
   # Before committing, check what you're adding
   git diff
   git status
   
   # Use .gitignore to prevent accidents
   ```

### Secret Management

```bash
# Development secrets (simple)
echo "postgres" > secrets/postgres_password.txt

# Production secrets (strong random)
openssl rand -base64 32 > secrets/postgres_password.txt

# Verify permissions
chmod 644 secrets/*.txt
```

### Security Checklist

Before submitting security-related changes:

- [ ] No secrets in code or configuration
- [ ] Permissions are correct (644 for secrets, 600 for keys)
- [ ] Rate limiting configured appropriately
- [ ] IP whitelisting reviewed
- [ ] Input validation added
- [ ] Error messages don't leak sensitive info
- [ ] Audit logging captures relevant events
- [ ] TLS/SSL properly configured

---

## Review Process

### Pull Request Requirements

1. **Descriptive Title**: Use conventional commit format
2. **Clear Description**: 
   - What: What changes were made
   - Why: Why were these changes necessary
   - How: How were they implemented
3. **Testing**: Describe how you tested the changes
4. **Screenshots**: If UI changes, include screenshots
5. **Documentation**: Update relevant documentation
6. **Checklist**: Complete the PR checklist

### PR Template

```markdown
## Description
Brief description of changes.

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update
- [ ] Security improvement

## Testing
Describe testing performed.

## Checklist
- [ ] Code follows project conventions
- [ ] Self-reviewed code
- [ ] Documentation updated
- [ ] No secrets committed
- [ ] Tests pass locally
- [ ] No breaking changes (or documented)

## Related Issues
Closes #123
```

### Review Criteria

Reviewers will check:

1. **Code Quality**
   - Follows Go best practices (for Gateway)
   - Clear and maintainable
   - Proper error handling
   - Appropriate comments

2. **Security**
   - No secrets exposed
   - Input validation
   - Authentication/authorization
   - Rate limiting where needed

3. **Documentation**
   - Code changes documented
   - README/docs updated
   - Examples provided

4. **Testing**
   - Adequate test coverage
   - Tests pass
   - Integration verified

5. **Infrastructure**
   - Docker compose valid
   - Networking correct
   - Services health checked

---

## Development Tips

### Useful Commands

```bash
# View logs for specific service
docker compose -f docker-compose.local.yml logs -f postgres

# Rebuild specific service
docker compose -f docker-compose.local.yml up -d --build gateway

# Execute command in container
docker compose -f docker-compose.local.yml exec postgres psql -U postgres

# Check resource usage
docker stats

# Clean up
docker compose -f docker-compose.local.yml down -v
```

### Debugging

```bash
# Gateway debugging
cd coresystem-gateway
go run cmd/gateway/main.go

# View container details
docker inspect coresystem-gateway

# Network debugging
docker network inspect coresystem-local

# Check Traefik routing
curl http://localhost:8080/api/http/routers
```

### Performance Profiling

```bash
# Gateway profiling
go test -bench=. -benchmem ./...
go test -cpuprofile cpu.prof -memprofile mem.prof

# Container stats
docker stats --no-stream
```

---

## Getting Help

### Resources

- **Documentation**: Start with `docs/INDEX.md`
- **Security**: Review `docs/SECURITY.md`
- **Networking**: Check `docs/NETWORKING.md`
- **Changelog**: See `CHANGELOG.md` for recent changes

### Support Channels

- **Technical Questions**: `dev-team@coresystem.com`
- **Security Issues**: `security@coresystem.com` (private)
- **Infrastructure**: `devops@coresystem.com`

### Common Issues

See troubleshooting sections in:
- `docs/NETWORKING.md#troubleshooting`
- `README.md#troubleshooting`

---

## License

This project is proprietary and confidential. All rights reserved.

---

**Thank you for contributing to Aquatiq!** 🎉
