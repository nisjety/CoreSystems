# Quarry V2

Two-plane ingestion engine.

- **Runtime plane (Rust)** — hot path: scrape, crawl exec, browser leases, transforms, fingerprint/diff.
- **Control plane (Go)** — durable: jobs, stores, snapshots, artifacts, schedules, event history, Temporal orchestration, webhooks.
- **Lab (Python)** — experiments only: extraction prompts, anti-bot strategies, eval harness. Never in hot path.

Donor: `../Quarry` (Go). V2 rebuilds **execution boundary**, not product boundary. Public REST stays canonical.

## Layout

```
Quarry-v2/
├── crates/                      # Rust workspace
│   ├── quarry-core/             # shared IDs, envelopes, policy, errors
│   ├── quarry-security/         # preflight + discovered-URL checks
│   ├── quarry-transform/        # markdown/links/fingerprint/diff
│   ├── quarry-browser/          # browser lease + session affinity runtime
│   ├── quarry-runtime/          # driver select + fetch + action runtime + artifacts
│   └── quarry-edge/             # HTTP ingest boundary (axum), SSE, cache, handoff
├── services/
│   ├── quarry-control/          # Go control plane (resources REST/GraphQL)
│   └── quarry-orchestrator/     # Go Temporal workflows + schedules + webhooks
├── pkg/quarrycontracts/         # Go mirror of contracts (generated from OpenAPI/JSON Schema)
├── lab/                         # Python lab (extraction/evasion/eval, never in prod path)
├── docs/                        # CONTRACTS.md, ROADMAP.md, ARCHITECTURE.md
└── deploy/                      # compose + dev scripts
```

## Deployables

| Service                 | Lang  | Role                                                    |
|-------------------------|-------|---------------------------------------------------------|
| `quarry-edge-rs`        | Rust  | public ingest, fast path, SSE, preflight, cache resolve |
| `quarry-runtime-rs`     | Rust  | driver + fetch + browser + transform + artifacts        |
| `quarry-control-go`     | Go    | jobs/stores/snapshots/artifacts/profiles CRUD + history |
| `quarry-orchestrator-go`| Go    | Temporal workflows, schedules, webhooks, durable state  |

## Contract flow

```
client ──REST──▶ quarry-edge-rs ──┬─ fast path ─▶ quarry-runtime-rs
                                  └─ scheduled  ─▶ quarry-orchestrator-go ──▶ quarry-runtime-rs
                                                        │
                                                        └── publishes events ─▶ quarry-control-go (history)
```

## Start

Requirements for the Rust runtime:

- Rust 1.85+ (required by the `wreq` TLS emulation dependency)
- CMake 3.15+ and a C/C++ compiler for BoringSSL builds
- Standard Go toolchain for the control/orchestrator services

```
make bootstrap     # install toolchains + deps
make build         # cargo build + go build
make dev           # docker-compose up
```

## Docs

- [`docs/GOAL.md`](docs/GOAL.md) — north star, success criteria, anti-goals.
- [`docs/PLAN.md`](docs/PLAN.md) — detailed work plan per phase.
- [`docs/PROGRESS.md`](docs/PROGRESS.md) — live build state + test snapshot.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — phase order.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — plane split, lease model, middleware rule.
- [`docs/CONTRACTS.md`](docs/CONTRACTS.md) — frozen IDs, envelopes, policy, output.
