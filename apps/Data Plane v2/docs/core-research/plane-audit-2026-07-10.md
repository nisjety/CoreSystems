# Data Plane v2 Plane Audit

Date: 2026-07-02

Scope: `apps/Data Plane v2`

This is a plane-local audit report. It focuses on Data Plane v2 readiness and cross-plane blockers without editing service code.

## Current Shape

Data Plane v2 is the canonical durable knowledge layer. It owns documents, chunks, embeddings, retrieval, graph, wiki, source traces, quality gates, and related orchestration. The core split remains Go for durable/control services and Rust for hot-path indexing, embedding, retrieval, graph, and search adapters.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `make check-rs` | Pass with warning | Rust workspace checks; `retrieval-engine-rs` reports unused `RerankClient::new`. |
| `make build-go` | Pass | `documents-api-go`, `wiki-store-go`, `data-orchestrator-go`, and `data-quality-go` built. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` | Fail | Formatting drift in embedding, index, and retrieval Rust services. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Fail | `retrieval-engine-rs/src/pipeline/orchestrator.rs:58` hits `clippy::doc_lazy_continuation`. |
| `cargo test --workspace` | Pass | Rust tests pass; 3 pipeline e2e tests remain ignored behind docker-compose dependencies. |
| `gofmt -l services/*-go tools/migrator gen/go` | Fail | Formatting drift in data-orchestrator, data-quality, documents-api, and wiki-store Go files. |
| `make vet-go` | Pass | Go vet passed for documents-api, wiki-store, data-orchestrator, and data-quality. |
| `staticcheck` over Go services | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |

## Live Validation Addendum

Additional validations run against the local Data Plane runtime:

| Probe | Result | Notes |
|---|---|---|
| `documents-api-go` health | Pass | `/health` returns 200; unauthenticated `/v1/documents` returns 401. |
| retrieval-engine readiness | Pass | `/readyz` returns 200 with cache, Postgres, and Qdrant ready. |
| retrieval-engine Control mode | Partial/pass | Container runs with `CONTROL_PLANE_ENFORCEMENT=strict`. |
| documents-api Control consultation | Gap confirmed | `authctx` enforce mode still returns 503 because signature verification is explicitly not implemented. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P0 | Multi-tenant production readiness is still blocked by incomplete Control Plane consultation. | retrieval-engine is strict, but documents-api `pkg/authctx` still has unimplemented signature verification and fail-closed 503 enforce mode. | Implement documents-api JWT/JWKS verification and add tenant-isolation integration tests. |
| P1 | Data ingest contract changes have not propagated to Quarry-v2 tests/runtime test constructors. | Quarry-v2 `cargo test --workspace` fails because `DataPlaneIngestRequest` constructors lack `initiator_user_id` and `visibility`. | Coordinate with Ingestion Plane and patch contract constructors with explicit semantics. |
| P2 | Rust formatting drift spans hot-path Data services. | `cargo fmt --all -- --check` reports drift in `embedding-engine-rs`, `index-engine-rs`, and `retrieval-engine-rs`. | Run Rust formatting as a mechanical cleanup after coordinating with active branches. |
| P2 | Go formatting drift spans durable Data services. | `gofmt -l` reports drift in data-orchestrator, data-quality, documents-api, and wiki-store. | Run `gofmt` as a mechanical cleanup and keep it in CI. |
| P2 | Clippy warning blocks the strict Rust lint gate. | `retrieval-engine-rs/src/pipeline/orchestrator.rs:58` has a doc quote continuation missing the `>` marker. | Patch the doc comment and rerun `cargo clippy --workspace --all-targets -- -D warnings`. |
| P2 | `retrieval-engine-rs` has a dead or unwired reranker constructor. | `make check-rs`; `services/retrieval-engine-rs/src/search/rerank.rs:46`. | Remove it if dead or wire it through provider configuration. |
| P2 | Data docs still mix closed implementation work with current transition debt. | `docs/gap-data.md` records many closed gaps while also listing live blockers and scaffold/lab items. | Split present-state readiness from historical gap closure notes. |
| P2 | Some graph/eval follow-ups remain explicitly not done. | `docs/gap-data.md` lists AST-first extraction, graph exports/rebuild follow-ups, and `retrieval-eval-py` scaffold state. | Convert remaining unchecked items into owner/service issues. |
| P3 | Go static analysis is blocked by local toolchain skew, not by a confirmed code issue. | `staticcheck` reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain before treating staticcheck coverage as complete. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Full Rust test status | This pass ran `check-rs`, not `test-rs`. | Run `make test-rs` after contract drift is resolved if cross-plane tests depend on Quarry/Data shared types. |
| Integration readiness | `make test-integration` requires environment setup. | Run with `TEST_DATABASE_URL` and required local services. |
| Control consultation implementation shape | The blocker is documented, but the exact service API is not chosen here. | Design a small auth/org/quota validation contract with Control Plane owners. |

## Quality Gate

- Rust check: pass with warning.
- Go build: pass.
- Rust tests: pass.
- Rust format/clippy: fail.
- Go format: fail.
- Go vet: pass.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Integration tests: not run in this pass.

## Recommended Remediation Order

1. Define and implement Control Plane consultation for org/user/session/quota trust.
2. Coordinate Data Plane ingest contract defaults with Quarry-v2.
3. Remove or wire `RerankClient::new`.
4. Separate current readiness docs from historical gap-closure docs.
5. Run `make test-rs` and `make test-integration` with required local services.
