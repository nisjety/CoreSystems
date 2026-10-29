# capability-core

Go service in the Model Plane responsible for the capability registry, capability
policy evaluation, and the Model Plane implementation-status catalog.

## Ports

| Protocol | Port |
|----------|------|
| HTTP     | 8085 |
| gRPC     | 9097 |

## Endpoints

### `GET /api/v1/model-plane/implementation-status`

Returns the canonical, in-code catalog of Model Plane features and their
implementation status (`yes` | `partial` | `no`). Methods other than `GET`
return `405 Method Not Allowed` with an `Allow: GET` header.

Example response shape:

```json
{
  "serviceChecklist": [
    {
      "id": "capability-core",
      "name": "Capability Core",
      "category": "go",
      "status": "partial",
      "owner": "model-plane/go",
      "description": "Capability registry, policy, and status endpoint (this service)."
    }
  ],
  "backendRuntime": [
    {
      "id": "otel-tracing",
      "name": "OpenTelemetry Tracing",
      "status": "yes",
      "description": "Distributed traces emitted across Go services via otel/metric v1.43.0."
    }
  ],
  "productShell": [
    {
      "id": "implementation-status-api",
      "name": "Implementation Status API",
      "status": "yes",
      "description": "GET /api/v1/model-plane/implementation-status returns this catalog."
    }
  ],
  "claudeDonorRoadmap": [
    {
      "id": "plan-mode",
      "name": "Plan Mode",
      "status": "no",
      "description": "Structured plan-before-act workflow ported from Claude Code."
    }
  ]
}
```

### `GET /healthz` · `GET /readyz`

Kubernetes-style liveness and readiness probes.

## Layout

```
capability-core/
├── cmd/                 # main entrypoint (HTTP + gRPC servers)
├── internal/
│   ├── policy/          # capability policy engine
│   ├── registry/        # capability registry
│   ├── roadmap/         # implementation-status catalog + HTTP handler
│   └── server/          # HTTP/gRPC wiring
├── docs/
│   └── gap-analysis.md  # human-readable mirror of roadmap catalog
├── Dockerfile
├── go.mod
└── go.sum
```

## Catalog & Gap Analysis

The implementation-status catalog lives in
[`internal/roadmap/data.go`](internal/roadmap/data.go) and is the single source
of truth. The HTTP endpoint above serves it verbatim, and
[`docs/gap-analysis.md`](docs/gap-analysis.md) provides a human-readable mirror.
When the in-code catalog changes, update the doc so both agree.

## Security limitations

**Risk-level downgrade floor.** Before this was fixed, any caller holding the
plain `capability:write` scope could upsert a capability's `risk_level` down
to `low` — including on a high-risk, migration-seeded capability such as
`cap.command.shell` — which silently flips `policy/engine.go`'s
`High → Ask` decision to `Low → Allow`, disabling the human-approval gate for
that capability with no separate authorization step. `internal/registry/capabilities_store.go`'s
`Upsert` now floors any capability whose currently persisted `risk_level` is
`high` (or whose id is a known migration-seeded high-risk capability with no
readable prior row) so it cannot be lowered without the caller also holding
the `capability:risk:override` scope (`internal/authz/authz.go`); the same
check runs redundantly, and more cheaply, in the HTTP upsert handler
(`internal/api/capabilities.go`). Non-floored capabilities, and any upgrade,
are unaffected — this is a floor on `high`, not a general restriction on
changing risk levels. The floor check reads the current row and then writes
in a second round trip rather than one transaction, so a very tightly-timed
concurrent write could in principle still race past it; that residual is
accepted as narrow rather than engineered away.

**Risk-tier dispatch never inspects command or argument text.** Capability
governance in this service is risk-tier dispatch, not pattern matching:
`policy.EvaluateCapability` decides `allow` / `ask` / `deny` from a
capability's declared `risk_level`, scope, and durable grants — it never
parses, scans, or denylists the actual command string, arguments, or payload
a caller intends to send once a capability clears that decision. This is a
deliberate design choice (fixed-tier dispatch is simpler to reason about and
audit than a shell-command-canonicalizing denylist), not an oversight, and a
narrow content-inspection layer as defense-in-depth on top of it is a
known, explicitly out-of-scope idea for a future change — it is not being
worked on here.

## Development

```sh
# Build & test
go build ./...
go test ./...

# Run
go run ./cmd
```

## Status

This service itself is tracked as the `capability-core` entry under
**Service Checklist** in the catalog (currently `partial`).
