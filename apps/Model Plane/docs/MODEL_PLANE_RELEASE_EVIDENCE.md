# Model Plane release evidence register

Status date: 2026-08-17 (Europe/Oslo)

This is the current release-evidence register for the Model Plane. It is
deliberately separate from historical audits and design proposals. A claim is
not release-ready merely because source code or a unit test exists.

## Evidence classes

| Class | Meaning |
| --- | --- |
| `source` | The implementation exists in the checked-out source. |
| `test` | A repeatable local test covers the claim. |
| `integration` | The claim passed against release-shaped dependencies. |
| `staging` | The claim was observed in a deployed, immutable candidate. |
| `candidate` | A signed artifact and its external runtime policy were verified. |
| `rollback` | The separately signed rollback artifact was restored and exercised. |
| `production` | The approved candidate was observed in production under the declared window. |

Only `candidate` and `rollback` evidence can support a production promotion.

### Fresh objective audit (2026-08-17 05:05 UTC)

- The acceptance-trace validator now requires a later `reconciled` receipt for
  each individual ambiguous crash/timeout/provider-loss attempt, matched by
  the exact operation and idempotency tuple; a receipt for another attempt can
  no longer close that uncertainty window. Its negative contract regression
  passes. This strengthens fixture evidence only; it is still not a live
  owner-effect runner.

- `scripts/tests/capability-health-proof.sh` passed the tenant/global
  authorization guards, explicit generic global allowlist, `tickets.create`
  denial, and signed disposable-Postgres no-global-row-write evidence.
- `scripts/tests/scheduled-step-proof.sh` passed the Control decision,
  Orchestrator handoff, Session receipt ledger, Execution Core service-owned
  `ExecuteScheduledStep`, and retry/unknown contracts. The legacy user-bound
  `ExecuteStep` path remains unchanged.
- `scripts/tests/tickets-create-proof.sh` passed the V3 boundary, Control
  reservation/commit fence, Conversation owner interleavings, exact
  continuation tuple, provider-receipt reconciliation, and durable
  `unknown_outcome` paths. The Model allowlist remains empty.
- Capability Core and Orchestrator full Go suites, focused Execution Core
  scheduled Rust tests, and the aggregate `scripts/tests/coresystem-qm-proof.sh`
  all pass at source/disposable scope.
- A read-only browser snapshot of the existing Microsoft-authenticated
  AQUATIQ AS Space shows the server-derived Space shell, five conversation
  projections, agent cards, and the Activity/Agent navigation. No message,
  approval, connector, schedule, or owner effect was submitted.
- Release status remains blocked: candidate/rollback artifacts are absent,
  capabilities are `source_only`, proposed thresholds are not operator
  approved, and live service bindings/Auth Core registrations, provider/ZDR,
  deployed continuation, HA delivery, owner-effect, and candidate evidence
  remain open. No credential value was read, generated, rotated, or inserted.

### Continuation verification (2026-08-17 05:18 UTC)

- Re-ran the focused Capability Core, scheduled-step, and `tickets.create`
  proofs plus the full `scripts/tests/coresystem-qm-proof.sh` aggregate after
  tightening the acceptance-trace validator; all passed. The validator now
  rejects an ambiguous attempt unless a later reconciliation matches its exact
  operation and idempotency tuple.
- The read-only conformance report remains `blocked`: candidate and rollback
  are absent, the promotion ledger is `source_only`, acceptance thresholds are
  still `proposed`, and the scheduled/cross-plane preflights still report
  missing operator-owned bindings and Auth Core registrations. No credential
  value was read, generated, rotated, or inserted.

- Scheduled-step Orchestrator now fails closed when Execution Core returns a
  nil response or an unrecognized status: it cannot advance to another step
  without a valid terminal/approval/unknown receipt. Focused activity tests and
  `scripts/tests/scheduled-step-proof.sh` pass.

### Runtime checkpoint (2026-08-17 05:44 UTC)

- The aggregate CoreSystem QM proof remains green at source/disposable scope;
  full Capability Core and Orchestrator Go suites also pass.
- The live conformance report remains blocked: candidate/rollback artifacts
  are absent, the capability ledger remains `source_only`, and acceptance
  thresholds remain `proposed`.
- Presence-only dev checks still report empty scheduled service-token
  bindings and missing Conversation/Model authority references plus Auth Core
  principal/scope registrations. These are operator-owned handoff gates; no
  credential value was read, generated, rotated, or inserted.
- Scheduled-step inference correlation is now namespaced by the exact
  `org_id`/`idempotency_key` tuple used by Session Core receipts; the focused
  and eight-test Execution Core scheduled suite passes.
- Fresh conformance remains blocked with absent candidate/rollback artifacts,
  `source_only` capabilities, proposed thresholds, and blocked dev/cross-plane
  preflights.

### Proof-environment checkpoint (2026-08-17 05:56 UTC)

- The standalone `scripts/tests/tickets-create-proof.sh` passed again,
  including the Control real-Postgres reservation fence and Conversation owner
  interleavings.
- Two aggregate reruns reached disposable Postgres lanes but were blocked by
  Docker Desktop connection resets (Conversation owner interleavings, then
  Session approval-continuation lease recovery). No source assertion failed;
  only the disposable containers/proof clients were stopped, with no running
  CoreSystem service or credential touched.
- The aggregate QM result must be rerun after Docker Desktop recovery before it
  can be treated as fresh evidence. Release status remains blocked by the
  existing candidate/rollback, provider/ZDR, deployed continuation, delivery
  HA, owner-effect, authority-registration, and capability-promotion gates.

### Source-suite checkpoint (2026-08-17 05:59 UTC)

- Full Capability Core Go tests, full Orchestrator Core Go tests, and the
  eight-test Execution Core scheduled Rust suite passed after the Docker
  connection-reset failures. These are source/test evidence only and do not
  substitute for disposable Postgres, deployed continuation, or candidate /
  rollback observations.

### Proof-harness hardening (2026-08-17 06:00 UTC)

- The tickets disposable-Postgres harness now pins both local DSNs to
  `sslmode=disable`, matching the plaintext development containers and
  removing an avoidable TLS negotiation/reset source. Docker Desktop remains
  unresponsive, so the corrected harness still needs a fresh end-to-end run.

### Objective continuation (2026-08-17 06:06 UTC)

- Focused Capability Core authorization tests passed for the exact global
  identity, explicit generic allowlist, owner-action exclusion, tenant/global
  row isolation, and no persistence on denied requests. Full Capability Core
  and Orchestrator Core Go suites, scheduled-step workflow/activity tests, the
  eight-test Execution Core scheduled suite, and the 19-test tickets suite all
  passed.
- Control's real-Postgres reservation test already proves revocation before
  commit is denied; the standalone tickets proof passed again.
- A fresh in-app-browser attempt reached the local V3 Vite document but left
  the application root empty, so it is not new Microsoft-authenticated Space
  evidence. The prior authenticated read-only Space snapshot remains the
  latest valid browser observation.
- Docker Desktop remains the blocker for disposable Postgres and aggregate /
  conformance reruns. No credential value was read, generated, rotated, or
  inserted; Model capabilities remain disabled.

### Proof rerun (2026-08-17 06:24 UTC)

- `scripts/tests/capability-health-proof.sh` passed all R-2 lanes, including
  signed real-Postgres no-global-row-write evidence.
- `scripts/tests/scheduled-step-proof.sh` passed workflow retry/unknown,
  activity handoff, Execution Core, and both Session Core real-Postgres receipt
  tests. Its harness retries only Docker host-forwarder transport failures;
  assertion/build failures remain fatal.
- Tickets source, Control unit, Conversation owner, and V3 boundary lanes
  pass. The disposable Control Postgres phase still resets before migration;
  the bounded retry wrapper reports this as an environment blocker rather than
  masking a source failure.
- The running V3 endpoint is reachable, but the in-app browser still renders
  an empty application root. No new Microsoft-authenticated Space evidence is
  claimed and no login or effectful action was submitted.

### Aggregate attempt (2026-08-17 06:28 UTC)

- The aggregate runner re-confirmed the R-2 unit lanes but its disposable
  Capability Core Postgres test hung in Docker before producing a result. The
  runner was stopped and only its own stale cleanup client was terminated.
- The aggregate result is therefore not fresh evidence. The standalone R-2
  and scheduled-step proofs remain valid; tickets disposable Control Postgres
  and the aggregate Docker lane still require a healthy runtime rerun.

### Runtime retry (2026-08-17 07:21 UTC)

- R-2 and scheduled-step proofs remain green; the scheduled receipt harness
  passed both real-Postgres tests with transport-only retry.
- Tickets source lanes pass, but Docker could not complete creation of its
  disposable Control Postgres container even after switching to the
  create/start path. The proof client was stopped without touching running
  CoreSystem services or credentials.
- The remaining failure is therefore disposable-container capacity/forwarding,
  not a Capability, Control, Conversation, or Execution source failure.
  Aggregate and fresh conformance evidence remain open.

### Owner-fence test hardening (2026-08-17 07:23 UTC)

- The cross-database `tickets.create` interleaving test now queries the real
  Control `space_authority_revisions` row for both the initial and final
  authority checks. After a revision bump, the final check must deny before
  Conversation writes. The focused test compiles; it runs live only when the
  disposable databases are available.

### Fresh objective verification (2026-08-17 04:43 UTC)

