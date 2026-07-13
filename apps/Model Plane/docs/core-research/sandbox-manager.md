# sandbox-manager Research Dive

Generated: 2026-07-11 (Phase-4 Model Plane audit re-verification; supersedes 2026-06-09 pass)

Scope: `apps/Model Plane/go/services/sandbox-manager`

## 2026-07-13 secure-MVP correction

Current source requires exact `aud=sandbox-manager`, derives tenant/owner from
verified identity, applies `sandbox:read`/`sandbox:write` to service principals,
and leaves only standard gRPC health public. Focused auth/lease/server coverage
is 94.5% and the complete Go suite passes. Auth Core and model-gateway caller
issuance remain incomplete, so deployment is gated. The service still owns only
lease/snapshot metadata; this auth work does not fabricate a real sandbox
provisioner or object-store snapshot implementation.

Evidence grades used below: **[live-curl]** = observed on the running host; **[inspect]** = `docker ps`/`docker inspect` config/state (no exec); **[source-only]** = read from disk / host toolchain. Docker exec/build/logs were unavailable this pass (containerd content store corruption) and were not used.

## Bottom Line

`sandbox-manager` is a **real, live, well-tested lease + snapshot BOOKKEEPING service** — but it is **not a code-execution sandbox**. It provisions nothing: no container, VM, or process is ever created; `AcquireLease` hands back a fabricated `sandbox://<id>` endpoint that points at no runtime, and `SnapshotSandbox` returns a fabricated MinIO object key without writing a byte to object storage. It is also **dormant**: model-gateway constructs a `sandbox_client` but never calls it, and no other Model Plane service (Go or Rust) leases a sandbox. So relative to its stated charter ("code sandboxes for agents") it is a **thin metadata seam / provisioning stub over no backend** — deliberately so, per the design ruling recorded in `docs/STUBS.md`. Actual agent code execution in Model Plane happens elsewhere, in execution-core's in-process bubblewrap path, which is separate and real.

Net change vs the 2026-06-09 doc: that pass correctly said "live" + "in-memory only" but missed four material facts documented below — (1) no real sandbox/provisioning, (2) zero callers / dormant, (3) dead Redis+MinIO env promising durability the code never implements, (4) telemetry counters that are never exported.

## Live State

- **Health [live-curl]:** `GET :8086/healthz` → `200 ok`; `GET :8086/readyz` → `200 ok`. `GET :8086/` and `:8086/metrics` → `404` (no metrics HTTP endpoint).
- **Container [inspect]:** `model-plane-sandbox-manager-1` — `State=running`, `Running=true`, `RestartCount=0`, `Up ~2 days`. Ports `8086->8086` (HTTP health) and `9094->9094` (gRPC) published. `Health=unhealthy` is a **false negative**: the compose healthcheck is `["CMD","curl","-f","http://localhost:8086/healthz"]`, an exec-based probe that fails under the fleet-wide containerd exec corruption. The HTTP endpoint itself is healthy [live-curl].
- **gRPC :9094 [source-only/inspect]:** port published; the gRPC surface itself was not probed this pass (no grpcurl), but the handler code is present and unit-tested over a real transport (bufconn round-trip test).

## Runtime Shape [source-only]

~787 LOC of Go across ~12 files. Entry `cmd/main.go` (72 LOC): slog JSON logger → HTTP health mux on `:8086` (`/healthz`, `/readyz`) → gRPC server on `:9094` registering the real `SandboxManager` implementation → SIGTERM/SIGINT graceful stop.

Packages:

- `internal/server` — `SandboxManagerServer` implementation. `AcquireLease`, `ReleaseLease`, `SnapshotSandbox`, `Health`. Full request validation (scope_id/scope_type∈{thread,agent}/org_id/ttl>0; lease_id; label). Embeds `mpv1.UnimplementedSandboxManagerServer` purely for gRPC forward-compat — **not** a stub marker.
- `internal/lease` — in-memory `map[string]*Lease` behind `sync.RWMutex`; 128-bit `crypto/rand` hex IDs; `ExpiresAt`/`IsExpired` TTL enforcement; `nowFn`/`randFn` seams for testing. Endpoint is `"sandbox://" + id`.
- `internal/snapshot` — in-memory `map[string]*Snapshot`; ObjectKey is `"snapshots/" + leaseID + "/" + id` (a computed string, **no MinIO write**).
- `internal/server/errors.go` — single-source `mapErr` (PolicyDenied→PermissionDenied, LeaseNotFound→NotFound, LeaseExpired/InvalidLease→FailedPrecondition, else→Internal) + telemetry outcome classifiers.
- `internal/telemetry` — three OTEL `Int64Counter`s (`requests_total`, `lease_decisions_total`, `snapshot_decisions_total`).

Proto: `proto/model_plane/v1/sandboxes.proto` — service comment claims ownership of "lease creation, TTL enforcement, snapshots, cleanup, quota"; `SnapshotResponse.object_key` is documented as a "MinIO object key". Only lease creation, TTL, and snapshot-bookkeeping are implemented; cleanup, quota, and real snapshot persistence are not.

