# Velion — Phase 2 Execution Progress & DoD Evidence

> Companion to `docs/PHASE_2_PLAN.md` (authoritative spec). One section per PR with
> DoD evidence (commands + result) and the no-new-fakeness proof. Base: `main` @ Phase 1
> merged (`15704f00`). Execution started 2026-06-20.

## Gate status (PR-0 prerequisite)
- **Phase 0** (CI gate, IDOR fix, de-fake): merged to `main` (`ef1e57af`, `ff2a3f59`, `b269d09a`).
- **Phase 1** (HITL, GDPR, insights, change-monitoring): merged to `main` (`15704f00`); in-flight
  cc-go Track-B work committed (not clobbered) via `5d633954`/`15704f00`.
- **Phase 1 green verification** (PR-0): see PR-0 below.

## W2 day-15 go/no-go (recorded per PR-0)
- **Decision date: 2026-07-05** (day 15 after the 2026-06-20 Phase-1 merge / Phase-2 start).
- **Criterion:** PR-4 (W2 recurring monitoring) proceeds **only if** Phase-1 Track C
  (C1 edge rebuild on `--features postgres-queue` + `QUARRY_EDGE__DATABASE_URL`, C2 gateway
  `monitoring.rs`, C3 SPA Monitoring tab) is **merged-and-green** AND a live
  `POST /v1/change/check` returns **200, not 501**. Otherwise W2 defers wholesale to Phase 3 —
  nothing half-built ships.
- **Cut order if the window compresses:** W1 → W3 → W2. **Never** cut W4 or E5.

---

## PR-0 — Reconcile Phase-1-B, confirm Phase 1 green, re-baseline + IDOR CI lint
**Status:** in progress.

DoD checklist:
- [x] In-flight cc-go Track-B work committed (not lost) — on `main` via `15704f00` (`internal/conversation`
      `repository.go`/`service.go`/`types.go`/`handlers.go` + `repository_test.go`/`service_test.go`).
- [x] No-client-header-org **CI lint** strengthened in `.github/workflows/velionv3-ci.yml`
      `fabrication-guard`: bans `fn org_id_from_headers`, asserts `x-org-id` + `x-velion-org-id`
      stay `STRIPPED_HEADERS` entries, and bans any `domains/` read of a client org header
      (`get("x-(velion-)?org-id")`). **Proven red on a deliberate violation** (probe domain reading
      `x-velion-org-id` + reintroduced helper → exit 1; STRIPPED_HEADERS removal → fail). Clean tree passes.
- [ ] Phase 1 merged-and-green (per-stack gate evidence below).
- [x] W2 day-15 go/no-go date on record (2026-07-05, above).

Green evidence (commands run on `main`):
- cc-go: `go build ./...` + `go test -race ./...` → green (`internal/conversation` ok; 0 FAIL/panic).
- user-core: `go build ./...` + `go test ./...` → green (grpc/http/users ok; 0 FAIL).
- gateway: `cargo fmt --check` ok; `cargo test` → 116 passed; `cargo clippy --all-targets -- -D warnings`
  was **red** on `browser.rs:549` (`clippy::unnecessary_lazy_evaluations`) → fixed
  (`unwrap_or_else(|| <const struct>)` → `unwrap_or(<const struct>)`); re-run **green**.
- SPA: `pnpm verify` (typecheck → lint → test → build) → (recording).