- `scripts/tests/capability-health-proof.sh` passed the R-2 authorization,
  stale/unhealthy policy, and signed disposable-Postgres no-global-row-write
  proofs. The full Capability Core Go suite also passed. The generic global
  reporter remains limited to `cap.command.sandbox` and `cap.command.shell`;
  the generic route cannot attest `cap.tool.ticket.create`, and denied
  requests create no global capability or attestation-audit row. The handler
  now repeats the global-lane binding defensively: only
  `service:execution-core` with the reserved `global` organization and global
  health scope can select the global store method, even if a caller reaches
  the handler without the public route middleware.
- `scripts/tests/scheduled-step-proof.sh` passed the Temporal retry/unknown,
  activity handoff, Execution Core service-owned contract, and Session
  disposable-Postgres receipt ledger. The Orchestrator Go suite and focused
  Execution Core Rust scheduled-step tests passed; no user credential was
  added to legacy `ExecuteStep`.
- `scripts/tests/tickets-create-proof.sh` passed the Execution/Control/
  Conversation/V3 contracts, Control reservation fence, Conversation owner
  interleavings, exact continuation tuple, and durable unknown/receipt paths.
  The aggregate `scripts/tests/coresystem-qm-proof.sh` also passed. These are
  source/disposable proofs only; no Model capability was enabled.
- A fresh `scripts/coresystem-conformance.sh --json` capture remains
  `blocked` (`candidate=absent`, `rollback=absent`, dirty worktree,
  `source_only`). The cross-plane preflight still reports missing operator-
  owned Conversation/Model bindings and Auth Core principal/audience/scope
  registrations, plus open provider/ZDR, approval-continuation, delivery-HA,
  owner-reporter, owner-effect, and candidate evidence. No credential value
  was read, generated, rotated, or inserted.
- The Microsoft-authenticated browser remains on the Personal Space. The
  read-only snapshot shows the AQUATIQ AS Space shell, and the current
  navbar, Space context/threads, and transcript requests are returning
  `200 OK`. No message, approval, connector, capability, or owner effect was
  submitted; this is authenticated UI evidence, not a live effect or release
  gate.
- A follow-up conformance capture at 04:50 UTC remains `blocked` with the same
  candidate/rollback, runtime-binding, Auth Core registration, and live
  provider/approval/HA/owner-effect blockers. The aggregate source/disposable
  proof still exits `0`; this is not a promotion transition.

### Acceptance thresholds and stop-rule contract (2026-08-17 04:56 UTC)

- Added `docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv` and its validator
  `scripts/tests/acceptance-thresholds-test.sh`. The nine rows make the
  existing source-derived limits explicit: decision/step TTL and clock skew,
  delivery claim lease, callback skew, unknown reconciliation age, provider
  receipt deadline, HA recovery window, and zero-content ZDR residuals.
- The values are deliberately marked `proposed`, not operator-approved. The
  conformance report now emits `acceptance_thresholds=proposed` and adds
  `acceptance_thresholds_unapproved` to its blockers. This keeps a healthy
  Docker stack, a fixture trace, or a source test from being mistaken for
  release readiness. Promotion must remain blocked until Release/Infra and
  each owning plane approve or revoke every row and record measured evidence.
- `scripts/tests/coresystem-qm-proof.sh`, the acceptance-trace contract, and
  conformance-artifact contract pass with the new required artifact. No
  credential was read, generated, rotated, or inserted.

### Fresh acceptance-trace contract and browser recheck (2026-08-17 04:25 UTC)

- Added `scripts/coresystem-acceptance-trace.sh`, a secret-free and
  content-free validator for the canonical
  `Space → BFF → Control → owner → Activity` trace. It requires ordered
  authority/reservation/receipt/projection/reconciliation stages, explicit
  revocation and duplicate cases, and `unknown_outcome` followed by
  reconciliation for crash, timeout, and provider-loss cases. It never calls a
  service, reads credentials, or promotes a capability.
- Added
  `scripts/tests/coresystem-acceptance-trace-test.sh` and wired it into the
  CoreSystem conformance file set and `scripts/tests/coresystem-qm-proof.sh`.
  The aggregate source/disposable proof passes with this contract included.
  This validates the trace shape only; it is not deployed owner-effect,
  provider/ZDR, candidate, or rollback evidence.
- Increased the disposable Application delivery Postgres readiness window to
  120 seconds so Docker Desktop startup contention does not create a false
  aggregate-proof failure. The runner still uses only a short-lived local
  database and remains provider-disabled.
- A fresh Microsoft-authenticated, read-only browser check found `200 OK` for
  navbar, Space context/threads, and transcript routes. The earlier 502/503
  responses are therefore recorded as transient historical runtime evidence,
  not a current source defect. The browser remained read-only; no message,
  approval, connector, capability, or owner effect was submitted.
- The conformance report remains blocked by absent candidate/rollback
  artifacts, `source_only` capability state, missing operator-owned authority
  bindings and Auth Core registrations, and open provider/ZDR, approval,
  delivery-HA, owner-reporter, and owner-effect evidence.
- Final serial rerun at 04:35 UTC passed the complete
  `scripts/tests/coresystem-qm-proof.sh` aggregate, including the ticket
  interleavings and the new acceptance-trace contract. The ticket disposable
  Postgres wait was widened to two minutes and now suppresses expected
  pre-publication port warnings; this improves proof reproducibility only and
  does not change the release-gate status.

### Fresh objective continuation (2026-08-17 04:00 UTC)

- `scripts/tests/capability-health-proof.sh` passed the R-2 authorization,
  stale/unhealthy policy, and signed disposable-Postgres no-global-row-write
  regressions. The generic reporter remains restricted to its explicit
  execution capability allowlist; `cap.tool.ticket.create` remains denied on
  that route, and the Model capability ledger remains `source_only`.
- `scripts/tests/scheduled-step-proof.sh` passed the canonical-template,
  Control step-decision, Session authority/claim/receipt, Execution Core
  service-owned lane, and Temporal retry/unknown checks. This remains
  source/disposable evidence; Auth Core registrations, provider/ZDR, deployed
  Temporal/provider crash proof, and candidate observation remain open.
- `scripts/tests/tickets-create-proof.sh` passed the V3 boundary, Control
  decision/reservation, Conversation owner interleavings, exact continuation
  tuple, provider-receipt reconciliation, and `unknown_outcome` checks. No
  Model capability was enabled; deployed signer/verifier configuration,
  authenticated transport, provider receipt, and candidate evidence remain
  open.
- `scripts/tests/application-delivery-proof.sh` passed its disposable
  Postgres lease/receipt and feed-projection proof on retry after a transient
  disposable-container readiness race. Provider/ZDR, multi-replica HA,
  crash-before/after-submit, and candidate/rollback evidence remain open.
- `scripts/tests/coresystem-qm-proof.sh` then passed the complete aggregate,
  including customer-facing chat/reconnect/ZDR/approval scenarios, all empty-
  default configuration contracts, the delivery proof, the conformance
  artifact contract, and the cross-plane preflight contract. A stale
  `RunService` test double was updated to implement the additive scheduled-step
  authority RPC; the focused customer proof and `cargo fmt --check` pass.
- The read-only Microsoft-authenticated V3 browser session remains on the
  Personal Space. Its console currently reports repeated `502 Bad Gateway`
  and `503 Service Unavailable` responses for transcript, Space context,
  navbar, and Space-thread routes. No message, approval, connector, or other
  effectful action was submitted. A healthy Docker container is therefore not
  sufficient to claim the authenticated C0/Space proof.
- No credential was read, generated, rotated, or inserted. The next safe
  action is to resolve the runtime readiness/projection failure and obtain the
  operator-owned dev authority handoff; do not change the allowlist or enable
  scheduled/ticket effects before candidate evidence exists.
- A fresh `scripts/coresystem-conformance.sh --json` capture at 04:09 UTC
  remains `blocked`: the worktree is dirty, candidate and rollback artifacts
  are absent, the promotion ledger is `source_only`, and the dev-runtime and
  cross-plane preflights still report missing operator-owned bindings,
  Auth Core scope registrations, provider/ZDR, approval, delivery-HA,
  owner-reporter, and owner-effect evidence.

### Current objective verification (2026-08-17 continuation)

- Capability Core R-2 unit, package, and real-Postgres negative proofs pass. The
  tenant/global-row guard, generic reporter allowlist, and dedicated
  `cap.tool.ticket.create` route remain enforced; the Model allowlist is still
  empty.
- Orchestrator scheduled-run input is canonical-template/digest bound, and new
  scheduled turns use `ExecuteScheduledStep` with fresh per-step Control
  authority, Session claim/receipt, and explicit timeout/unknown handling. The
  focused Go/Rust suites pass; no user credential was added to legacy
  `ExecuteStep`.
- Control's scheduled-step issuer now requires the additive Session Core
  `ResolveScheduledStepAuthority` read under the separate
  `session:scheduled-step-authority` scope. Session derives the subject and
  exact prepared-run schedule/fire/template/policy/idempotency tuple from
  durable metadata (including the newly persisted idempotency binding), and
  Control rejects mismatches before refreshing Space authority and signing.
  `cargo check -p session-core --tests`, focused Control Go tests, the
  scheduled-step config contract, and `scripts/tests/scheduled-step-proof.sh`
  pass. Auth Core scope registration and deployed Temporal/provider evidence
  remain open.
- Governed `tickets.create` continuation re-parses the frozen descriptor,
  recomputes the exact schema/payload/idempotency tuple, requests fresh Control
  authority, and forwards the frozen owner only to Conversation Core. The
  focused owner/interleaving/unknown-outcome proofs pass.
- A Microsoft-authenticated V3 read-only browser session reached the Personal
  Space and rendered its conversation and Activity projections. No owner-effect
  receipt or capability availability was exposed; no message, agent, connector,
  or effectful action was submitted.