## Build & Test [source-only, host toolchain works]

- `go version` = go1.26.2 darwin/arm64 (host); module pins `go 1.25.1`. Workspace `replace ../../gen` for generated protos.
- `go build ./...` → exit 0.
- `go test ./...` → `internal/server` **ok** (0.45s); `cmd`, `internal/lease`, `internal/snapshot`, `internal/telemetry` report **no test files**.
- Test coverage is real and meaningful for the handler + error layers: field validation, NotFound/InvalidArgument codes, success paths, and a `bufconn` transport round-trip. Gap: the `lease` and `snapshot` stores have no direct unit tests (exercised only indirectly through server tests) — TTL-expiry (`ErrLeaseExpired`) and concurrent-access paths are not directly asserted.

## API & Relationship Map

- **Intended consumers:** model-gateway holds `AppState.sandbox_client: SandboxManagerClient<Channel>` (`rust/services/model-gateway/src/state.rs`), built from a **lazy** tonic channel keyed on `SANDBOX_MANAGER_URL` then `SANDBOX_MANAGER_ADDR` (default `http://localhost:9094`). Lazy = no connection attempt at startup.
- **Actual traffic: NONE [source-only].** Grepping all non-generated Rust and Go: no `.acquire_lease(...)`, `.release_lease(...)`, or `.snapshot_sandbox(...)` call sites exist anywhere. execution-core, session-core, inference-core, orchestrator-core do not reference it. The client is constructed and stored but never invoked — a wired-but-unused seam. The service currently serves zero production requests.
- **Downstream:** none. Purely in-process state.

## Config / Code Drift (genuine findings) [source-only/inspect]

1. **Dead durability env — promises persistence the code does not implement.** The compose service block sets `REDIS_URL=redis://dragonfly:6379`, `MINIO_ENDPOINT=minio:9000`, `MINIO_ACCESS_KEY`, `MINIO_SECRET_KEY`, and `depends_on: dragonfly`. The Go source imports **no** Redis or MinIO client and reads **none** of these vars. All lease/snapshot state is in-memory and is lost on restart. The env is aspirational (matches the gap-model.md Tier-2 "snapshot-as-OCI → MinIO" / "Redis SET NX PX state lock" backlog), not wired.
2. **`SANDBOX_MANAGER_ADDR: http://…:9094`** uses an `http://` scheme for what is an h2c gRPC endpoint. The lazy tonic channel tolerates it and, since it is never dialed, this is latent rather than a live fault.
3. **Stale entry-point comment.** `cmd/main.go:48` still reads `// gRPC server on :9094 — sandbox manager stub returning Unimplemented.` The server is fully implemented; the comment is wrong and was already flagged in the prior pass — still present.

## Telemetry gap [source-only]

`internal/telemetry` defines three counters via the **global** `otel.Meter(...)`, but `main.go` never installs a `MeterProvider`/reader/exporter. With no provider set, the global meter is a no-op, so the counters increment into nothing and are never exported; there is also no `/metrics` HTTP endpoint. Metrics are effectively decorative today.

## Stub / Marker Triage [source-only]

Grep for `todo|fixme|mock|stub|fake|placeholder|not-implemented|unimplemented|hack|xxx` in the service returned exactly two hits, both benign-by-classification:

- `cmd/main.go:48` — stale/misleading comment (see drift #3). Cosmetic.
- `internal/server/server.go:22` — `mpv1.UnimplementedSandboxManagerServer` embed. Standard gRPC forward-compat idiom, not a stub.

No error-masking, no fake data returned to callers beyond the by-design fabricated endpoint/object-key strings, no dead code.

## Doc-Register Reconciliation

- **`docs/STUBS.md` (lines 42–43): ACCURATE — leave as the source of truth.** It already states the nuanced reality: lease/snapshot bookkeeping is real + deployed; only provisioning is unbuilt; `sandbox_client` is wired but no caller leases a sandbox; provisioning is YAGNI-deferred pending a concrete per-session consumer. The `apps/STALE_DOC_DELETION_REGISTER.md` flag ("likely stale … several services described as stubs are now live") does **not** apply to the sandbox-manager entry — for this service STUBS.md is correct and should not be softened.
- **`docs/gap-model.md` (lines 413–426): ACCURATE.** Real create-paths are tracked as the Tier-2 "largest infra gap" (bwrap OS isolation in execution-core; sandbox-manager reconcile/egress/snapshot-to-MinIO/Redis lock). No revision needed.
- **This file (core-research/sandbox-manager.md): REWRITTEN** to add the four missing facts. Not a deletion candidate.

## Relevance to the Phase-4 audit questions

- The user's "test the Visma MCP" / broken-tool complaints are **unrelated** to sandbox-manager — it is not on the chat tool-execution path (that is execution-core's runtime loop). This service is dormant and touches no chat feature today.
- "Code sandboxes: real or stub?" → **bookkeeping real; sandbox provisioning stub/absent by design; zero callers.** The honest, deliberate gap, not a regression.
