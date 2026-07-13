# bridge-core Research Dive

Generated: 2026-07-11 (supersedes 2026-06-09)

Scope: `apps/Model Plane/go/services/bridge-core` (Go, HTTP `:8091` / gRPC `:9100`),
plus the sibling `apps/Model Plane/bridges/mcp-bridge` (Node) and
`apps/Model Plane/bridges/lsp-bridge` (Python).

## 2026-07-13 secure-MVP correction

Current source requires exact `aud=bridge-core` authentication on every session
HTTP route and business gRPC call; derives organization and owner from verified
identity; applies exact `bridge:read`/`bridge:write` service scopes; and leaves
only HTTP readiness/liveness and standard gRPC health public. Caller-supplied
body/query identity cannot override claims. The unauthenticated WebSocket
skeleton is explicitly quarantined, and configured webhook URLs are no longer
written to logs or delivery errors. Focused auth/server/session coverage is
90.7% and the complete Go suite passes. Deployment is blocked because Auth Core
issuance and bridge-cli/caller bearer forwarding are not yet one tested
contract; the running service retains the historical behavior below.

Evidence grades used throughout: `[live-curl]` (verified against the running
container from the host), `[source-only]` (read from disk), `[inspect]`
(`docker ps` / compose / env from disk). Docker exec/build/logs are unusable this
pass (corrupted containerd content store), so every container shows `(unhealthy)`
because the exec-based healthcheck fails — not because the service is down.

## Bottom line

`bridge-core` is a **standalone, in-memory session-bookkeeping + channel-ingress
shell**. Its HTTP session lifecycle is real and live. But in the deployed
configuration every channel runs the **noop (echo) adapter**, the gRPC port
registers **zero services**, the voice pipeline and WebSocket adapter are
**unwired skeletons**, the HTTP API is **unauthenticated**, and nothing in the
system calls it. It is *not* in the Velion chat path and is *not* why chat tools
failed.

**The MCP question is answered here and the answer is: no.** MCP bridging is a
*reference implementation* (`bridges/mcp-bridge`) that is **not deployed** — no
container exists, host ports are unreachable. The model-gateway's MCP proxy code
is genuinely implemented, but there is **no Visma MCP wiring anywhere**, and the
one live `visma mcp` registry record is misconfigured (transport `stdio` with an
HTTPS URL) so discovery fails before any call. "Test the Visma MCP" is therefore
not currently a reachable Model Plane capability.

## Live health & functional probe `[live-curl]`

| Check | Result |
|---|---|
| `GET :8091/healthz` | `200 ok` |
| `GET :8091/readyz` | `200 ok` |
| `POST :8091/api/v1/sessions` (no auth header) | `201` — session created |
| `GET :8091/api/v1/sessions?org_id=<any>` (no auth) | `200` — returns that org's sessions |
| `POST :8091/api/v1/sessions/{id}/ingest` | `200` — `{"result":"aGVsbG8=","delivered":true}` (payload echoed verbatim by the noop adapter) |
| `DELETE :8091/api/v1/sessions/{id}` | `200 {"closed":true}` |
| `GET :9301/health` (mapped mcp-bridge) | connection refused — bridge not running |
| `GET :9302/health` (mapped lsp-bridge) | connection refused — bridge not running |
| `GET :9201` / `:9202` | answered by Data Plane `index-engine-rs` / `embedding-engine-rs` (documented host-port collision) |

`docker ps` `[inspect]`: `model-plane-bridge-core-1` Up ~2 days, `0.0.0.0:8091->8091`,
`0.0.0.0:9100->9100`, `(unhealthy)` (healthcheck exec failure only). No
`mp-mcp-bridge` / `mp-lsp-bridge` container exists (`docker ps -a`).

## Runtime shape `[source-only]`

Entrypoint `cmd/main.go`:

- Builds an in-memory `session.Registry` and a `channel.AdapterRegistry`.
- Starts a `delivery.Worker` draining a `delivery.MemoryStore` with a real
  `HTTPSender` (retry/backoff) — the durable webhook outbox.