- Fresh read-only verification at 2026-08-17 02:00 UTC showed the same
  authenticated `Aquatiq AS / Expert` session. Personal Space displayed one
  completed conversation, Activity displayed only the conversation/run
  projection with no owner receipt, and Agent displayed no bound agent. This
  is UI/projection evidence only; it does not prove Space registration,
  capability health, owner effects, or cross-plane delivery.
- The current conformance artifact remains `blocked` with absent candidate and
  rollback, missing operator-owned service bindings/scope registrations, and
  open provider/ZDR, deployed approval, and candidate evidence. No credential
  was read, generated, rotated, or inserted during this verification.

### Scheduled preparation retry proof (2026-08-17 03:15 UTC)

The Session Core disposable-Postgres harness now runs both the scheduled-step
claim/receipt idempotency test and a prepared-fire retry test. The latter
creates a service-owned scheduled thread, retries the same `(org, owner,
system_thread_key)` fire with a fresh Control decision reference, and proves
that the same thread is returned and only one row exists. This closes the
source/integration proof for the retry fence; the live scheduled lane remains
blocked by operator-owned service bindings, Auth Core registrations,
provider/ZDR evidence, and candidate promotion.

### Microsoft-authenticated Space observation (2026-08-17 03:18 UTC)

The existing Microsoft-authenticated browser session reached the shared
`AQUATIQ AS` Space as `Aquatiq AS / Expert`. The Space rendered five
conversation projections, five Activity conversation/run entries, and three
Control-bound agent cards (including the Teams/Messenger binding metadata).
The Work and Knowledge tabs still explicitly report that Model/Data have not
published their Space projections. No message, connector, approval, agent
mutation, or owner effect was submitted; this is read-only UI evidence and
does not clear the cross-plane or release gates.

### Owner-action final Control recheck (2026-08-17)

Conversation Core's private `tickets.create` boundary now requires the
current-Control authority validator at construction time. It rechecks the
signed decision before requesting the Control owner reservation and again
immediately before the owner transaction. A denied final recheck durably marks
the local operation `cancelled` without writing a ticket; an unavailable
recheck marks it `unknown` and requires reconciliation. Focused
`internal/http` tests pass for the two-read path, missing-validator fail-closed
startup, and revocation between reservation and owner effect. This is source
hardening only: the Control reservation remains the cross-plane ordering
receipt, and a live authenticated revocation/commit interleaving plus deployed
transport proof are still required before Model availability.

### Application delivery proof hardening (2026-08-17 01:59 UTC)

`scripts/tests/application-delivery-proof.sh` no longer skips its real-Postgres
lease/receipt test when a caller has not supplied database variables. It now
creates a short-lived `postgres:16-alpine` container, runs the notification
attempt, callback-before-projection, two-worker lease-fencing, idempotent feed
replay, and delivered Activity/Inbox projection test against that container,
and removes it on exit. `application-delivery-proof-test.sh` pins the runner to
this disposable-only behavior and rejects shared database or credential-shaped
configuration. The aggregate `coresystem-qm-proof.sh` passed with both the
delivery proof and its contract test. This strengthens source/disposable
evidence only; provider/ZDR attestation, real multi-replica HA replay, and
candidate/rollback evidence remain open.

### Active-objective re-audit (2026-08-17 02:00 UTC)

- Capability R-2 unit tests and both signed real-Postgres negative tests passed.
  The tenant health writer cannot mutate a global row, the generic reporter is
  restricted to `cap.command.sandbox`/`cap.command.shell`, and
  `cap.tool.ticket.create` remains denied on the generic route with no global
  row or attestation-audit write.
- `scripts/tests/scheduled-step-proof.sh` passed the Temporal retry/unknown
  workflow tests, exact Control activity handoff, Execution Core contract, and
  disposable Session receipt ledger. The dedicated lane remains fail-closed in
  the running stack because its operator bindings/scopes are absent.
- `scripts/tests/tickets-create-proof.sh` passed the Execution Core tuple
  checks, Control decision/reservation tests, Conversation owner checks, and
  disposable-Postgres reservation/commit/revocation interleavings. The Model
  allowlist remains empty pending deployed signer, transport, provider receipt,
  and candidate evidence.
- Microsoft-authenticated V3 read-only verification showed Personal Space
  `p97esa9rwyrzdnqgnj5tv0gdq18certp` with one completed conversation and an
  Activity projection for the conversation/run. No owner receipt, capability
  availability, or bound Agent was exposed, and no effectful UI action was
  taken.

### Fresh release preflight (2026-08-17 02:05 UTC)

The running Docker topology is healthy, but both release preflights remain
blocked without exposing credential values. The scheduled preflight still
finds the three schedule service-token bindings empty. The cross-plane
preflight still finds the Conversation owner-decision/reservation references,
Model execution references, and their User Core/Auth Core scope registrations
missing; provider/ZDR, deployed approval-continuation, and candidate evidence
remain open. `scripts/coresystem-conformance.sh --json` therefore remains
`status=blocked` with `candidate=absent`, `rollback=absent`, dirty worktree,
and every capability `source_only`. No credential was read, created, rotated,
or injected.

### Fresh aggregate rerun (2026-08-17 02:10 UTC)

After one disposable-container startup race, the isolated scheduled receipt
proof passed and the aggregate was rerun successfully. The aggregate passed
Capability R-2, scheduled-step, approval continuation, `tickets.create`,
customer E2E, configuration contracts, Application delivery, its new
disposable-delivery runner contract, and conformance-artifact contracts. The
scheduled proof runner now prints the captured child log when a disposable
sub-proof fails, so an environment failure cannot be mistaken for a source
pass or fail. This remains source/disposable evidence; the conformance report
still blocks live promotion.

The 02:11 UTC Microsoft-authenticated browser check reached the same Personal
Space and confirmed the Activity projection contains only the completed
conversation/run entries; the Agent tab still reports no bound agent. No
effectful control was used.

### Current objective rerun (2026-08-17 02:26 UTC)

- Capability Core's focused unit/package suite and both signed disposable-
  Postgres no-write regressions passed. The tenant health writer cannot mutate
  a global row, the generic reporter remains limited to its explicit probe
  allowlist, and `cap.tool.ticket.create` remains unavailable on the generic
  route.
- `scripts/tests/scheduled-step-proof.sh` passed the workflow retry/unknown,
  exact activity handoff, Execution Core service-owned lane, and Session
  receipt-ledger checks. `scripts/tests/tickets-create-proof.sh` passed the
  V3 gateway boundary, owner tuple, Control reservation fence, Conversation
  owner interleavings, receipt, and unknown-outcome checks. These remain
  source/disposable proofs; the Model allowlist is unchanged.
- `scripts/tests/coresystem-qm-proof.sh` passed all aggregate lanes, while
  `scripts/tests/release-contract-test.sh` passed its artifact contract. The
  scheduled dev preflight remains blocked by the three empty schedule
  service-token bindings. The cross-plane preflight remains blocked by the
  missing Conversation owner-decision/reservation and Model execution
  references, User Core/Auth Core scope registrations, and open provider/ZDR,
  approval-continuation, and candidate evidence. It now also reports the
  Application delivery HA/provider gate, the dedicated owner-action health
  reporter observation, and the owner-effect Activity receipt as separate
  blockers rather than collapsing them into topology health.
- The aggregate runner now includes the cross-plane preflight contract, so a
  source/disposable green result also proves that those live blockers are
  intentionally surfaced and remain fail-closed.
- The read-only conformance report remains `status=blocked` with
  `candidate=absent`, `rollback=absent`, dirty-worktree, and every capability
  `source_only` (latest capture: 2026-08-17 02:31 UTC). No credential was
  read, generated, rotated, or inserted.
- A fresh Microsoft-authenticated V3 read-only check reached the Personal
  Space. Activity showed only the completed conversation/run projection and
  the Agent view reported no bound agent. No message, connector, approval, or
  other effectful UI action was submitted.

### Fresh continuation verification (2026-08-17 02:44 UTC)

- Capability Core's focused package suite passed again, including the tenant
  HealthWriteScope-to-global no-write regressions and the generic global
  reporter allowlist. The reserved `cap.tool.ticket.create` capability is
  still denied on the generic health route.
- `scripts/tests/scheduled-step-proof.sh`,
  `scripts/tests/tickets-create-proof.sh`, and the aggregate
  `scripts/tests/coresystem-qm-proof.sh` passed. The aggregate now includes
  the V3 gateway ticket boundary and cross-plane preflight contract. These
  remain source/disposable proofs only.
- The read-only preflights remain correctly fail-closed: the scheduled lane
  has empty schedule service-token bindings; the cross-plane lane lacks the
  Conversation owner-decision/reservation, dedicated owner-reporter, and
  Model execution references, exact User Core/Auth Core scope registrations,
  and live provider/ZDR,
  approval-continuation, delivery-HA, owner-reporter, owner-effect, and
  candidate evidence. `coresystem-conformance.sh --json` remains blocked with
  `candidate=absent`, `rollback=absent`, dirty worktree, and every capability
  `source_only`. No credential was read, generated, rotated, or inserted.
- Microsoft-authenticated V3 read-only evidence reached the Personal Space
  (`p97esa9rwyrzdnqgnj5tv0gdq18certp`). The Activity tab showed only the
  completed conversation/run projection; the Agent tab reported no bound
  agent; and the Work tab explicitly reported that Model Plane has not yet
  published a Space projection. No message, approval, connector, or other
  effectful UI action was submitted.
