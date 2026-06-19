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
- [x] Phase 1 merged-and-green — verified green across all four stacks (evidence below).
- [x] W2 day-15 go/no-go date on record (2026-07-05, above).

Green evidence (commands run on `main`):
- cc-go: `go build ./...` + `go test -race ./...` → green (`internal/conversation` ok; 0 FAIL/panic).
- user-core: `go build ./...` + `go test ./...` → green (grpc/http/users ok; 0 FAIL).
- gateway: `cargo fmt --check` ok; `cargo test` → 116 passed; `cargo clippy --all-targets -- -D warnings`
  was **red** on `browser.rs:549` (`clippy::unnecessary_lazy_evaluations`) → fixed
  (`unwrap_or_else(|| <const struct>)` → `unwrap_or(<const struct>)`); re-run **green**.
- SPA: `pnpm verify` (typecheck → lint → test → build) → green (41 files / 156 tests; built in 5.51s)
  after isolating the dev-bypass test.

---

## PR-1 — W4 ticket.classification executor (THE SPINE)
**Status:** complete + live-verified.

What shipped:
- `internal/consumers/`: a reusable `DurableConsumer` scaffold (QueueSubscribe + Durable +
  ManualAck + AckWait + MaxAckPending(1)) — PR-3 mirrors this — and `AIActionExecutor`, an
  in-process durable JetStream consumer on `velion.application.conversation.ai_action.reviewed`.
- On `decision=approved` + `kind=ticket.classification`: load the action (`GetAIAction`), find the
  suggested ticket, **claim** it atomically (`MarkAIActionExecuted`: `status approved→executed`,
  the idempotency gate), promote suggested→open + apply routing via `Service.UpdateTicket`, emit
  `ai_action.executed`. Rolls the claim back on a promote failure so JetStream can redeliver.
- Repo additions: `GetAIAction`, `MarkAIActionExecuted`, `UnmarkAIActionExecuted` (org-scoped).
- Loop reachability fixes in `AiActionReviewPanel.tsx` discovered during live verification: the
  panel filtered `status:'suggested'` but the producer writes the pending status as
  `suggest_ticket`, and `summarize()` read `proposed_ticket` but the payload uses
  `suggested_fields` — both fixed so pending actions and their details actually render.
- B3 copy flipped **"Decision recorded" → "Applied — the suggested ticket was promoted and routed"**
  for approve (reject stays "Decision recorded — suggestion dismissed"); the false "not
  auto-executed yet" note removed. Flipped **only after** live verification.

Plane rule honored: executor lives in conversation-core (Application Plane), never Model Plane.

DoD evidence:
- `go build ./... && go vet ./... && go test -race ./...` → green (`internal/consumers` +
  `internal/conversation` ok; 0 fail/panic). Tests cover: promote+route on approve, idempotent
  duplicate delivery (sequential + 16-goroutine concurrent → exactly one promote + one emit),
  rejected/wrong-kind/foreign-org no-op, transient-error retry, promote-failure rollback, malformed-ack.
- **Live smoke through the running stack** (cc-go rebuilt + restarted; executor logged
  "subscribed to velion.application.conversation.ai_action.reviewed"): ingest → classify
  (suggested ticket) → approve → ticket promoted **suggested→open** + `category=billing` routing
  applied → ai_action **executed**; re-approve left it **open** (no double-apply). RESULT: PASS.

No-new-fakeness proof: "Applied" copy ships only because a real approve→promote round-trip was
verified end-to-end against running NATS+Postgres+cc-go. The review panel now renders real pending
actions (not an empty list) and their real suggested fields.
