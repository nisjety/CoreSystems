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