- 02:51 UTC: the cross-plane preflight contract was tightened to check the
  dedicated Conversation Core owner-action reporter's four non-secret
  configuration references separately from the generic execution reporter.
  Its contract and aggregate proof still pass, and the live report now names
  those missing inputs rather than collapsing R-2 into generic topology.
- The governed `tickets.create` adapter now rejects plaintext Control or
  Conversation Core URLs for service-name, private-network, and remote hosts.
  HTTP is available only through an explicit
  `EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK=true` development opt-in and
  only for IP-loopback endpoints; the default remains false. The transport
  tests, configuration contract, and full tickets proof pass. This is a
  source/security hardening, not deployed authenticated-transport evidence.
- 03:09 UTC: after one disposable-Postgres startup race, the aggregate
  `coresystem-qm-proof.sh` rerun passed all lanes, including Application
  delivery and the updated ticket transport/configuration contract. The
  conformance report still correctly remains blocked (`candidate=absent`,
  `rollback=absent`, `source_only`) because no operator-owned runtime
  references or release evidence were added.

## 2026-08-17 runtime and proof checkpoint

This checkpoint is secret-free and does not create, read, inject, or rotate
credentials.

- The canonical evidence capture contract is now
  `scripts/coresystem-conformance-artifact.sh OUTPUT_PATH`. It persists the
  secret-free JSON conformance result once, refuses to overwrite an existing
  path, preserves the blocked/ready exit status, and reserves explicit fields
  for image/config/migration digests, authority-key references, policy/catalog
  hashes, feature flags, owner, receipts, candidate state, and rollback
  linkage. `scripts/tests/coresystem-conformance-artifact-test.sh` verifies
  schema, redaction, blocked-status preservation, and overwrite refusal. This
  closes the artifact-contract implementation only; no candidate or rollback
  artifact has been created.

### Fresh aggregate proof (2026-08-17 01:32 UTC)

`scripts/tests/coresystem-qm-proof.sh` exited `0` after the artifact-path
correction. It passed Capability Core R-2 authorization and real-Postgres
no-write checks, the scheduled-step workflow/activity/Execution Core/Session
receipt proof, approval-continuation lease recovery, the full `tickets.create`
owner/interleaving proof, both configuration contracts, Application delivery
source evidence, and the conformance-artifact contract. This remains
source/disposable-dependency evidence; it does not promote any capability.

The live conformance report remains `blocked`: candidate and rollback are
absent, the worktree is dirty, every capability remains `source_only`, and the
runtime preflight names missing scheduled, Conversation owner-effect, and
Model execution bindings plus open provider/ZDR, approval, and candidate
evidence. No credential was read, generated, rotated, or inserted while
running this proof.

### Fresh objective rerun (2026-08-17 continuation)

The aggregate `scripts/tests/coresystem-qm-proof.sh` was rerun after the
scheduled-step proto added `get_scheduled_step_context` to the Run Service. The
Model Gateway approval E2E mock now implements that method as an explicit
`unimplemented` test boundary, and the aggregate passed the customer-facing
chat/context, reconnect durability, ZDR, and approval-scope scenarios in
addition to the existing Capability, scheduled-step, approval-continuation,
`tickets.create`, Application delivery, configuration, and artifact-contract
lanes. This closes a source-test drift defect; it does not create live
candidate, provider/ZDR, owner-effect, or rollback evidence.

- `scripts/tests/capability-health-proof.sh` passed the generic-health
  allowlist, stale/unhealthy fail-closed policy, and real-Postgres no-write
  regressions. The capability ledger remains `source_only`.
- `scripts/tests/scheduled-step-proof.sh` passed the workflow retry/unknown,
  activity handoff, Execution Core service-owned lane, and Session receipt
  ledger checks. The scheduled lane remains disabled.
- Orchestrator's scheduled activity now treats a known Execution Core
  `failed` receipt as terminal and non-retryable, preserving its receipt/error
  code and preventing the workflow from advancing to a later step. The
  focused activity/workflow suites pass; this is a source safety invariant,
  not live provider or candidate evidence.
- `scripts/tests/approval-continuation-proof.sh` passed against a disposable
  Postgres with the real Session Core migrations. A second worker reclaimed an
  expired approval-delivery lease and received the original immutable
  continuation start receipt, proving no duplicate start receipt. This is
  integration evidence only; deployed worker, exact `tickets.create` owner
  receipt, provider/ZDR, unknown-outcome reconciliation, and candidate proof
  remain open.
- The focused notification-core suite and
  `scripts/tests/application-delivery-proof.sh` passed after wiring the
  queue/worker behind `NOTIFICATION_DELIVERY_WORKER_ENABLED=false` by default.
  Submission now atomically records a content-free feed-projection obligation;
  callback acknowledgement carries the receipt into that obligation, and the
  lease-fenced projector was exercised against disposable Postgres, including
  callback-before-projection ordering, two-worker lease fencing, idempotent
  feed replay, and a delivered feed row. Provider/ZDR, real multi-replica
  replay, and candidate evidence remain open.
- `scripts/coresystem-conformance.sh --json` remains `blocked`: candidate and
  rollback artifacts are absent, the worktree is dirty, and live promotion
  evidence is absent. The Model dev runtime now has the non-secret
  `CONTROL_USER_CORE_URL=http://user-service:3012` reference wired into the
  active Capability Core and Orchestrator containers; the scheduled preflight
  therefore reports only the three missing schedule service-token bindings.
  The read-only cross-plane preflight still reports missing Conversation
  owner-effect and Model execution bindings, open provider/ZDR, approval, and
  candidate evidence. A subsequent read-only preflight observed
  the Imports API healthy again; the remaining blockers are configuration and
  evidence, not a proven Imports API defect.
- The authenticated Microsoft browser session reached the server-composed
  Verevon Space shell. A fresh account-selection flow reached the AQUATIQ AS
  room as the owner, showed five projected conversations, and opened Activity;
  Activity explicitly reports that owner-plane receipts are not yet correlated.
  This is authenticated UI/session evidence only, not a successful cross-plane
  effect, revocation, or delivery proof.

## 2026-08-17 continuation verification

This rerun is source/disposable evidence only. It did not create, read, inject,
rotate, or expose any credential, and it did not change the Model allowlist.

- `go test ./...` passed for Capability Core and Orchestrator Core.
- `cargo test -p execution-core scheduled_step_decision --no-fail-fast` passed;
  `cargo test -p session-core scheduled_step --no-fail-fast` passed with only
  the expected DATABASE_URL-gated real-Postgres case ignored.
- `scripts/tests/capability-health-proof.sh` passed the generic allowlist,
  legitimate-tenant and forged-global no-write cases for both an ordinary
  runtime row and the reserved `cap.tool.ticket.create` global row,
  stale/unhealthy policy, and the real-Postgres negative checks.
- `scripts/tests/scheduled-step-proof.sh` passed the deterministic workflow,
  activity handoff, Execution Core contract, and Session receipt proofs.
- `scripts/tests/tickets-create-proof.sh` passed the owner contract,
  reservation fence, two-Postgres interleaving, and unknown/receipt paths.
- `scripts/tests/approval-continuation-proof.sh`,
  `scripts/tests/application-delivery-proof.sh`, and both scheduled/ticket
  configuration-contract tests passed. These remain integration/source proofs;
  provider, deployed worker, HA, candidate, and rollback evidence are still
  open.
- `scripts/tests/release-contract-test.sh` passed the signed-artifact fixture
  suite, including manifest integrity, deployability, rollback shape, and
  compatibility checks. This verifies the release contract implementation only;
  no candidate or rollback artifact was created or promoted.
- The Microsoft-authenticated V3 session reached the server-composed AQUATIQ AS
  Space as owner, showed five conversation projections, and rendered the
  Activity and Agent surfaces. Activity has no correlated owner receipt yet;
  the Agent/chat surface continues to report that no model is attested for
  zero-data-retention temporary chat. This is UI/session evidence only.

### Fresh aggregate rerun (2026-08-17 00:40 UTC)

`scripts/tests/coresystem-qm-proof.sh` was rerun sequentially against the
current worktree and exited `0`. The harness passed capability-health,
scheduled-step, approval-continuation contract and disposable-Postgres checks,
`tickets.create`, scheduled/ticket configuration contracts, and Application
delivery. The harness still explicitly reports `source_only`; this rerun does
not promote a capability or substitute for a signed candidate, rollback,
provider/ZDR, deployed approval-continuation, callback/HA, or live owner-effect
observation.

The scheduled-run handoff packages also passed their direct regression suites:
Capability Core `internal/taskexec`, `internal/cron`, and `cmd/...`, plus
Orchestrator Core `internal/orchestration`, `cmd/activities`, and
`cmd/workflows`. These tests cover dispatcher construction order, strict
non-secret Temporal input, canonical template-digest binding, and the
dedicated scheduled-run Session preparation path; they remain source/test
evidence until the distinct dev Control credentials are provisioned and a
deployed effect is observed.

The active dev containers also logged successful Execution Core probes for
`cap.command.sandbox` and `cap.command.shell`. This is runtime corroboration
for the generic reporter only; the owner-action reporter and all effectful
capabilities remain fail-closed until their distinct Control bindings and
release evidence are present.

The authenticated Microsoft V3 Space was re-opened read-only during this
rerun. AQUATIQ AS showed five conversation projections, two Control-bound
agents (Driftsassistent and Statusagent), and no active work in the Space
pulse. Activity showed one failed and several completed/ongoing projections,
while the UI explicitly states that owner-plane run receipts are not yet
correlated. The Agent view showed the agents and their channel/binding policy,
but no Model capability availability or owner effect. This is UI evidence only;
no message, agent, connector, or effectful action was submitted.

### Second sequential rerun (current continuation)

