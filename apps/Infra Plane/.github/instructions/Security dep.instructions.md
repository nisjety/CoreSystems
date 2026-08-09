---
applyTo: '**'
---
Provide project context and coding guidelines that AI should follow when generating code, answering questions, or reviewing changes.
# Aquatiq Deployment and Security Instructions
🔵 1. HTTP Client, Retry, Circuit Breaker
✔️ go-retryablehttp (HashiCorp)

https://github.com/hashicorp/go-retryablehttp

Automatiske retries

Backoff

Timeout

Tåler “skitne” API-er som Visma.net (429/500)

Bedre enn raw net/http i integrasjoner

✔️ gobreaker (circuit breaker)

https://github.com/sony/gobreaker

Brukes i store enterprise-miljøer

Hindrer at du spammer SuperOffice/Visma når API er nede

🔵 2. Rate Limiting (må-ha for ERP/CRM integrasjoner)
✔️ golang.org/x/time/rate

Standard i Go-verden

High performance leaky bucket

Lav latency

Enkelt å implementere per token/per kunde/per endpoint

✔️ ulule/limiter (for mer avansert API-limiting)

https://github.com/ulule/limiter

Redis-distributed rate limiting

Perfekt når du har flere pods/instances

Brukes i gateway-setup der flere microservices kaller samme integrasjon

🔵 3. OAuth2 + Token Rotation
✔️ golang.org/x/oauth2

Beste offisielle OAuth2-pakken

Brukes for SuperOffice OAuth2 og Visma Connect OAuth2

Støtter automatic token refresh

Kombinér med custom token store (Redis)

✔️ square/go-jose

For JWT-signering/validering

Hvis Microservices ↔ Integration Proxy bruker signed internal tokens

🔵 4. Redis (TLS + encrypted cache)
✔️ redis/go-redis/v9

https://github.com/redis/go-redis

Standard Redis client i Go 1.22+

Støtter TLS/SSL

Perfekt for token store, caching, rate limiting, sessioning

✔️ crypto libs for encrypting data before storing:

github.com/gtank/cryptopasta → AES-256 secure defaults

golang.org/x/crypto/nacl/secretbox → libsodium-style security

Hashicorp Vault Transit → hvis du bruker Vault for encryption at rest

🔵 5. Logging + Observability (må være PII-safe)
✔️ uber-go/zap

Raskeste strukturerte logger i Go

JSON-output → perfekt for API-gateway

Høy throughput + lav CPU

✔️ OpenTelemetry for Go

https://github.com/open-telemetry/opentelemetry-go

Full tracing mellom microservices

Perfekt når du har NestJS → Go → Python → ERP

Se alt i Grafana, Tempo, Jaeger eller Azure Monitor

✔️ slog (Go 1.21+)

Moderne standard logging API

Kan kobles til Zap som backend

🔵 6. Secure Config + Secrets
✔️ Azure Identity SDK for Go

https://github.com/Azure/azure-sdk-for-go/sdk/azidentity

For Azure Key Vault integration:

Managed Identity

Client Secret Credential

Workload Identity for Kubernetes

Token-akselerasjon

✔️ Azure Key Vault Go SDK

https://github.com/Azure/azure-sdk-for-go/sdk/keyvault/azsecrets

Brukes til:

Hente SuperOffice OAuth client secret

Hente Visma.net API client/key

Rotere secrets automatisk

🔵 7. Internal Auth Between Microservices

Hvis NestJS og Python skal snakke med Go-gateway:

✔️ go-jose v3

Signer interne JWT tokens (ES256/RS256)

Valider på Go-gateway

Zero-trust internt i clusteret

✔️ github.com/golang-jwt/jwt/v5

Standard lib for JWT i Go

Lett og stabil

🔵 8. API Router / Framework

Hvis du vil ha noe minimalistisk og enterprise:

✔️ Chi

https://github.com/go-chi/chi

Rask

Middleware-basert

Perfekt til API-gateways

Brukes av store enterprise-løsninger

✔️ Fiber (Express.js-lignende)

https://github.com/gofiber/fiber

Veldig rask

Hvis du liker NestJS/Express patterns

Lett å bruke for JSON API

Men for Secure Integration Layer anbefaler jeg:

✔️ Chi – mest stabilt og enterprise-friendly.

🔵 9. Validation + Sanitization
✔️ go-playground/validator

https://github.com/go-playground/validator

Valider request bodies

Sikrer at API-gateway ikke sender feil til SuperOffice/Visma

✔️ bluemonday

HTML sanitization

Hvis brukere legger inn data som skal inn i ERP

🔵 10. YAML/JSON Config Handling
✔️ viper

Laster config fra env, yaml, json, secrets

Perfekt for gateway

🧊 11. Enterprise Patterns: Resilience + Policy
✔️ github.com/go-kit/kit

Må-ha toolkit for enterprise Go

Rate limiting

Circuit breaking

Retries

Logging

Tracing

Metrics

Go-kit gir deg “Netflix-style” microservice survival-patterns.

🧩 Anbefalt "best possible" dependency stack

Hvis jeg skulle bygget en enterprise-integration-gateway for Aquatiq, ville jeg brukt:

HTTP + resiliency

retryablehttp

gobreaker

go-kit

API

chi

validator

Security

go-jose

jwt-go

azidentity

azsecrets

cryptopasta

Cache / Rate limit

redis/go-redis

x/time/rate

Observability

zap

opentelemetry-go

prometheus metrics

Config

viper