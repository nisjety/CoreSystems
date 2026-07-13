# browser-broker Research Dive

Generated: 2026-07-11 (Phase 4 Model Plane audit; supersedes 2026-06-09 pass)

Scope: `apps/Model Plane/go/services/browser-broker`

## 2026-07-13 secure-MVP correction

Current source requires exact `aud=browser-broker`, derives tenant/owner from
verified identity, applies `browser:read`/`browser:write` to service principals,
fails cross-tenant/user lookups as not-found, and exposes only standard gRPC
health publicly. Focused auth/grant/server coverage is 87.8% and the complete Go
suite passes. This is source-only: Auth Core/Quarry/model-gateway caller issuance
is not complete, so the hardened service must not be deployed independently.
It remains a grant-bookkeeping service; this change does not turn it into the
Quarry browser executor.

Evidence grades used below: **[live-curl]** = verified against the running
container from the host; **[source-only]** = read from disk, not exercised at
runtime; **[inspect]** = from `docker ps` / compose / git (config + state, not
`docker exec`). Docker's containerd content store is corrupted this pass, so
`docker exec`/`build`/`logs` were not used.

## Snapshot

`browser-broker` is a small, honest Go gRPC service that owns a **browser-grant
lifecycle**: `AcquireGrant`, `RevokeGrant`, `ValidateGrant`, `Health`. The
handlers are real and non-mocked, the build is clean, and the server-package
tests pass on the host toolchain. The service is live: `:8087/healthz` and
`:8087/readyz` return `ok`, and gRPC `:9095` accepts connections.

Three things the 2026-06-09 doc got wrong or omitted, now corrected:

1. **It is not wired to Quarry.** There is zero Quarry reference in the service.
   Grants are opaque tokens with a placeholder scope (`browser://cloud` /
   `browser://local`) and a placeholder endpoint (`https://browser-broker.local/...`).
   The "grants Quarry browser sessions to agents" description is aspirational,
   not implemented.
2. **No caller invokes a grant RPC.** `model-gateway` (Rust) and
   `orchestrator-core` (Go) each hold a client channel to `:9095` but never call
   `AcquireGrant`/`Validate`/`Revoke`. The lifecycle is served but dead-ended.
3. **The rate limiter and header scrubber are defined but not wired.** `main.go`
   constructs a bare `grpc.NewServer()` with no interceptor chain, so at runtime
   the service neither rate-limits nor scrubs internal-prefixed metadata.

Non-generated Go files in the tree: 13 (`cmd` + `internal/{grant,server,telemetry}`).

## Runtime Shape

- `cmd/main.go` — health HTTP on `:8087` (`/healthz`, `/readyz`, both plain
  `ok`), gRPC on `:9095`, in-memory grant store. **Bare `grpc.NewServer()` — no
  interceptors registered.** [source-only]
- `internal/grant/grant.go` — thread-safe in-memory `Store`; `Create` (128-bit
  `crypto/rand` hex ID, caller-supplied TTL), `Get` (enforces revoked + expired),
  `Revoke`. No persistence, no expired-entry sweep. [source-only]
- `internal/server/server.go` — gRPC handlers; `defaultGrantTTL = 15m`; base URL
  from `BROWSER_BROKER_BASE_URL` (defaults to `https://browser-broker.local`);
  `scopeURLForMode` maps `""|cloud → browser://cloud`, `local → browser://local`,
  else `InvalidArgument`. [source-only]
- `internal/server/errors.go` — single `mapErr` translating internal errors to
  gRPC codes (`NotFound`, `FailedPrecondition` for revoked/expired,
  `PermissionDenied` for `ErrPolicyDenied`, else `Internal`). `ErrPolicyDenied`
  is defined but never returned by any handler. [source-only]
- `internal/server/ratelimit.go` — `UnaryRateLimitInterceptor` (per-peer
  fixed-window). **Referenced only in tests.** [source-only]
- `internal/server/headers.go` — `ScrubInternal` / `SendSafeHeader` /
  `UnaryHeaderScrubInterceptor` for stripping `x-internal-*` / `x-triode-internal-*`
  from **outbound response** metadata. **The interceptor is referenced only in
  its own definition (not even in a test); `ScrubInternal`/`SendSafeHeader` are
  exercised only by unit tests.** [source-only]
- `internal/telemetry/metrics.go` — 5 OTEL counters (requests, grants
  issued/revoked/validated, gateway rate-limited). Instruments are created and
  incremented by handlers; no OTEL exporter is wired in `main.go`, so counters
  are recorded to the no-op default meter. [source-only]

Primary surfaces: HTTP health `:8087`; gRPC `:9095`. Compose publishes both to
the host in dev/base; the production overlay does `ports: !reset []`
(internal-only). [inspect]

## Live Verification (this pass)

- `curl :8087/healthz` → `ok`, HTTP 200. **[live-curl]**
- `curl :8087/readyz` → `ok`, HTTP 200. **[live-curl]**
- `curl :8087/` → `404 page not found` (health-only mux; no stray routes). **[live-curl]**
- `nc :9095` → connection succeeded (gRPC listener up; no reflection registered,
  so no method-level probe from host). **[live-curl]**