The aggregate harness was rerun again after this checkpoint and passed every
source/disposable lane: R-2 capability authorization, scheduled-step,
approval-continuation, `tickets.create`, scheduled/ticket configuration, and
Application delivery. Its final output still reports `source_only` and
explicitly leaves live candidate, provider/ZDR, deployed approval, HA callback,
and rollback evidence open. The authenticated V3 Agent view remained read-only:
two agents were visible, the Space pulse reported no active work, and no Model
capability or owner-effect receipt was exposed.

## 2026-08-17 objective continuation

The four requested objective lanes were rerun sequentially after the previous
parallel proof attempt (which briefly contended for the disposable Rust
artifact lock):

- `go test ./...` passed for Capability Core and Orchestrator Core.
- `scripts/tests/capability-health-proof.sh` passed the generic allowlist,
  tenant/forged-global no-write cases, stale/unhealthy policy, and real-
  Postgres global-row protections. The Model capability ledger remains
  `source_only`.
- `scripts/tests/tickets-create-proof.sh` passed the Execution/Control/
  Conversation contracts, reservation fence, two-Postgres owner interleaving,
  and durable receipt/`unknown_outcome` paths.
- `scripts/tests/scheduled-step-proof.sh` passed the Temporal retry/unknown
  workflow, activity handoff, Execution Core scheduled lane, and Session
  claim/receipt ledger.
- `scripts/tests/coresystem-qm-proof.sh` passed the aggregate Capability,
  scheduled-step, approval-continuation, tickets, configuration, and
  Application delivery source/disposable suite.

These results do not clear the release gates. `scripts/coresystem-conformance.sh
--json` still reports `blocked` with dirty source, absent candidate and rollback
artifacts, `source_only` promotion state, missing scheduled Control bindings,
missing Conversation/Model owner-effect bindings, and open provider/ZDR,
approval-continuation, callback/HA, and candidate evidence.

The Microsoft-authenticated V3 read-only Space observation reached AQUATIQ AS as
owner, showed five conversations, two Control-bound agents, and Activity rows
for the current thread/run projection. No owner-effect receipt or Model
availability was observed; no write action or credential change was performed.

The release-artifact dry-run also rendered the complete candidate image-build
plan for the current revision without building, signing, importing, or changing
the Docker stack. A real build remains correctly refused until a clean isolated
revision, managed signing references, an accepted distinct rollback artifact,
and operator-owned runtime evidence are supplied.

The non-secret scheduled-runtime URL was then added to the internal dev
environment and only Capability Core and Orchestrator were recreated. No
credential value was changed. Both containers returned healthy; the remaining
scheduled blockers are the Capability Core reauthorization token and the two
Orchestrator schedule/step tokens.

The read-only conformance report remains intentionally `blocked`: the current
dev containers are healthy, but scheduled Control bindings, Conversation owner
signer/reservation bindings, Model execution bindings, provider/ZDR evidence,
approval-continuation deployment evidence, candidate, and rollback artifacts
are absent. Keep `cap.tool.ticket.create` and all other unproven capabilities
`source_only` until those gates are independently observed.

### Current Control principal-registration check

As a secret-free diagnostic, the active Auth Core registry was inspected for
principal names, audiences, and scopes only; credential values were not read or
printed. `capability-core` and `orchestrator-core` are registered, but neither
has the required `spaces:schedule:reauthorize`, `spaces:schedule:execute`, or
`spaces:schedule:step` scope in the running registry. No complete Conversation
owner-decision/reservation or Model owner-action binding is registered in the
active containers. This explains the three scheduled-token and five
owner-action configuration blockers reported by preflight. The correct next
step is an operator-managed, distinct dev registration; reusing an existing
service credential would violate the authority contract.

The cross-plane preflight now checks these exact User Core and Auth Core
principal/audience/scope tuples in addition to environment-key presence. It
prints only `present`, `missing`, or `malformed` state and turns a missing
registration into a named blocker, so a copied token or broad audience cannot
look like a configured authority.

- Runtime logs provide corroborating fail-closed evidence rather than a
  readiness claim: Capability Core logged that it could not obtain a Session
  Core credential, Conversation Core's ticket outbox could not claim work, and
  Orchestrator logged Temporal poll/keepalive timeouts. These observations are
  consistent with the named preflight blockers; they do not authorize a
  fallback credential or a capability promotion.

## 2026-08-16 owner-action and scheduled-run follow-up

This checkpoint is source/test evidence only. It does not change any Model
capability availability or promote a release.

- **Owner-effect reservation fence:** Control now stores a content-free,
  revision-fenced reservation and Conversation Core's private ticket path now
  resolves an exact active owner-grant ID, calls Control reserve then commit
  with the verified signed decision, and only afterward locks/rechecks that
  same local grant while atomically writing ticket, audit, outbox, and owner
  receipt. The reservation credential is distinct from the read-only
  current-authority credential; the signed decision and both credentials are
  direct-hop only. Focused tests prove commit before local effect and zero
  write on a reservation/commit denial. `go test ./...` passed in both Control
  User Core and Conversation Core. The current dev configuration deliberately
  lacks the signer/verifier and dedicated service credentials, so the private
  route remains unavailable. Required next proof: disposable multi-database
  reserve/commit/revoke interleavings plus crash/timeout reconciliation; do
  not claim the distributed protocol complete before those pass.
- A throwaway-Postgres Control repository run now passes the real migration
  path: exact reservation replay returns the original receipt, commit replay is
  idempotent, and a revision change cancels an uncommitted reservation while
  preserving a committed one. This proves the Control ledger/fence only; the
  cross-database owner interleaving, crash reconciliation, authenticated
  transport, and deployed signer/verifier remain open.
- **Scheduled run:** source tests pass for Capability Core's reauthorization,
  Session preparation, and Orchestrator's effect-time decision path. The
  running dev containers are healthy but log that Control endpoint/credential/
  decision-key configuration is absent, so scheduled work fails closed. No
  credential was read, created, substituted, or rotated. Independently, the
  normal scheduled workflow reaches Execution Core with no user-scoped
  execution identity, which Execution Core correctly rejects; new
  `ScheduledRunSupervision` histories now use a replay-versioned,
  service-owned scheduled-step activity instead of that user-bound path. The
  dedicated activity remains fail-closed until Session claim/receipt and the
  bounded runtime adapter are deployed.
- **Scheduled-step contract:** the additive Execution Core RPC now has the
  bounded service-owned lane in source. It independently verifies the Control
  `model.schedule.step` envelope, mints a separate `inference-core` service
  token, claims the exact run/step in Session Core, performs one tool-free
  dedicated `GetScheduledStepContext` read for the exact service-owned goal,
  performs one tool-free inference turn, and records a metadata-only
  `completed`, `failed`, or explicit `unknown_outcome` receipt. Duplicate
  claims do not open a second effect attempt; inference timeout/transport
  ambiguity is never reported as success. Focused verifier, Auth Core token,
  Execution Core compile, Session Core claim-shape, and Orchestrator tests pass.
  The lane remains fail-closed when its verifier, service scopes, provider, or
  Session credentials are absent, and no Model capability is enabled. A real
  Session/Postgres claim/receipt run, crash-before/after-provider proof,
  provider/ZDR attestation, and deployed candidate observation remain open.
- Control's per-step `model.schedule.step` issuer and Orchestrator verifier are
  present in source with a distinct service scope and exact digest binding;
  Orchestrator mints the Execution Core audience only for
  `ExecuteScheduledStep`, never for user-bound `ExecuteStep`/`RunAgent`. There
  is no deployed or provider/effect evidence.
- The Orchestrator activity regression now forwards a fresh, verified Control
  step decision with the exact run/thread/Space/schedule/fire/template/step and
  idempotency tuple to the dedicated Execution Core RPC, and never places that
  bearer in workflow input. `go test ./cmd/activities` and the full
  `go test ./...` suite pass. This proves the source handoff only; it does not
  replace disposable-Postgres claim/receipt, Temporal crash, provider/ZDR, or
  candidate observation evidence.
- The scheduled-step workflow now treats an Execution Core
  `unknown_outcome` response as a non-retryable Temporal application error.
  The durable Session receipt remains the reconciliation authority; the
  workflow cannot continue to a later step or blindly retry the uncertain
  provider call. Activity and workflow regressions pass, including the
  `RUN_FAILED`/unknown-reason path. This is source/test evidence only and does
  not enable the scheduled capability.
- A Temporal workflow regression now injects a transient worker/transport
  failure after the first scheduled-step dispatch and verifies that the
  retry reuses the identical `{step_id, step_index, idempotency_key,
  template_digest, policy_digest}` tuple before completing. This proves the
  retry boundary is deterministic; it does not prove that a real provider
  accepted the first request or replace the required Session receipt and
  crash-after-submit evidence.
- Session Core's scheduled-step claim/receipt ledger now has a real-migration
  Postgres proof: duplicate `{run_id, step_id}` claims return the original
  receipt, an ambiguous transport is terminalized as `unknown_outcome`, an
  identical terminal replay is idempotent, and a later `completed` overwrite is
  rejected. The focused ignored test passed with a throwaway Postgres using
  `DATABASE_URL=... cargo test -p session-core
  scheduled_step_claim_and_unknown_receipt_are_idempotent_against_real_pg --
  --ignored`. This proves the Session ledger only; Temporal/provider crash
  recovery, Auth Core scopes, ZDR attestation, and deployed candidate evidence
  remain open.
- The same Session claim/receipt proof is reproducible through
  `scripts/tests/scheduled-step-receipt-postgres-test.sh`, which uses only a
  disposable Postgres container and passed without changing the running dev
  stack or any service credential.
