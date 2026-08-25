# Model Plane — Project Instructions

## What this is
A **Rust + Go agent platform**: Rust owns the hot path (invoke, streaming, session
assembly, execution loop, provider routing, multimodal), Go owns durable control
(workflows, registries, policy, scheduling). North star (`docs/GOAL.md`): reach
product-surface parity with `claude-code-fork` and selectively adopt ideas from
`hermes-agent`, `openai/codex`, GraphRAG, and others — not a file-for-file port.

## Tech Stack
| Layer | Technology |
|---|---|
| Hot path | Rust (`rust/services/{model-gateway,inference-core,session-core,execution-core}`) |
| Durable control | Go (`go/services/{orchestrator-core,capability-core,cost-core,sandbox-manager,browser-broker,letta-bridge,bridge-core,nats-provisioner}`) |
| Orchestration | Temporal (workflows + activities, 3 task queues) |
| Bus | NATS/JetStream (`mp.v1.*` subjects) |
| DB | Postgres (append-only `events`, `checkpoints`, ULID-prefixed keys) |
| Contracts | protobuf (`proto/`), shared Rust crates (`rust/crates/mp-*`), Go pkgs (`go/pkg` equivalents) |
| Sandboxing | `bwrap` (bubblewrap) + Landlock/seccomp, Linux-gated (`rust/services/execution-core/src/sandbox.rs`) |

## Code Style
- Rust owns anything on the request-hot-path; Go owns anything long-running/durable. Don't cross this line casually.
- Cross-service invariants that can't be a build dependency (two independently deployed services) are asserted at runtime and mutation-tested, not just commented — see `cross_service_loop_contract.rs`. Follow this pattern for any other cross-service constant/behavior coupling.
- Two tool-dispatch loops exist on purpose and must **stay separate**: model-gateway's `dispatch_tool` (read-only router, refuses side effects — the "plain chat" loop) and execution-core's `execute_step_inner` (capability/hook/permission pipeline around sandboxed execution — the "deployed agent" loop). Do not merge them; the refusal boundary between them **is** the authority boundary (see `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md` for the full reasoning).
- Prefer the smallest correct abstraction. A registry/interface for one implementation is premature — see the `letta-bridge` memory-adapter deferral and the sandbox-backend-trait deferral it set the precedent for, both in `docs/decisions/ledger.md`: build the seam when the second real backend lands, not before.
- Before proposing to merge, unify, or add a new registry/interface for something that already exists in two places: check `docs/decisions/ledger.md` first — it may already be a settled "rejected" or "deferred" call, not an oversight. Add new decisions there rather than letting them live only in a commit message.
- One canonical owner per capability — check `docs/capability-ownership-matrix.md` before adding anything that might duplicate an existing registry/store.

## Testing
- Rust: `cargo test -q -p <service> --all-targets` per service; `cargo test --workspace` for everything.
- Go: `go test ./...` and `go vet ./...` per service directory.
- Coverage: `cargo llvm-cov` (risk-based target: 80% on changed security/business-critical paths, not 100% globally — gaps must be explicitly accepted, not hidden).
- Read `MODEL_PLANE_STATUS.md` before trusting any "done" claim — it tracks source-verified vs. live-verified vs. blocked with dates.

## Build & Run
```bash
cd "apps/Model Plane"
./scripts/compose.sh up -d
cd rust && cargo test --workspace
cd go && go build ./... && go vet ./...
```
- `buf generate` for proto regen; `buf lint` has pre-existing debt, don't expect it clean.
- Release artifacts use a signed v3 format (`scripts/release-artifact.sh`) — never hand-roll a deploy that bypasses it.

## Project Structure
- `docs/GOAL.md` / `docs/ROADMAP.md` — target state and phased plan (ROADMAP is superseded by `MODEL_PLANE_ROADMAP.md` for production sequencing).
- `docs/gap-analysis.md`, `docs/gap-model.md` — the stub-replacement tracker; check before assuming a documented feature is real.
- `docs/external-ideas-harvest.md` — license-gated adoption plan mined from codex/hermes-agent/pi/daytona/claude-code. **Read the license matrix before porting anything from an external repo** (Apache = vendor freely, MIT = port freely, AGPL = clean-room only + legal sign-off, proprietary leaked = shapes only, zero code).
- `docs/capability-ownership-matrix.md` — one-owner-per-capability rulings; the authoritative de-duplication reference.
- `docs/core-research/*.md` — per-service "research dive" audits (source-grounded, dated — check the date before trusting a claim).
- `MODEL_PLANE_STATUS.md` / `MODEL_PLANE_ROADMAP.md` (repo root) — current production-readiness truth, supersedes older docs/ROADMAP.md claims.
- `docs/decisions/ledger.md` — settled design tradeoffs (proposed/implemented/rejected/deferred), so a rejected approach doesn't get silently re-proposed. `docs/postmortem/NNNN-*.md` holds the deeper root-cause write-ups the ledger's bigger entries point to.

## Conventions
- Commits: Conventional Commits (`feat(model-gateway): ...`, `fix(inference-core): ...`, `docs(model-plane): ...`).
- Commit messages explain **why**, often narrating what was measured and what didn't hold (see `docs/decisions/ledger.md`) — this repo values documenting rejected approaches, not just shipped ones.
- Never claim a feature is "done" without source + test evidence; the team has a documented history of correcting stale ❌/✅ claims (see ROADMAP.md's 2026-05-30 and 2026-07-11 correction notes).