- **Conditionally** registers a real `WebhookAdapter` per channel *only if*
  `BRIDGE_WEBHOOK_URL` is set; otherwise logs "all channels using noop adapter".
- Serves the HTTP API on `:8091`.
- Creates a bare `grpc.NewServer()` on `:9100` and `Serve()`s it **with no
  service registered** (`server.GRPCPlaceholder` documents this as intentional
  "future iteration").

HTTP surface (`internal/server/server.go`): `GET /healthz`, `GET /readyz`,
`POST /api/v1/sessions`, `GET /api/v1/sessions`, `GET /api/v1/sessions/{id}`,
`POST /api/v1/sessions/{id}/ingest`, `DELETE /api/v1/sessions/{id}`. No auth
middleware on any route.

## THE MCP QUESTION (`bridges/`) `[source-only]` + `[inspect]` + `[live-curl]`

`bridges/README.md` states plainly: these are "Reference implementations for the
optional external bridges the model-gateway proxies to … Without either bridge
configured, the corresponding gateway RPCs return `Unimplemented` cleanly."

**mcp-bridge (`bridges/mcp-bridge/server.js`, ~200 LOC)** — a real, functional
translator. It reads a `bridge.json` (env `MCP_BRIDGE_CONFIG`), exposes
`POST /tools/call?server_id=<id>` with `{name, arguments}`, and dispatches to the
upstream over **stdio** (spawns the configured command, MCP JSON-RPC over stdin/
stdout) or **http** (forwards verbatim). It has an allowlist filter and a health
endpoint. It is not a stub — but it is **not deployed**: it appears only in
`deploy/docker-compose.bridges.yml`, an explicitly opt-in overlay
(`MODEL_PLANE_BRIDGES=1 ./scripts/compose.sh up -d … mcp-bridge lsp-bridge`), and
is absent from `docker-compose.yml`, `.override.yml`, and `.production.yml`.
Host ports 9301/9302 (its published mappings) are refused `[live-curl]`.

**Gateway side is real `[source-only]`.** `rust/services/model-gateway/src/runtime_registries.rs`
implements `McpRegistry` (per-`(org, server_id)`), `handle_register_mcp_server`,
`mcp_tool_defs` (discovery + `tools/list` caching + allowlist + `mcp__<server>__<tool>`
namespacing), `stdio_tool_call`, and `http_list_tools`; `handle_proxy_mcp_tool` is
wired into `grpc.rs::proxy_mcp_tool` and dispatched from `tool_loop.rs`
(`mcp if mcp.starts_with("mcp__")`). `mcp_capability_payload` is unit-tested to
never leak the auth token. So the plumbing from the agent loop → gateway → bridge
→ external MCP server genuinely exists in code.

**No Visma wiring exists `[source-only]`.** A repo-wide grep for `visma` in the
Model Plane returns only two hits, both in prior audit docs. `bridge.example.json`
ships only `fs` and `git` filesystem/git MCP servers. There is no Visma MCP
config, no Visma bridge, no Visma token, nothing.

**Why "test the Visma MCP" fails, precisely:**
1. The mcp-bridge container is not deployed (no HTTP endpoint for the gateway to
   proxy to over an `http` transport).
2. Per prior live audits, the only MCP registry record is one org-scoped
   `visma mcp` entry configured as `transport: stdio` with an **HTTPS URL** and no
   allowlist. For `stdio`, `runtime_registries.rs::stdio_tool_call` calls
   `parse_stdio_command(url)` and spawns the result as a subprocess — an
   `https://…` string is not a runnable command, so discovery/dispatch fails.
   Transport and URL disagree; no MCP tool is ever executed.
3. Normal chat exposes no tools anyway (documented in the plane audit).

Net: MCP bridging is scaffolded end-to-end in code but off in the deployed stack,
and the single Visma record is malformed. This is the direct reason chat cannot
reach any MCP server, Visma included.

**lsp-bridge (`bridges/lsp-bridge/server.py`, ~200 LOC)** — same posture: a real
reference LSP wrapper (diagnostics/hover/definition/completion over LSP JSON-RPC,
TS/Python/Go/Rust), speaking the gateway's `POST /lsp/query`. Gateway `LspQuery`
returns `Unimplemented` unless `LSP_BRIDGE_URL` is set, which only the bridges
overlay sets. Not deployed.