- `scripts/tests/scheduled-step-proof.sh` now composes the scheduled-step
  evidence: Temporal workflow tests prove prepared-thread routing, deterministic
  retry, and non-retryable `unknown_outcome`; Orchestrator activity tests prove
  exact Control-decision forwarding and receipt mismatch rejection; Execution
  Core scheduled contract tests pass; and the disposable-Postgres Session
  receipt proof passes. This remains source/disposable-dependency evidence,
  not deployed Temporal/provider, Auth Core, ZDR, candidate, or rollback proof.
- **Model ticket tool:** source checks cover the unavailable seed, owner-only
  health attestation, server-resolved run-bound view, reserved tool names, and
  approval-continuation exclusion. The dedicated approval worker now binds the
  frozen descriptor's schema/payload/idempotency/owner tuple, obtains a fresh
  Control decision, and calls a private Conversation Core owner adapter. Its
  ambiguous-response path reconciles by idempotency before retry; an unresolved
  lookup is recorded as `unknown_outcome`. Focused Rust worker/client tests and
  Conversation Core reconciliation-route tests pass. The real leased Postgres
  worker path, deployed signer/verifier/credentials, distributed crash/revoke
  proof, and Space Activity projection remain open. This is evidence that the
  action remains unavailable, not permission to enable it.
- The frozen approval continuation now carries `owner_user_id` from the
  Session-owned descriptor through Execution Core's ticket adapter and into the
  private Conversation Core request. A wire-level regression,
  `approved_continuation_carries_the_frozen_owner_to_the_owner_boundary`,
  passed in Execution Core. This closes descriptor ownership propagation at
  source level; it does not replace the deployed Control decision, owner grant,
  authenticated transport, provider receipt, or candidate evidence gates.
- A throwaway Postgres run now passes Conversation Core's live owner-grant
  lifecycle (`TestLiveOwnerGrantCreateRevokeLifecycle`) against the actual
  migrations and router: grant, idempotent grant replay, owner transaction with
  a complete committed-reservation evidence tuple, revoke, and post-revoke
  denial. This proves the local grant/transaction boundary only; the Control ↔
  Conversation cross-database reservation interleaving, crash reconciliation,
  deployed transport, and candidate observation remain open.
- **Owner-side ticket intent:** Conversation Core migration `031` is now wired
  into the private effect route. Before Control reserve/commit it persists only
  immutable `{operation_id, actor, conversation, request_sha256,
  schema_hash, payload_digest, decision_ref, grant_ref}` facts in
  `pending_control_commit`; the committed reservation is then bound as
  `reserved`, and the final owner transaction promotes that same row to
  `completed`. Deterministic owner denial becomes `cancelled`; an ambiguous
  owner failure becomes terminal `unknown` and is exposed by reconciliation
  without ticket content. Unit/full Conversation Core tests pass, and
  `TestLiveAgentTicketOperationIntentUnknownLifecycle` passed against the real
  migrations in throwaway Postgres, including exact-payload conflict and
  protection against overwriting `unknown` with late cancellation. This closes
  the local owner-intent gap only; distributed Control/Conversation crash
  interleavings, authenticated transport, deployed credentials, provider
  receipt, and candidate proof remain open. The Model ticket capability stays
  unavailable.
- **Governed ticket proof harness:** `scripts/tests/tickets-create-proof.sh`
  now composes Execution Core ticket-contract tests, Control authority and
  reservation tests, Conversation Core owner-contract tests, and disposable
  Postgres owner-interleaving proofs. It passed exact unknown-outcome replay,
  owner-grant revoke/effect races, Control cancellation of uncommitted
  reservations, committed-row preservation, and idempotent receipt paths
  against the real migrations. The Conversation Core proof now also holds the
  owner commit boundary, revokes the real grant before release, and proves a
  `cancelled` operation with zero ticket rows. The cross-database variant
  uses real Control and Conversation Postgres instances: the Control
  reservation is committed, its authority revision is advanced, and the
  separate Conversation grant is revoked before the owner transaction is
  released; the committed Control receipt remains committed while no ticket
  is written. This is source/disposable-dependency evidence only; a deployed
  authenticated transport/crash journey, signer/verifier configuration,
  provider receipt, and immutable candidate/rollback evidence remain open.
  The Model ticket capability is still unavailable.
- **Authenticated reservation transport contract:** the Conversation Core →
  Control reservation client now has a real `httptest.NewTLSServer` proof. It
  verifies the HTTPS certificate path, the dedicated `conversation-core`
  service identity, the separate reservation token, and the exact signed
  decision/commitment request body. Plaintext is rejected except for an
  explicitly enabled IP-loopback development endpoint. This is source/test
  evidence only; deployed TLS/mTLS configuration and live cross-plane
  observation remain required.
- **Governed ticket configuration contract:** `scripts/tests/tickets-create-config-contract-test.sh`
  verifies that the five exact Execution Core adapter bindings are declared in
  Model Plane Compose and `.env.example` with empty defaults and no literal
  values. This makes the source wiring auditable without creating or rotating
  credentials; the runtime adapter remains disabled until the existing
  deployment-owned references are supplied and independently observed.
- **Scheduled-step configuration contract:** `scripts/tests/scheduled-step-config-contract-test.sh`
  verifies the six empty-default scheduled-run bindings and the exact Auth Core
  scope names requested by Capability Core, Orchestrator, Session Core, and
  Execution Core. The live preflight still reports missing Control endpoints /
  tokens and absent scope registration, so the scheduled lane remains
  fail-closed.
- **Cross-plane owner-path preflight:** `scripts/coresystem-cross-plane-preflight.sh`
  now checks the five Execution Core `tickets.create` adapter bindings in
  addition to Conversation Core's decision/reservation inputs. The current
  healthy-but-blocked report names those missing inputs without printing any
  value, preventing topology health from being mistaken for owner readiness.
- **Dev reference audit (2026-08-16):** the active Model Plane and Conversation
  Core `.env` files and their local backups contain the existing Space decision
  key references, but none of the exact scheduled-run, owner-reservation, or
  ticket adapter bindings required by the preflight. No credential was copied,
  generated, or rotated; the missing references are therefore a real external
  deployment gate rather than an undiscovered local value.
- **R-2 capability health proof harness:** `scripts/tests/capability-health-proof.sh`
  passes the generic allowlist/tenant-global no-write checks, stale/unhealthy
  policy checks, and signed real-Postgres regressions. This is repeatable
  source/integration evidence; it does not substitute for live stale/outage
  observations against a signed candidate.
- **Generic reporter identity fence:** the Capability Core HTTP suite now
  proves at both the outer HTTP authorizer and handler boundary that a service
  with `capability:health:global:write` but the wrong signed service identity
  cannot enter or persist even `cap.command.sandbox`; the recorder remains
  untouched. The exact `service:execution-core` identity is still restricted to
  its explicit two-capability probe allowlist.
- **Capability health boundary:** Capability Core now has an end-to-end signed
  middleware regression for the tenant-to-global spoof: a `HealthWriteScope`
  credential presenting `org_id=global` receives 403 and the global registry
  row is not written. The real-Postgres integration variant uses a genuine
  tenant claim (`org_id=tenant-a`), reaches the tenant lookup, returns 404 for
  the global row, and verifies that its state/timestamp and audit count remain
  unchanged. The generic global reporter is also allowlisted to the
  two execution probes it can measure and cannot attest `cap.tool.ticket.create`.
  The new build-tagged integration test runs the same signed request against a
  throwaway Postgres with real migrations and verifies both the row state and
  the absence of a `global_availability_attested` audit row. This is source/test
  evidence only; stale/outage observations required by R-2 remain absent. On
  2026-08-16 the dev stack was rebuilt without changing credentials: Auth Core
  issued the exact signed `service:execution-core` identity, both measured
  probe attestations were accepted, and live reads returned `available` for
  `cap.command.sandbox` and `approval_required` for `cap.command.shell` (the
  expected high-risk normalization). The pre-fix 403 was an unprefixed
  allowlist comparison; the allowlist now requires the exact signed identity
  and denies the unprefixed form in regression tests. Development also treats
  an intentionally empty capability-decision public key as unverified
  (production compose still requires it), preventing a dev crash loop. This
  closes the dev-stack reporter observation only; stale/heartbeat expiry,
  outage recovery, provider/ZDR, candidate, and rollback evidence remain open.
  A second disposable-Postgres integration case now exercises the reserved
  `cap.tool.ticket.create` row itself: both a tenant `HealthWriteScope` token
  and the exact generic `service:execution-core` global-health token receive
  403, with no state/timestamp or audit-row change. This closes the specific
  generic-health-to-owner-action write path; it does not create owner-action
  readiness or enable the Model capability.
- **Conversation Core owner-action reporter:** Conversation Core now has a
  source-wired, fail-closed reporter for the dedicated `tickets.create`
  capability lane. With complete Control-bound execution configuration and a
  separate service principal, it mints a short-lived `aud=capability-core`
  token scoped only to capability read plus owner-action health write, reads
  the current catalog version, and posts content-free readiness metadata. Any
  mint/read/write failure leaves the capability unavailable. The Conversation
  Core full Go suite and reporter/config tests pass; no deployment credential
  was created, read, rotated, or injected, so live observation remains open.

