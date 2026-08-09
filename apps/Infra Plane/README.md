# CoreSystem Infra Plane

> Local-only edge traffic and operator infrastructure for CoreSystem.

CoreSystem Infra Plane is deliberately a **small operational plane**, not a
shared application-data platform. It owns local edge routing, the NGINX
landing surface, and a bounded operator gateway. It does not own product data,
identity, tenant scope, provider credentials, or durable infrastructure for
the other CoreSystem planes.

## Ownership

| Core Infra owns | Core Infra does not own |
| --- | --- |
| Local Traefik ingress and traffic middleware | PostgreSQL, Redis, NATS, Qdrant, MinIO, or n8n for other planes |
| `coresystem-edge` network for routeable HTTP services | Direct database access to any plane |
| NGINX static local landing page | Auth, organization, user, billing, or audit authority |
| `core-infra-gateway` self-status and local operator status | Cross-plane API aggregation, BFF behavior, or service discovery |
| Local route configuration and traffic hardening | Provider tokens, browser credentials, or secrets belonging to another plane |

Each plane brings and owns its own datastore and private networks. Cross-plane
communication remains explicit HTTP/gRPC or authenticated NATS/JetStream
contracts. The browser-facing Verevon application continues to use its own
same-origin Rust gateway; Core Infra does not replace it.

## Local topology

```text
localhost:8090
      |
      v
Traefik (routing, rate limiting, security headers)
      |                                |
      v                                v
NGINX local landing page       Core Infra gateway (/infra/*)
                                      |
                                      v
                         self-status only; no database,
                         Docker socket, Redis, or gRPC access
```

All services share only the `coresystem-edge` network. No database, broker,
object store, or workflow engine is started by this plane.

## Public domains — verevon.no / verevon.com

`configs/traefik/dynamic.yml` declares explicit `Host()` routers for the
product domains. **verevon.no is canonical**; `verevon.com`, `www.verevon.com`
and `www.verevon.no` all issue a 301 to it, so there is one canonical origin
for links, cookies, and SEO.

| Host | Behaviour |
| --- | --- |
| `verevon.no` | `/infra/*` → operator gateway, everything else → NGINX landing |
| `www.verevon.no` | 301 → `verevon.no` |
| `verevon.com`, `www.verevon.com` | 301 → `verevon.no` |
| `127.0.0.1:8090` | unchanged path-based fallback, for local development |

HSTS is applied to the domain routers only — never to the localhost fallback,
since pinning HSTS to `127.0.0.1` would force every local service onto HTTPS
and break development.

**The base compose never serves publicly.** It binds `127.0.0.1` and terminates
no TLS. Public exposure lives entirely in `docker-compose.production.yml`,
which binds `:80`/`:443`, forces HTTP→HTTPS, and mounts `./cloudflare-certs`
at `/certs`.

Two prerequisites are **not** satisfiable from inside this repo:

1. **A certificate.** `cloudflare-certs/` is empty and nothing is committed.
   Generate a Cloudflare Origin certificate covering `verevon.no`,
   `*.verevon.no`, `verevon.com`, `*.verevon.com`, save it as
   `origin-cert.pem` / `origin-key.pem` (mode 0600), and uncomment the
   `certificates:` block in `configs/traefik/dynamic.yml`. The ACME/Let's
   Encrypt alternative is documented in `docker-compose.production.yml`.
2. **DNS.** Both apexes need A records pointing at the host that runs this.

Until both exist, the domain routers are inert and only the localhost fallback
is reachable — which is the intended safe default.

Note the ownership boundary above still holds: these routers serve *this
plane's* landing page and operator gateway on those hostnames. The
browser-facing Verevon application keeps its own same-origin Rust gateway in
the Frontend Plane. Publishing the product app through this edge would be a
deliberate ownership change — see the commented `verevon-frontend` service in
`dynamic.yml`.

## Start locally

Prerequisites: Docker Engine and Docker Compose v2+.

```bash
cd "apps/Infra Plane"
cp .env.example .env.local   # only if .env.local does not already exist
./start-local.sh
```

The default local endpoint is `http://127.0.0.1:8090`.

```bash
curl --fail http://127.0.0.1:8090/infra/health
curl --fail http://127.0.0.1:8090/
```

`/infra/health` and `/infra/ready` are public local health endpoints. To
enable the operator-only status endpoint, set a non-empty randomly generated
`CORE_INFRA_OPERATOR_TOKEN` in `.env.local`, then call:

```bash
curl --fail \
  -H "Authorization: Bearer $CORE_INFRA_OPERATOR_TOKEN" \
  http://127.0.0.1:8090/infra/v1/operator/status
```

The operator endpoint intentionally reports only the Infra Plane's own
bounded status. It cannot inspect another plane's database, Redis, containers,
or tenant data.

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `CORE_INFRA_HTTP_PORT` | `8090` | Loopback port for Traefik's local HTTP entrypoint |
| `CORE_INFRA_OPERATOR_TOKEN` | empty / endpoint disabled | Local-only token for `/infra/v1/operator/status` |

Keep `.env.local` out of source control. Generate a token with:

```bash
openssl rand -hex 32
```

The local Compose file also uses pinned images, loopback-only host binding,
read-only filesystems, dropped Linux capabilities, `no-new-privileges`, and
separate application-level security headers.

## Traffic integration

Core Infra can expose a plane's **public HTTP service** locally, but it must
not attach that plane's datastore to `coresystem-edge`.

1. Connect only the routeable HTTP service to the external `coresystem-edge`
   network.
2. Add a deliberate route and upstream in
   [`configs/traefik/dynamic.yml`](configs/traefik/dynamic.yml).
3. Keep authorization and tenant scoping inside the owning plane's gateway or
   service.
4. Verify the route through Traefik before treating it as available.

For Verevon, route application traffic to the existing Frontend Plane Rust
gateway. Do not route a browser directly to an upstream Control, Data,
Ingestion, Model, or Application service.

## Runtime files

| File | Purpose |
| --- | --- |
| `docker-compose.yml` | Authoritative local Core Infra stack |
| `docker-compose.local.yml` | Compatibility entry point that includes the authoritative stack |
| `configs/traefik/dynamic.yml` | Explicit local routing and middleware |
| `configs/nginx/default.conf` | Hardened NGINX landing-page server |
| `core-infra-gateway/` | Dependency-free self-status/operator gateway |
| `start-local.sh` | Safe local build, startup, and health verification |
| `reset-local.sh` | Stops only Core Infra containers; it does not delete volumes or other planes' state |

Older shared-data and deployment artifacts remain outside the active local
runtime and are not part of the CoreSystem Infra contract. Do not use them to
provision databases or join another plane to a shared datastore.

See [the legacy artifact boundary](LEGACY_MIGRATION.md) before inspecting or
removing those historical files.