- `docker ps`: `model-plane-browser-broker-1  Up 2 days (unhealthy)  8087->8087, 9095->9095`.
  "unhealthy" is the compose `curl` healthcheck evaluated via exec, which fails
  under the corrupted content store — the service itself is serving. **[inspect]**
- Host toolchain (go 1.26.2): `go build ./...` exit 0, `go vet ./...` clean,
  `go test ./...` → `internal/server` **ok** (grant/telemetry/cmd have no tests). **[source-only]**

## API And Relationship Map

- Proto contract: `model_plane.v1.BrowserBroker` (`AcquireGrant`, `RevokeGrant`,
  `ValidateGrant`, `Health`) — generated Go (`go/gen/...`), Rust
  (`mp-contracts`), and TS (`velion` legacy) stubs all exist. [source-only]
- `orchestrator-core` (Go): `BROWSER_BROKER_ADDR` (default `browser-broker:9095`);
  `grpcclient` dials and closes the conn but **calls no grant method**. [source-only]
- `model-gateway` (Rust): `state.rs` builds `browser_client: BrowserBrokerClient<Channel>`
  (env `BROWSER_BROKER_URL`/`BROWSER_BROKER_ADDR`, default `http://localhost:9095`)
  and stores it in `AppState`, but **never calls acquire/validate/revoke**. [source-only]
- `capability-core` roadmap catalog lists `browser-broker` as a capability entry
  (registry metadata only). [source-only]
- **No Quarry-v2 wiring in either direction.** browser-broker does not call
  Quarry, and Quarry does not validate grants against browser-broker. [source-only]

## Stubs, Placeholders, And Missing Connections

- No `TODO`/`FIXME`/`mock`/`fake`/`placeholder`/`not-implemented` strings in the
  Go source. The only match is the generated `UnimplementedBrowserBrokerServer`
  embed — a standard forward-compatibility guard, **not a stub**. [source-only]
- Placeholder *values* (not code stubs): scope URLs `browser://{cloud,local}`
  and the default base URL `https://browser-broker.local`. With
  `BROWSER_BROKER_BASE_URL` unset in every compose overlay, issued endpoints are
  non-routable placeholders. [source-only]

## Findings (Phase 4)

- **HIGH — Rate limiter and header scrubber are unwired.** `main.go` uses
  `grpc.NewServer()` with no `ChainUnaryInterceptor`. `UnaryRateLimitInterceptor`
  appears only in `ratelimit_test.go`; `UnaryHeaderScrubInterceptor` appears only
  in its own definition. At runtime the service does **not** rate-limit and does
  **not** scrub metadata. The audit ask "strips inbound internal metadata" is not
  met on two counts: the scrub logic targets *outbound* response metadata, and it
  is not installed regardless. [source-only]
- **HIGH — Grant flow is served but not exercised end-to-end.** No service calls
  `AcquireGrant`/`ValidateGrant`/`RevokeGrant`; callers hold idle channels. The
  lifecycle is real code with no live consumer. [source-only]
- **HIGH — No Quarry integration.** The service mints opaque grants with a
  placeholder scope and endpoint; there is no create/proxy/validate handshake
  with Quarry-v2. The "grants Quarry browser sessions" charter is unimplemented. [source-only]
- **MEDIUM — In-memory only; no durability; unbounded growth.** Grants are lost
  on restart. Expiry/revocation are lazy (checked on `Get`); revoked/expired
  entries are never removed, so the map grows for the process lifetime. [source-only]
- **LOW — No authz on grant minting.** Any gRPC caller can mint a grant for any
  `org_id`/`session_key`; no identity/capability/ZDR check. Harmless while unused,
  a real gap once wired. [source-only]
- **LOW (cosmetic) — Field mislabel.** `Store.Create(orgID, agentID, ...)` is
  called with the request's `session_key` as `agentID`, so `Grant.AgentID`
  actually holds a session key. [source-only]

## MCP / Visma Question

Out of scope for this service. `browser-broker` has no MCP wiring and no Visma
reference. The "test the Visma MCP" ask belongs to the bridge layer
(`bridges/mcp-bridge` / `bridge-core`), not the browser broker.

## Uncommitted WIP

`git status --porcelain` and `git diff --stat` for the service dir are both
empty — **no uncommitted changes**. Last touch was the fleet-wide
`perf(build): BuildKit cache mounts + healthcheck gating` commit. [inspect]

## Doc Cleanup Read

- This file (was 2026-06-09) overstated the service as "real but not durable"
  and implied a working browser-grant path. Corrected here: real handlers, but
  no Quarry binding, no live caller, and unwired safety interceptors.
- `apps/Model Plane/docs/STUBS.md` (flagged in `apps/STALE_DOC_DELETION_REGISTER.md`,
  Model / review) cites "live browser-broker handlers" — accurate at the handler
  level, but it should be qualified: the handlers are live, the *grant path* is
  not consumed and not connected to Quarry.

## Bottom Line

`browser-broker` is a clean, tested, live Go gRPC micro-service that correctly
implements a browser-grant lifecycle — but it is an **island**. Nothing calls it,
it does not talk to Quarry, and its rate-limit and metadata-scrub interceptors are
defined yet never attached to the server. It is honest scaffolding, not a working
browser-grant control path.