- **Application delivery attempt contract:** notification-core now has a
  content-free `notification_delivery_attempts` ledger with lease-fenced
  `pending`, `claimed`, `sent_unconfirmed`, `acknowledged`, `failed`, and
  `unknown` states. The opt-in worker marks provider transport ambiguity as
  `unknown`, refuses to persist ZDR payloads for asynchronous delivery, and
  cannot finalize a claim from a stale worker. The callback verifier requires
  a fresh HMAC envelope and an external replay-store claim. The focused
  `apps/Model Plane/scripts/tests/application-delivery-proof.sh` passes the
  state-machine, migration-content, queue, worker, callback, and migration
  contract tests. The server now wires the queue and lease-fenced worker only
  when `NOTIFICATION_DELIVERY_WORKER_ENABLED=true`; configuration rejects that
  flag unless Novu mode and a callback verifier are both present. This is still
  source/test evidence only: durable feed reconciliation, HA replay,
  provider/ZDR attestation, and immutable candidate observation remain open.
  The callback route and worker are separately config-gated and fail-closed
  while their dev references are empty. No provider was enabled and no
  credential was read, created, or rotated.

## Current register

| Claim | Source | Test/integration | Candidate | Rollback | Current disposition |
| --- | --- | --- | --- | --- | --- |
| Release artifact v3 captures immutable inputs and partitions secrets | present | `release-contract-test.sh`, `release-runtime-partition-test.sh`, and `release-input-security-test.sh` pass | absent | absent | Gate exists; operator signing and accepted artifacts are still required |
| ZDR is issuer-monotonic and local durable boundaries fail closed | present | Rust gateway/inference/session tests pass | absent | absent | Safe failure is proven; no usable provider route is attested |
| One provider deployment is independently ZDR-attested | configuration gate only | no external provider attestation | absent | absent | Release blocker |
| Approval delivery resumes the exact approved action | worker, encrypted descriptor table, and descriptor protocol present | focused worker tests pass; disposable-Postgres lease-expiry/reclaim test passes with duplicate start receipt suppression | absent | absent | Local crash/recovery is proven; immutable-candidate and deployed effect observation remain required |
| Grounded answer uses authorized Data Plane evidence and citations | source path present | unit coverage present; customer journey E2E absent | absent | absent | Integration proof required |
| Normal chat persists the canonical conversation and resumes after disconnect/restart | message store, identity-scoped SSE buffer, and thread replay query present | gateway Redis restart/cross-device harness and disposable-Postgres thread replay test pass; full deployed chat journey absent | absent | absent | Local durability parity is proven; staging observation and canonical BFF ownership remain required |
| A user can durably erase their own thread and all Session Core thread/run evidence | owner-bound single/bulk Session Core delete RPCs and Model Gateway DELETE routes present | session-core test build passes; real-Postgres owner/scope/cleanup test is available but requires `DATABASE_URL` | absent | absent | Session Core cleanup is local-source complete; Letta semantic-memory erasure is a separate DSAR workflow and must not be implied by this receipt |
| Learning closes the run → feedback → reviewed candidate loop | producer/consumer paths and startup wiring present | durable-layer, composition, focused tests, and real-NATS trigger harness pass; live session-core/inference-core/LLM persistence absent | absent | absent | Integration proof required |
| Capability policy/health state is authoritative before dispatch | source path present | negative policy, health, and decision-correlation tests pass; live dev reporter observed both execution probes; Conversation Core owner-action reporter is source-wired but not deployed/observed | absent | absent | Dev health proof is present (`sandbox=available`, `shell=approval_required`); dedicated owner-reporter stale/outage, provider, candidate, and rollback evidence remain required |
| Application notification delivery is durable and receipt-reconcilable | source state machine, content-free attempt migration, opt-in worker wiring, config-gated callback route/verifier present | `scripts/tests/application-delivery-proof.sh` passes focused state, migration, queue, worker, callback, HTTP fail-closed, and worker-configuration tests | absent | absent | Durable feed reconciliation, HA replay, provider/ZDR attestation, and candidate observation remain open |
| Per-call capability allow decisions are bound to the exact dispatch tuple | Ed25519 JWS signer/verifier and execution dispatch gate present | Go signer tests plus Rust valid/tampered/expired proof tests pass | absent | absent | Deployment keys, rebuilt images, and a live allow/verify observation remain required |
| Service-owned scheduled step is bounded by Control → Session → Execution → Inference | `model.schedule.step` verifier, Session claim/receipt ledger, dedicated service token, tool-free runtime, timeout/unknown mapping, and idempotent receipt path present | `cargo check -p execution-core --tests`; focused scheduled-decision, scheduled-inference-auth, session-terminal-auth, Session scheduled-step, Orchestrator, and real-migration Session claim/receipt tests pass | absent | absent | Runtime remains fail-closed until Temporal/provider crash proof, explicit Auth Core scopes, provider/ZDR attestation, and deployed observation are recorded |
| Org safety policy controls PII redaction before provider dispatch | capability-core safety projection read in unary and SSE gateway paths; unknown policy state redacts fail-closed | `cargo test -p model-gateway --lib moderation::tests --no-fail-fast` (9 passed) | absent | absent | Source/test proof only; staging must observe an enabled and disabled tenant policy against a provider-bound prompt |
| JetStream topology survives a clean broker bootstrap | NATS provisioner owns required Model Plane streams/consumers | provisioner unit tests pass; live provisioner created topology and session-core consumers recovered | absent | absent | Staging must exercise restart/reconnect and bind the provisioner image to a candidate |
| Quality claims separate model capability from the integrated harness and retain bounded telemetry | versioned digest-only eval evidence contract present | contract test rejects missing/changed evidence, non-independent verifiers, and limit-reached false successes | absent | absent | Run both lanes against a private, rotating suite and bind their validated records to the signed candidate |

## Evidence captured in this checkout

- `scripts/tests/coresystem-cross-plane-preflight-test.sh` (`CoreSystem
  cross-plane preflight contracts: ok`) validates the read-only cross-plane
  health/key/evidence check. It covers the active Control, Application, Data,
  Ingestion, Frontend, Temporal, and NATS containers, checks only the presence
  of named authority/owner-effect inputs, and reports provider/ZDR, approval,
  and candidate evidence as open without printing values.
- `scripts/tests/capability-promotion-ledger-test.sh` (`capability promotion
  ledger contracts: ok`) validates that the ledger covers every seeded
  capability migration ID, permits only the documented promotion states, and
  keeps `cap.tool.ticket.create` in `source_only` until owner and release
  evidence exist. The ledger is deliberately not a promotion mechanism by
  itself; candidate-bound evidence and an operator decision are still required.
- `scripts/tests/coresystem-conformance-test.sh` (`CoreSystem conformance
  contracts: ok`) validates the root conformance report's stable human/JSON
  shape, secret-redaction, composed Model plus cross-plane preflights, and
  fail-closed exit semantics. The current report is intentionally blocked by
  dirty source, absent candidate/rollback artifacts, scheduled Control
  bindings, missing Conversation owner configuration, the `source_only`
  capability ledger, and open provider/ZDR, approval, and candidate evidence;
  it is not a release-ready claim.
- `scripts/tests/release-contract-test.sh` (`release contracts: ok`)
- `scripts/tests/release-runtime-partition-test.sh` now also covers the three
  scheduled Control service-token keys used by Capability and Orchestrator;
  the audited secret partition and full release-contract harness both pass.
- `scripts/tests/dev-runtime-preflight.sh` reports only container/config state
  and an expiry, never credential values. Against the current dev stack it
  correctly exits blocked for the empty Control URL and scheduled service-token
  bindings, so scheduled effects remain fail-closed.
- `scripts/tests/release-input-security-test.sh`
- release secret-partition audit now includes the newly surfaced capability,
  application, cost, deep-research, and provider-attestation credentials;
  unreviewed credential-shaped Compose keys remain a hard failure
- `scripts/verify-durable-layer.sh`
- `cargo test -p model-gateway --lib` (781 passed, 0 failed); the focused
  post-change `stream_buffer` suite passed its non-ignored tests
- `scripts/tests/customer-proof-e2e-test.sh` (four scenarios passed using the
  isolated verification target)
- `scripts/tests/approval-recovery-postgres-test.sh` (lease expiry/reclaim and
  idempotent start receipt passed against disposable PostgreSQL)
- `scripts/tests/chat-thread-replay-postgres-test.sh` (thread-owned message
  replay passed against disposable PostgreSQL)
- `scripts/tests/chat-redis-durability-test.sh` (completed stream survived a
  Redis container restart; same-user resume passed and cross-tenant replay was
  rejected)
- `scripts/tests/learning-nats-trigger-test.sh` (RUN_COMPLETED subscription,
  delivery, review, and shutdown passed against a real NATS server; session and
  inference boundaries were fakes)
- `go test ./internal/server ./internal/policy` from
  `go/services/capability-core` (signed decision evidence contract passed)
- `cargo test -p execution-core capability_policy --no-fail-fast` (five policy
  tests passed, including valid, tampered, and expired Ed25519 evidence)
- `cargo test -p model-gateway --lib cancel_registry` (four tests passed,
  including cross-tenant and cross-user cancellation rejection)
- `cargo test -p model-gateway --lib skills::tests` (11 skill-cache and
  reconciliation tests passed)
- `cargo test -p model-gateway --lib retrieval::tests` (17 retrieval/graph/
  authorization tests passed, including packed pinned-fact ordering) plus the
  focused context-pack regression (one test passed)
- `cargo test -p model-gateway --lib confidence::tests` (10 confidence tests
  passed, including Data Plane low-confidence suppression)
- `cargo test -p model-gateway --lib moderation::tests --no-fail-fast` (9
  tests passed, including capability-core policy enforcement and fail-closed
  missing/malformed-policy behaviour)
- `cargo check -p model-gateway --lib` (resolved agentic permission posture is
  forwarded into RunAgentRequest.mode)