## Stubs / placeholders / unwired surfaces `[source-only]`

| Item | State | Note |
|---|---|---|
| `NoopAdapter` (cli/vscode/web/api default) | Deployed default | Pass-through echo; `Deliver` logs and drops. Active because `BRIDGE_WEBHOOK_URL` is unset. |
| `WebhookAdapter` + `delivery.Worker`/`HTTPSender` | Real, unconfigured | Durable outbox + retried POST. Correct code, but never wired in the deployed env. |
| `WebSocketAdapter` | Skeleton | "actual WebSocket upgrade … future iteration"; `Deliver` encodes a frame then discards it. Not registered anywhere. |
| `voice.VoicePipeline` (STT→LLM→TTS) | Unwired skeleton | Only `Noop*` providers exist; `NewVoicePipeline` is never called; no HTTP/gRPC endpoint reaches it. |
| gRPC server `:9100` | Placeholder | `grpc.NewServer()` serves with zero registered services; `GRPCPlaceholder` documents intent. |
| `session.Registry` | Real but ephemeral | In-memory map; no persistence, no NATS publish; lost on restart. Distinct from durable session-core. |
| `SESSION_CORE_ADDR` / `MODEL_GATEWAY_ADDR` config | Dead | Loaded by `config.Load()` but `main.go` never dials them; bridge-core forwards to nothing. |

No `TODO`/`FIXME` markers; everything is honestly labeled "noop"/"skeleton".

## Security findings `[source-only]` + `[live-curl]`

- **HIGH — unauthenticated HTTP API with attacker-controlled tenancy.** No route
  has auth. `JWTValidator` (`internal/channel/jwt.go`, HMAC-SHA256) exists and is
  correct, but is referenced *only* by the unreachable `WebSocketAdapter`. The
  compose-supplied `JWT_SECRET` / `BRIDGE_JWT_SECRET` / `JWT_AUDIENCE` are **never
  read by `config.Load()` or the server** — dead env. `GET /api/v1/sessions`
  authorizes solely on the caller-supplied `org_id` query param, so any client can
  create sessions for any org/user and enumerate any org's sessions. Verified live
  with no credentials `[live-curl]`. Mitigating factor: sessions carry no
  sensitive payload and nothing consumes the service, but this must be closed
  before bridge-core is placed on any real ingress path.
- **MEDIUM — dead gRPC listener.** `:9100` accepts connections but serves no
  methods; readiness/mesh probes will connect but every RPC fails.
- **LOW — noop echo can masquerade as success.** `ingest` returns
  `"delivered":true` from the noop `Deliver`, which could read as real delivery in
  logs/telemetry.

## Uncommitted WIP `[inspect]`

Working tree is **clean** for both `go/services/bridge-core` and `bridges/`
(`git status --porcelain` empty, `git diff --stat` empty). Last commit touching
them is `fb161cc7` (BuildKit cache mounts + healthcheck gating). No WIP to
reconcile.

## Build `[source-only]`

Host Go toolchain `go1.26.2` (module targets `go 1.25.1`). `go build ./...` and
`go vet ./...` both exit 0 with no output. Bridge Dockerfiles are standard
(`node:22-alpine`; `python:3.12-alpine` with pylsp + typescript-language-server;
Go/Rust LSP intentionally not bundled).

## Doc-cleanup read

- The 2026-06-09 dive was directionally right ("real HTTP, gRPC registered but
  unwired, default adapters noop/skeleton") and this pass confirms all three,
  adds the auth gap, the dead upstream config, and the deployed-noop reality.
- `docs/STUBS.md`, `docs/gap-model.md` should **retain** bridge-core's noop/
  skeleton entries (they are accurate, not stale) but note the webhook path is
  real-when-configured. Update, don't delete.
- Recommend recording in the plane audit that `bridge-core` is out of the chat/
  tool path entirely, and that MCP (incl. Visma) is gated by the un-deployed
  `bridges/mcp-bridge` overlay + a malformed `visma mcp` registry record.