- `go test ./...` from `go/services/nats-provisioner` (stream/consumer topology
  reconciliation passed)
- Live NATS provisioner run logged `Model Plane JetStream topology ready`,
  followed by session-core `NATS consumer ready` and `orchestration NATS bridge
  ready` after the broker had initially reported missing streams
- `cargo test --manifest-path rust/Cargo.toml -p session-core approval_delivery
  --no-fail-fast` (18 tests passed)
- `go test ./internal/server ./internal/policy` from
  `go/services/capability-core` (passed)
- `cargo test -p execution-core health_attest` (10 health-attestation tests
  passed)
- 2026-08-16 dev-stack probe: rebuilt Capability Core and Execution Core with
  existing credentials; authenticated `service:execution-core` health writes
  succeeded for both measured capabilities, and live reads returned the
  expected availability states. No credential was created or rotated.
- `go test ./internal/learning/... ./internal/llmreviewer/... \
  ./internal/skillsink/... ./internal/sessionreview/...` from
  `go/services/capability-core` (passed)
- `scripts/tests/eval-evidence-contract-test.sh` (digest-only, ZDR-safe eval
  evidence contract passed; validates the model-only/integrated-harness lane,
  harness fingerprints, independent verifier identity, and bounded action,
  latency, token, cost, and recovery telemetry)
- `scripts/tests/coresystem-qm-proof.sh` (aggregate source/disposable proof
  passed for Capability Core R-2 authorization, scheduled-step retry/claim/
  receipt behavior, governed `tickets.create` owner interleavings, and the
  empty-default configuration contracts). This is a reproducible subgraph
  result only; it does not create candidate, provider/ZDR, approval-delivery,
  or rollback evidence, and the capability ledger remains `source_only`.
- `docs/DEV_AUTHORITY_HANDOFF_MATRIX.md` records the non-secret operator handoff
  for the scheduled, `tickets.create`, Space, delivery, and release lanes. It
  deliberately contains names/scopes/key references only; the current running
  stack still reports the required runtime inputs as absent.

The current checkout also contains pre-existing uncommitted gateway changes.
Those files are intentionally listed as working-tree state, not candidate
evidence. A candidate must be built from a clean, isolated revision and must
bind its image and configuration digests to the signed root manifest.

## Required promotion evidence

Before a release claim can move to `candidate`:

1. Build from a clean revision with managed signing and verification keys.
2. Verify the candidate root, external runtime environment, migrations, and
   image lock without rebuilding.
3. Restore the images in a non-production environment and run the authenticated
   negative and positive boundary matrix.
4. Rehearse the separate rollback artifact with a distinct source revision and
   image payload.
5. Attach the four customer-proof scenarios: grounded chat, durable chat,
   effectful approval recovery, and learning review.
6. Record observation windows, thresholds, operator sign-off, and the exact
   artifact/rollback digests.

Until those rows are populated with the required evidence classes, the Model
Plane remains source-verified or locally integrated, not production-ready.

## External quality-grounding implications

The external references supplied for this review sharpen what the remaining
quality gates must measure:

- [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent) demonstrates
  that durable goals, child-agent state, schedules, compaction, bounded
  continuation, and recovery belong to the runtime owner—not to a UI cache.
  Its host-permission execution model is not an acceptable security boundary
  for Model Plane; Quarry-v2 and capability policy must remain authoritative.
- [PokeAgent (arXiv:2603.15563)](https://arxiv.org/abs/2603.15563) separates
  model capability from the harness and reports milestones, action count,
  wall-clock latency, tokens, and cost. Model Plane eval manifests should
  therefore fingerprint model/provider, prompt, tools, memory/retrieval,
  feedback/verifier, policy, and budgets, with model-only and integrated-harness
  controls.
- [ARC-AGI-3](https://arcprize.org/leaderboard) reinforces private rotating
  evaluation tasks, replayable action traces, independent completion checks,
  and contamination/ZDR controls. A local regression score or a successful
  harness run is not evidence of generalization or release readiness.

These are evaluation requirements, not a license to copy benchmark-specific
tools or to train on customer traces. The local quality milestone is now
implemented as `scripts/eval-evidence.sh`: it accepts only digest-only suite
and harness identities, requires a distinct independent-verifier identity, and
checks per-attempt action, latency, token, cost, and recovery bounds. It does
not turn a local fixture into a quality result. The remaining milestone is to
run both lanes against a private rotating suite and attach the validated
evidence records to a signed candidate.

## Recommended next-step order

1. **Create the external candidate gate.** Build from a clean revision, sign
   the root/image/config manifest, attach an independently verified provider
   ZDR route, and retain a distinct signed rollback artifact. The dirty
   working tree and local fixture tests cannot satisfy this gate.
2. **Prove grounded chat in staging.** The direct gateway and tool retrieval
   paths now request Data Plane's token-budgeted `context_pack` (including
   pinned facts) and retain only local citation/injection framing. The live
   positive proof is still blocked: the running Data Plane rejected the
   verified bearer with `not_member` despite the Control Plane databases
   containing an owner membership. Resolve that authority/projection mismatch,
   then observe non-empty authorized evidence plus citations in a real turn.
3. **Exercise effectful approval recovery.** Run the encrypted descriptor,
   lease-reclaim, receipt-before-effect protocol with the real execution
   worker, then kill the worker after the receipt and verify exactly one
   external effect and a durable audit trail.
4. **Make the conversation owner canonical.** Transcript reads now go through
   the durable Model Gateway/Session Core message owner; the frontend BFF no
   longer stores or serves a Redis transcript cache. Pin/title/preview remain
   owner-backed projections. Staging still needs the authenticated customer
   journey, DSAR/erasure observation, and activity/run projection evidence.
5. **Close the learning loop live.** The real NATS trigger is proven and the
   consumer starts, but the live reviewer currently fails closed because no
   provider deployment has verified ZDR support. Register and independently
   attest a provider route, then observe session-core/inference-core review,
   candidate persistence, and next-turn injection.
6. **Add signed per-call capability evidence.** The policy contract now carries
   a short-lived Ed25519 JWS binding capability/version, tenant, run, agent,
   scope, decision, reason, and budget; execution-core verifies it before an
   allow reaches dispatch. Production still requires key provisioning, image
   rebuild, and a live allow/verify trace.
7. **Attach the two eval lanes.** Run `scripts/eval-evidence.sh validate` for
   the fixed-policy `model-only` control and `integrated-harness` lane, then
   use `compare` to prove they use the same suite revision and fixture IDs.
   Each evidence directory carries only task/config/trace fingerprints and
   bounded result telemetry; the private task corpus and any customer material
   stay out of the candidate. Bind both validated records to the signed
   candidate before reporting an improvement claim.

## Six-sequence execution status (2026-08-11)

The current execution sequence is intentionally evidence-first:

1. **Release evidence before features — complete locally.** The release
   contract, runtime partition, input-security, durable-layer, and gateway
   focused test gates are repeatable. A signed candidate and rollback rehearsal
   are still operator-controlled gates, so this item is not a production
   promotion by itself.
2. **Release blockers — local recovery proof complete; external attestations pending.**
   ZDR downgrade prevention, durable mutation scoping, browser risk
   backstops, encrypted continuation descriptors, and approval lease recovery
   are covered in source/tests plus disposable-dependency harnesses. The
   remaining blockers are an independently verified provider ZDR route, a
   deployed crash/recovery observation, and signed candidate/rollback artifacts.
3. **Customer-proof E2E — local boundary harness complete.** Run
   `scripts/tests/customer-proof-e2e-test.sh` for grounded context, disconnect
   persistence, monotonic ZDR, and cross-tenant approval isolation. These are
   release-shaped integration tests; staging must still add the effectful
   approval crash/recovery and learning-review journeys against an immutable
   artifact and real cross-plane dependencies.
4. **Canonical chat durability — local implementation and restart proof complete.**
   Thread replay now includes direct thread-owned creation/message events, and
   stream buffers are identity-scoped with terminal-cursor semantics. Redis
   restart and PostgreSQL thread replay are executable local proofs; the
   frontend BFF transcript cache has been removed in favor of the durable
   Model Gateway/Session Core owner. The remaining parity gate is an
   authenticated deployed customer journey plus activity/DSAR projection
   observation.
5. **Governed capability intelligence — health reporter and signed evidence path present.**
   Policy responses carry a stable decision correlation ID, exact capability
   version, and a short-lived Ed25519 decision proof; execution-core verifies
   the proof before allowing dispatch and retains the measured heartbeat
   backstop. Key provisioning/rebuild and live ranking/allow observations
   remain open, while the NATS learning trigger has a real-bus harness but
   still needs real cross-service/LLM observation.
6. **Large bets — deferred behind gates.** GraphPlan, RL/continual learning,
   and broad capability expansion remain research or milestone work. They may
   not be promoted by a local score or an unverified external benchmark.

The authoritative “done” rule is the evidence class table above: local source
and tests can close engineering work, but only staging/candidate/rollback
evidence can close release work.

## Follow-up pass — capability-registry durability (2026-08-11)

The next local parity pass closed the MCP cache/share gap identified by CR-01,
CR-02, CR-03, and AZI-1: authenticated list/chat paths now hydrate a tenant's
MCP projection from capability-core, replace stale local entries fail-closed,
restore owner/share metadata, and persist share changes through the durable MCP
catalog with rollback on rejection. The reconcile consumer remains a low-
latency removal optimization; a JetStream/durable-consumer revocation proof,
restart across multiple replicas, and Control-Plane `resource_grants`
consolidation remain release/staging work. Hydration never imports credentials;
OAuth dispatch continues through capability-core's encrypted token resolver.
