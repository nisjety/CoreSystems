# Verevon Roadmap

**Last updated:** 2026-08-05. Synthesizes `verevon-vision.md` (what Verevon
should become), `VEREVON.md` and `Verevon-ai-first.md` (what is verifiably
live today), and `verevon-feature-map.md` (the feature-by-feature reality
map, including its own §5 recommended sequence, which this roadmap absorbs
and updates rather than duplicates). Also folds in the findings from a
code-health audit started today — see §7 for its actual status, which is
partial: a hard subagent-quota wall stopped it after 1 of 8 planned
verevonv3 areas, and the "down the stack" pass across the other 5 planes
never started. That is a real, named gap in this roadmap's evidence base,
not swept under anything — §7 is the concrete plan to close it.

> **Architecture synthesis — 2026-08-03.** This document is synchronized with the latest code-aware improvement sources:
>
> - `/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026.md`
> - `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane/docs/MODEL_PLANE_IMPROVEMENTS_2026.md`
>
> **Local Docker operating rule — 2026-08-03.** “Deploy” in this roadmap means
> rebuild and run the updated CoreSystem stack locally in Docker. It does not
> mean an external production release. After each meaningful implementation
> phase, rebuild affected images, apply migrations/configuration, verify health
> and the real user flow, fix runtime errors, and repeat. External customer
> deployment is out of scope until the local stack is complete and stable.

> The resulting authority split is explicit: **Quarry captures and verifies web evidence; Data Plane owns durable knowledge and organizational memory; Model Plane plans, reasons, selects capabilities, and proposes memory updates; Application/Control own human intent, identity, and review surfaces.** Mem0 is treated as a Data Plane design donor and benchmark target, not as Verevon's canonical memory authority.

---

## Current execution ledger — 2026-08-05

This is the current execution source of truth for the six Verevon strategy
documents. Their dated audit notes remain useful evidence, but where an older
status statement conflicts with this ledger, this ledger controls the current
claim and next action. It does not replace the Vision's product direction or
the Feature Map's detailed capability analysis.

**Operating boundary.** All work remains local-Docker work: implement, rebuild
the affected services, apply configuration or migrations, verify the running
flow, fix what fails, and continue. No statement in this ledger authorizes or
claims an external customer deployment.

**Support state today.** The unified Support workspace has working
Conversation, Ticketing, and content-free Outbound surfaces. Gmail and
Outlook use permission-scoped mailbox filters and source-health states rather
than generic provider lanes. Inbox and Ticketing share the bounded Support
context contract, governed AI proposals, and a right-rail assistant; Outbound
shares that assistant identity only for explain-only, receipt-scoped context.
Outbound now presents the selected canonical ledger receipt in its Details,
Actions, and Audit rail tabs: work state, provider receipt state, opaque
provider reference when available, error code, content-free audit identifiers,
and a safe link back to the source conversation. It cannot send, retry, infer
delivery, or expose customer message content from the ledger.

**Insights state today — 2026-08-05.** The local Insights surface is now a
real, permission-scoped read surface rather than one generic dashboard behind
several tabs. Overview, Support, Social, Agents, Chat, Knowledge, Ingestion,
Campaigns, and External sources each request only their allowlisted Insight
Core surface plus a 7/30/90-day or all-recorded window where that surface has
an event contract. The Gateway resolves the organization (the tenant boundary)
from the validated session and rejects unknown surfaces or browser-supplied
tenant/user identifiers. A nested **My activity** scope is resolved from the
same verified session identity, so it can only narrow the already-authorized
organization; it cannot select another user's activity. Inbound customer work
without a human actor correctly remains organization-level only.

Support continues to consume authoritative Conversation Core lifecycle events.
Chat consumes only new Model Gateway global-chat starts that carry both a
verified organization and user; it does not inspect or retain chat content.
Ingestion consumes durable Import/Crawl lifecycle events plus Integration Core
sync receipts, again as content-free counts. Knowledge combines a live Data
Plane snapshot under the gateway-minted, user-scoped Data Plane token with a
durable lifecycle bridge. The snapshot's document, indexed-document, and source
counts reflect only knowledge visible to the signed-in user in the active
organization. The bridge reads only Data Plane's already-published local
document outbox and emits a content-free lifecycle record with event ID,
organization, verified actor when present, and time. It excludes ZDR, private,
shared, and legacy rows without explicit organization visibility; it never
copies a document ID, title, source, URL, or body into Insights. Its separate
completion marker means an Insights outage can retry without delaying document
persistence or indexing.

The page renders only recorded scorecards and the matching live connector
registry. The static GA4/Search Console report-shape cards were removed:
external analytics displays only provider rows already recorded by Insight
Core. The third overview card is now the real organization-level Cost Core
AI-usage ledger total, explicitly not a fabricated cost-per-resolution ratio.
Focused Data Plane, Insight Core, Gateway, and frontend tests plus frontend
typecheck pass. The local Data Plane migration and Application/Insight images
have been rebuilt; Documents API is healthy on the reviewed cross-plane network
and its Knowledge mirror is enabled, while Insight Core has rebound its durable
Chat and Ingestion consumers. Existing local document-outbox rows predate the
organization-visibility field, so the bridge deliberately marked them complete
without emitting historical Knowledge metrics. That preserves privacy but means
the empty current Insights event store is correctly shown as no recorded
activity, not as zero historical change. Direct HTTP verification of the local
frontend returns 200; the in-app browser transport returned `ERR_EMPTY_RESPONSE`
and must be retried before treating a signed-in visual render as verified.

**Not yet proven or intentionally deferred.** A connected mailbox and inbound
ingestion are not provider send, acceptance, delivery, read, or bounce proof.
The next delivery gate is a deliberately authorized real send followed by the
provider's authoritative receipt and the full callback/bounce/retention path.
No automatic resend, customer communication, ticket mutation, routing, or
Autopilot behavior is implied. A structured Chat return receipt remains
blocked on the separate Chat team's approved contract. Meta channel completion
and Slack/Discord/X/LinkedIn inbound completeness remain provider-asset,
provider-tier, bot/configuration, webhook/publication, or implementation
gates; they must remain visible as such rather than being described as shipped
inbox capability.

**Ordered next work.**

1. Verify the Support delivery loop with an explicitly approved test send and
   authoritative provider evidence; then close the corresponding callback,
   bounce, timeout, retention, and UI state gaps found by that test.
2. Continue bounded Support work-object improvements only through explicit
   policy, preview, review, and per-item receipts; do not add bulk or
   context-free approval.
3. Integrate structured Support→Chat return only after the Chat team publishes
   its contract, then verify the exact context, history, and audit handoff.
4. Treat the broader security, cross-plane reliability, and operational work
   in §9 as parallel roadmap tracks, not as a reason to overstate Support
   provider parity.
5. Trigger and verify the first real organization-visible, non-ZDR Data Plane
   document lifecycle with a signed-in current session. Confirm that it produces
   exactly one tenant-scoped Knowledge metric and, when the write has a verified
   actor, that **My activity** narrows to that actor. Retry the signed-in visual
   check once the in-app browser can reach the healthy local frontend; do not
   manufacture a test scorecard.
6. Do not call the org-level Cost Core number “cost per resolution” until Cost
   Core records a verified support-surface and outcome attribution that can be
   joined to the same reporting window.

## 1. Where Verevon actually stands, right now

> **Local runtime verification — 2026-08-03.** The current Support slice was
> rebuilt locally (gateway, frontend, and Conversation Core), then checked by
> health endpoint and rendered Support UI. The first loop found stale local
> Control-broker topology rather than an application-code flaw: the scoped
> interactive-retention consumer was not provisioned. The source already used
> least-privilege permissions; rebuilding/running the idempotent topology
> provisioner and restarting Conversation Core made the consumer bind. Treat
> this as the intended workflow: source tests, rebuilt images, runtime checks,
> repair drift, then continue implementation.

> **Local provider-proof status — 2026-08-04.** The renewed Google Workspace
> connection is authenticated and renders as **Connected** with Gmail, Drive,
> and Calendar scopes. A manual incremental sync was accepted (`202`) and
> reached the explicit `handoff_data_plane` boundary. The OAuth refresh
> credential is no longer the blocker. After the mailbox was changed to an
> account with Gmail enabled, the email worker ingested six Google messages at
> 14:16 UTC. That is live inbound-read and ingestion evidence only: provider
> acceptance and delivery still need their own authoritative receipts. This
> preserves the existing acceptance/unknown-outcome model.

> **Integration truthfulness repair — 2026-08-04.** Auth Core now publishes
> its authoritative plane-token issuer with its JWKS, and Integration Core
> verifies incoming audience tokens against that authority-provided issuer
> (with the configured issuer only as a legacy fallback). This removes the
> cross-stack issuer-drift failure that previously surfaced as the Support
> Inbox's “Token verification failed” warning. A local authenticated Inbox
> connection read returned HTTP `200` and the warning disappeared before the
> required Auth Core rebuild invalidated the browser session. Finspo additionally
> caches unexpired provider-token leases per organization, so a SharePoint
> page crawl no longer asks the broker for one token per page and trips its
> rate limit. Outlook's scheduled worker has produced ingestion evidence, and
> the reconnected Gmail account has ingested six messages. A provider's
> machine-readable OAuth `invalid_grant` is now persisted as
> `needs_refresh`, excluded from token use and email sync, and shown as
> **Needs reconnect** rather than **Connected**. The API and email-worker
> images were rebuilt, recreated, and verified against the affected Google
> connection. The Google reconnect was completed on 2026-08-04. The remaining
> provider proof is a real send plus the resulting authoritative acceptance or
> delivery evidence; the formerly disabled mailbox should be disconnected or
> re-authorised deliberately if it remains configured.

The single biggest fact this week's work changes: **the "engine not proven
live" caveat that gated almost every recommendation in `verevon-feature-map.md`
is gone.** Model Plane's hot path ran real streaming chat for hours against
live Azure providers, tool loop, and GraphRAG retrieval. That single proof
retires more open questions than any individual feature fix — it converts
"the source is built for this" (§2 of the vision doc) into "the running
system has demonstrated it" for the chat surface specifically.

Alongside that, this week closed several capabilities that were previously
either completely inert or outright faked, without anyone knowing it until
the diffs were actually read:

- Cost-aware model selection (Budget/Balance/Genius) was **silently inert**
  since it was built — a missing auth header 401'd every budget check.
- The per-answer confidence badge was **hardcoded at 72%** for every
  tool-grounded answer.
- Delegated subagents were **completely faked** — a canned string, zero
  execution.
- Durable Temporal workflows (deep research, wide research, memory/skill/
  feedback promotion) had **zero production callers**, despite being fully
  built and tested.

All four are now real and live-verified. This matters for how this roadmap
is written: **do not trust a "this exists" claim anywhere in this repo's
docs without a live-verification date next to it.** Several of the above
were documented as "live" for weeks while being completely inert in
production. The practice this roadmap commits to, and that any team
picking it up should keep: read the diff, not the commit message; verify
live, not just in source.

What is now real and load-bearing, condensed from the three status docs:

- **Chat**: streaming, resumable (a disconnect no longer kills the run),
  edit/regenerate version navigation, MCP third-party tool calls (proven
  live against a real Visma ERP), a code interpreter + canvas artifacts
  producing genuine files, a response cache reaching real traffic, memory
  provenance shown to the user, server-owned pinning.
- **Agents**: a real, HITL-enforced Run Console; durable workflows reachable
  end-to-end; cron-fired runs visible.
- **Knowledge**: hybrid + GraphRAG retrieval, real source traces, per-user
  ownership — the "crown jewel" claim in `verevon-feature-map.md` §1.1 holds.
- **Inbox / Ticketing**: real backends, real AI-draft-with-HITL, real
  SLA/macros/automation-rule data model, and a bounded recurring-support
  signal derived only from exact active-ticket taxonomy in the current queue —
  with two fresh code-health gaps named below (§7.1). Operators may run a
  permission-aware Knowledge lookup for a signal; only a completed empty result
  is called a knowledge-gap candidate. Neither is semantic clustering, proof
  that knowledge is absent, or an automated Incident/Problem decision.

What is still open, unchanged by this week's work:

- Post-approval continuation (an approved risky action has no restartable
  dispatcher/receipt).
- ZDR provider attestation (enforcement is complete; no provider is
  contractually attested — a business task, not an engineering one).
- The capability-health scope for code interpreter/canvas/sandbox is real
  in source but not yet granted in the deployed service-principal registry.
- WorkflowBuilder/ChatbotStudio/ChatbotPlayground remain presentation-only.
- Image provenance (no SHA tags/`/version` endpoints across the full
  estate — partially done for 5 slice services per feature-map §6.7).
- The session-core gRPC approval surface is unauthenticated — **and a
  second, more specific gap was found today**: even a fully authenticated
  caller can cross tenant boundaries on an approval, because model-gateway's
  approval RPCs carry no `org_id` at all (§7.2).

## 2. The vision, restated as a filter for everything below

Per `verevon-vision.md`: Verevon's moat is three properties combined —
grounded retrieval with real source traces, server-enforced _approvable_
execution, and EU/Norway residency as an architectural contract, not a PDF.
No roadmap item below should be read as "build a feature." Every item is
read as: **does this deepen one of those three properties, or does it not
matter yet.** The curated-wedge doctrine (vision §3, feature-map §6.6)
applies with equal force: one best-in-class move per competitor axis, never
a chase for parity.

## 3. Phase A — Close what this week's work surfaced (do first, small, real)

### 3.0 Priority override from the Model/Quarry/Data synthesis

The following order is authoritative where it conflicts with older Phase A wording. It reflects the two latest code-aware improvement documents and the Data Plane memory review.

#### P0 — release correctness and authority

1. **Quarry browser-network containment:** enforce resolve-and-pin address authority, redirect revalidation, private/metadata/unspecified-address denial, proxy/remote-runtime parity, and unignore adversarial SSRF tests.
2. **Approval continuation dispatcher:** lease the existing `approval_delivery_outbox`, bind tenant/user/run/step/capability/payload/policy/idempotency, execute once, verify, and acknowledge. Resume the current step/run continuation—not a future GraphNode.
3. **Wire Quarry's existing durable Postgres frontier:** remove the in-memory production path, prove lease/recovery/checkpoint/cancel/backfill semantics, and expose real readiness.
4. **Memory deletion and scope correctness:** fix Letta Postgres deletion, prove canonical and derived-index purge, and make Control-issued scope immutable to caller metadata.
5. **Resolved 2026-08-04 — Delegation replay closure for Control Session Core.**
   Gateway → Control Session Core now uses a signed `v2` delegation that binds
   a bounded, single-use nonce alongside the existing audience, actor,
   request-target, and body hash. Session Core rejects absent, duplicate, stale,
   oversized, or invalidly signed nonces before it installs the delegated user
   identity. The replay cache expires entries at the signed delegation's normal
   30-second lifetime and has a fixed capacity; it cannot grow without bound.
   A cross-language fixed vector and an authenticated same-nonce replay test
   cover the contract. The local Session Core and Gateway images were rebuilt
   together and then probed through the running HTTP boundary: the first signed
   request returned `200`; replaying the identical signed request returned
   `403`. The receiver therefore does not accept a format the active sender
   cannot produce or a previously consumed delegation.
6. **Truthful runtime evidence:** commit/image/config/schema/model/harness/capability hashes on every consequential run; readiness must test the real hot path.

#### P1 — verified outcome foundation

1. Define common `Effect`, `ExecutionReceipt`, `VerificationResult`, and failure taxonomy (`verified_success`, `verified_failure`, `partially_verified`, `unknown`).
2. **Landed 2026-08-08 (server + wiring) — the first Verevon Proof Bundle.**
   `proof_bundle.proto` defines `RunProofBundle`; session-core's
   `GetRunProofBundle` assembles it per run, tenant-scoped through the same
   guards as `list_approvals`, from the evidence chain this plane owns
   authoritatively: authority (`approvals`) → execution
   (`approval_continuation_receipts`) → observation
   (`approval_continuation_outcomes`) → verification (P1 item 1's shared
   `VerificationResult`). Reachable at
   `GET /api/v1/orchestration/runs/:run_id/proof-bundle` through
   model-gateway and the verevonv3 gateway.
   **v1 is deliberately partial and says so:** the dimensions other planes
   own — *known* (Data Plane retrieval/citations), *charged* (cost-core),
   *retained* (Control Plane policy) — are named in the bundle's
   `unavailable` list with a reason rather than fabricated or silently
   omitted, so a reader can tell "not proven yet" from "nothing happened".
   Every absent stage stays absent: an approval alone never implies
   execution, a receipt with no outcome never becomes a terminal state, a
   missing verification never defaults to a status, and an unrecognized
   verification value floors to `unknown` rather than a success variant.
   Assembly is a pure function so those rules are unit-tested without
   Postgres.
   **The Agent Run Console display landed the same day.** Each absent stage
   renders in its own `unproven` tone — deliberately neither an ok nor an
   error shade — so "no execution recorded", "under execution", and "no
   independent verification" each read as themselves rather than as failure;
   a finalized-but-unverified outcome is still shown, visibly marked as the
   executor's own report rather than a verified one. The `unavailable`
   sections always render with their reasons, never collapsed, and a failed
   fetch says so explicitly instead of rendering an empty panel.
   Remaining for P1 item 2: the Trust projection, and the cross-plane
   sections (*known*, *charged*, *retained*), each of which must arrive
   through its owning plane's own contract — there is no shared database to
   join across.
3. Add deterministic postcondition verifiers for the pilot actions and browser procedures.
4. Add stateful provider/browser simulators, fault injection, shadow replay, and CI release gates.
5. Establish core metrics: verified completion, false success, cost/time/human effort per verified outcome, evidence support, intervention, unnecessary approval, and rollback rate.
6. Add Surface/API/Agent parity tests for material actions.

#### P2 — quality, usability, and performance

1. Procedure/Skill compiler for 3–5 curated presets; no canvas dependency.
2. Progressive MCP/capability disclosure, schema caching, result handles, projections, and direct tool-to-tool dataflow.
3. Prompt/cache/context observability and benchmark-driven static TOON/compact-JSON selection.
4. Data Plane Memory Intelligence: async typed candidates, provenance, authorized scope, temporal validity, contradiction/supersession, multi-signal retrieval, retention, and deletion proof.
5. Quarry Adaptive Target Memory, Challenge Intelligence, compiled browser procedures, Change Intelligence, and Quarry Quality OS.
6. Action Readiness Map to turn production failures into ranked knowledge/data/capability/procedure/policy/verification improvements.

#### P3 — execution-model evolution

1. Harden existing linear `plan_steps` into typed step-run records with receipts, verification, retry, compensation, and semantic recovery.
2. Add dependency edges and joins only where real procedures require them.
3. Introduce immutable GraphPlan versions incrementally; avoid a big-bang rewrite.
4. Read-only MCP code mode may follow sandbox/result-handle maturity; effectful code mode waits for exact grants, simulators, and proof bundles.
5. Adaptive test-time compute, A2A, native audio/video memory, and autonomous skill generation remain post-pilot research.

Concrete, bounded, already-scoped fixes surfaced by this week's digest and
today's audit — not speculative roadmap items, actual named bugs:

1. **Resolved 2026-08-03 — Ticketing error and action boundaries**
   (`verevon-feature-map.md` §1.4): `loadTicketList` now preserves list errors
   as first-class state, allowing Ticketing to render its unavailable state
   instead of a false empty queue. Lifecycle controls route through the typed
   `tickets.*` action helpers and re-read the canonical ticket afterwards;
   each assignment or lifecycle change remains a separate auditable action.
2. **Resolved 2026-08-03 — Cross-tenant approval IDOR** (`VEREVON.md`):
   approval reads and decisions are now tenant- and actor-scoped from the
   verified identity through the SQL predicates. The decision update uses
   `WHERE id AND org_id AND status = 'requested'` in one compare-and-set
   transaction, and delivery records are inserted in that same transaction.
   Focused session-core tests prove cross-tenant creation/decision rejection,
   tenant-required reads, and the tenant-scoped decision query.
3. **Post-approval continuation** (long-standing P0, `verevon-feature-map.md`
   §1.2/§1.8): granting an approval now atomically writes an identifier-only,
   tenant- and actor-scoped delivery-outbox record. It can be safely leased,
   retried, and terminally settled, but deliberately has no execution
   dispatcher, successful-delivery state, or provider receipt until Execution
   Core can prove it started the exact suspended work. HITL can therefore
   pause durably but cannot yet resume an approved risky action; do not claim
   post-approval execution is implemented.

    **Required continuation contract, confirmed 2026-08-03:** this is not a
    switch on `StateStore.resume_approved`. The current in-memory state contains
    only a run ID, step index, and lifecycle. `CreateApprovalRequest` retains a
    step label but no original tool input, purpose-lock/tool definition, model
    round state, or reusable authority. Re-running `RunAgent` from the saved
    goal would ask the model to make a new decision and could execute work that
    differs from the approved action. A safe implementation must therefore add
    all of the following as one reviewed cross-plane design: (a) a versioned,
    immutable continuation descriptor bound to approval, run, organization,
    user, exact action fingerprint, and preconditions; (b) a retention rule that
    forbids descriptor persistence for ZDR runs and therefore leaves them
    non-resumable; (c) a narrowly scoped execution-service credential or
    one-time continuation capability—never a stored/replayed user bearer; (d)
    an execution-side compare-and-set that consumes exactly one leased delivery
    only after it has loaded and validated the descriptor; and (e) an immutable
    receipt identifying the descriptor version, started execution, terminal
    outcome, and any authoritative provider receipt. The existing outbox may
    then gain a success acknowledgement. Until every part exists and is tested,
    retry/terminal acknowledgement is the only valid worker outcome.

    **Descriptor ingress, started 2026-08-03:** \`RunAgent\` now supplies a
    versioned JSON description of the exact paused tool call to
    \`CreateApproval\`; Session Core rejects an invalid fingerprint,
    cross-run/org/user scope, or common credential-bearing keys before storing
    it with the immutable approval metadata. ZDR calls remain descriptor-free.
    A second service-only contract now lets a worker read that descriptor only
    with its exact active outbox lease (organization, delivery, approval, and
    opaque lease token); general approval reads still never expose it. This is
    deliberately still not execution: a worker can now record one append-only
    **started** receipt through its active lease, with descriptor version and
    action fingerprint derived server-side; an idempotent replay tells it not
    to execute a second time. It can likewise append one terminal outcome under
    that lease: **completed** requires an authoritative provider receipt ID,
    while failure/cancellation retain only an allowlisted code. A completed
    outcome can now settle the internal approval-delivery outbox only when it
    names the same active lease and immutable receipt. “Settled” explicitly
    means continuation-protocol complete, never customer delivery. No action is
    resumed until the one-time execution authority and worker portions above are
    implemented.

    **Provider-write release dependency, audited 2026-08-03:** Integration Core
    no longer accepts a bare approval marker for an effectful provider action.
    Its action endpoint requires a scoped \`integration:write\` service bearer and
    a trusted Ed25519 write attestation that binds the presenter, organization,
    connection, provider, operation, exact request payload hash, idempotency
    key, authorization/approval/action IDs, and actor. Execution Core's current
    integration client can mint the scoped bearer and forward an approval ID,
    but cannot mint that attestation; no Model Plane signing key or trusted-key
    deployment contract exists for it. A delivery worker must therefore remain
    disabled until Auth/Integration owners provision a dedicated, rotatable
    execution-continuation signer and register its public key with Integration
    Core. It must use the descriptor fingerprint-derived idempotency key and
    attest the exact descriptor payload—not a newly generated model decision.

4. **Grant the capability-health scope in the deployed registry** for
   `cap.command.shell`/`cap.command.sandbox` (code interpreter/canvas) — an
   operator/admin action, not code; the self-attestation heartbeat already
   works and is just waiting on this.
5. **Social calendar's clock was frozen in June 2026** (found 2026-08-02,
   resolved 2026-08-03): the calendar now derives its initial month, current
   day highlight, Today control, and new-draft timestamp from the live UTC
   clock. A component-level regression test freezes time on 2026-08-03 and
   proves the API receives that timestamp, so the June fixture cannot silently
   schedule a new draft in the past again.
6. **Resolved 2026-08-03 — 2FA step-up password visibility**
   (`verevon-feature-map.md` §1.7): the enrollment re-authentication control is
   now an explicit `type="password"` input with
   `autocomplete="current-password"`. A component regression test verifies
   both browser-visible properties without broadening the shared SettingsField
   API that is being changed elsewhere.
7. **Resolved 2026-08-03 — Social publish approval boundary**
   (`verevon-feature-map.md` §1.6): the original UI-bypass conclusion was
   disproved by the authoritative social-core path. `ensurePublishApproved`
   re-reads the tenant-scoped approval record before scheduling, enqueueing,
   and again in the publisher worker; an unapproved post receives
   `approval_required`. The calendar now reflects that boundary too: it hides
   schedule/publish controls and sends the operator to Social → Approvals
   until human review completes. Component coverage verifies the handoff and
   `go test ./...` passes in social-core. The generic action contract's unused
   `approvalId` remains a lower-priority consistency cleanup, not a way to
   bypass provider publication.
8. **Resolved 2026-08-03 — Chat's auto-attached Brreg tool schema.** The
   automatic action now uses the canonical `brreg.lookup_organization` ID,
   rather than the legacy underscore spelling that fell back to an empty
   dynamic tool. A focused regression proves a Brreg prompt emits the
   registered `q` and `size` parameter schema to the model.
9. **Runtime response validation resolved 2026-08-03**: the onboarding API
   layer now validates every response it consumes, rather than trusting a
   generic TypeScript cast. This includes Brreg search, crawl SSE packets,
   graph preview, plan recommendation/translation, shipping-carrier lookup,
   checkout start/confirm,
   lifecycle, organization/theme/plan mutations, connector actions, and
   persisted state. Malformed payloads fail closed with a precise endpoint
   error before they reach onboarding state. Focused regression coverage proves
   malformed SSE, Brreg, recommendation, and checkout bodies are rejected.
   **Remaining onboarding work:** split the large orchestration component and
   add coverage for its full state transitions. The first-step exit now asks
   for confirmation before wiping state/signing out, and the ZDR default is
   accurately documented as opt-in (OFF by default).
10. **Resolved 2026-08-03 — false frontend model-routing authority.** The
    bounded `buildModelContextPack` contract is now used by the Inbox and
    Ticketing Support-assist paths. The separately unwired browser
    `selectModelTier` policy was removed: it had no callers and could only
    produce a client-controlled model decision. Chat now exposes only the
    server-resolved Budget/Balance/Genius intent modes; Model Plane remains the
    authoritative complexity and budget router. This removes a false signal of
    local routing control rather than duplicating policy across trust boundaries.
11. **Resolved 2026-08-04 — Support AI run disclosure uses a reviewed
    server contract.** After a new Support-side Verevon result, the right rail
    shows the Control-Plane policy mode, the exact configured ZDR posture, and
    model identity only when the non-streaming invoke response supplies it.
    Model Gateway now returns measured input/output token counts and elapsed
    time for that invoke; standard-retention runs additionally return the
    Model Plane pricing calculation when it is available. The optional
    answer-quality signal is explicitly heuristic, not a factual confidence,
    customer outcome, action result, provider bill, or delivery receipt. The
    idempotency cache preserves the original invocation's usage rather than
    inventing replay metrics. The rail renders only supplied fields and keeps
    the explicit “not reported” state when a value is unavailable, including
    ZDR pricing. It neither borrows Chat streaming telemetry nor attaches new
    run metadata to rehydrated Chat history. The 2026-08-04 source-backed
    Inbox draft shortcut also preserves the exact server-returned `Assist` /
    `Review` mode; it cannot default an Assist result into the durable review
    queue.
12. **Resolved 2026-08-03 — bounded review-decision timing.** The Support
    AI review queue now derives a median from the organization-scoped action
    ledger's valid proposal-created and terminal-review timestamps. It names
    the sample size and excludes malformed or negative pairs. The UI labels it
    as recorded decision/outcome timing only—not delivery, customer resolution,
    or model quality—and retains the existing separate outcome and proposal-mix
    counts.
13. **Semantic recurrence remains a governed design gate.** The current
    Ticketing signal is exact taxonomy only. A future semantic feature must use
    a bounded conversation-core projection, verified active membership and
    retention policy, and return versioned, inspectable similarity candidates
    with no more than authorized ticket IDs as evidence. It must not use direct
    support-table reads, build a hidden retained corpus for ZDR organizations,
    infer cause/missing knowledge, or execute any Support action. The full
    contract is recorded in `verevon-inbox.md`; no semantic clustering is
    claimed until that design is implemented and evaluated.
14. **Resolved 2026-08-03 — outbound reconciliation handoff.** The Support
    outbound ledger's `unknown` canonical outcome now links only to its source
    conversation for an operator to reconcile. It does not expose a retry,
    start a new send, or call provider APIs. This makes the existing stale-send
    reconciler's `unknown` state actionable without turning an uncertain
    delivery into a duplicated customer message.
15. **Resolved 2026-08-03 — evidence-bound Ticket-update proposal from the
    Support rail.** After a human asks for a next step, the ticket rail can
    prepare a bounded `ticket.update` proposal only for a selected canonical
    ticket and an authorized transcript with canonical message IDs. The
    proposal enters the existing exact-payload review flow; preview-only text,
    Assist mode, and ZDR cannot create a durable update. It never directly
    changes lifecycle, ownership, a customer message, or ticket state.
16. **The biggest files in the repo are also the least tested** (found
    2026-08-02): `DashboardComposer.tsx` (2,586 lines), `BrowserChrome.tsx`
    (2,104), `WorkspaceSettingsPage.tsx` (2,053), `KnowledgeComposer.tsx`
    (1,949), `AgentRunConsole.tsx` (1,782), `use-chat-controller.ts` (1,416),
    `OnboardingPage.tsx` (1,096) — 21 files over the project's 800-line rule,
    6 of them over 1,500 — and every one of the largest six has **zero**
    component-level tests. This is not a coincidence: a file this size is
    both the hardest to safely refactor and the least likely to have been
    tested along the way. Treat file-size and test-coverage debt as one
    problem, not two — splitting a mega-file is also how it becomes testable.
    **Resolved sub-finding 2026-08-03 — agent route state:** `AgentsProvider`
    now accepts CoreShell's reactive router location, so a normal SPA
    navigation to a bare `/agents` or non-agent route clears stale role state
    without relying on `popstate`. A focused regression drives that transition
    without a browser-history event.

### 3a. Down-the-stack findings (2026-08-02) — three CRITICAL, rank above the rest of Phase A

A full code-health pass across the remaining 5 planes (Model Plane Rust +
Go, Data Plane v2, Control Plane, Ingestion Plane, Application Plane)
surfaced 29 findings. Three are **critical severity** and should be treated
as ranking above everything else in this Phase — not because the process
says so, but because they are live, wired, currently-exploitable-in-shape
gaps in exactly the properties `verevon-vision.md` §2 names as the moat
(approvable execution, trust boundaries):

1. **Partially resolved 2026-08-03 — headless-browser SSRF admission.**
   `quarry-browser` now owns one shared navigation guard
   that permits only `about:blank` or HTTP(S), applies `quarry_security`'s
   scheme/host/userinfo policy, resolves the target, and fails closed when
   any address is private, loopback, link-local, or otherwise unresolved.
   Chromiumoxide applies it before both `goto()` and initial `new_tab()`
   navigation, then installs a fail-closed CDP Fetch request listener before
   the page is allowed to navigate. The listener applies the same guard to
   page-controlled requests (including resource loads and CDP-reported redirect
   follow-ups), aborting a rejected request rather than passing it to Chromium.
   Browserless, Browserbase, and Kernel apply the direct-target guard before
   accepting a target (Browserbase before it creates a remote session). Four
   browser-independent unit regressions cover loopback, non-web schemes,
   blank tabs, and private resolved addresses; a Browserbase regression proves
   a blocked target creates no remote session; and all five formerly ignored
   Chromium SSRF checks pass when explicitly invoked; a Chromium regression
   proves an in-page loopback image request is aborted before its server
   receives it. **This is not complete browser-network SSRF containment:**
   browser providers other than local Chromium lack comparable request-level
   interception, an end-to-end HTTP-redirect fixture is still needed, and DNS
   resolution is not pinned after the preflight lookup.
2. **Resolved 2026-08-03 — static-fetch redirect SSRF bypass.** The primary
   crawl driver now uses `reqwest::redirect::Policy::none()` and turns every
   3xx response into `SecurityBlocked` before reading its body. A regression
   mounts a redirector and target server, proves the redirect fails closed,
   and proves the target receives no request. This closes implicit redirect
   following only; browser-provider and proxy-egress DNS-pinning gaps remain
   separately tracked below. Python connector and TLS-profile direct-egress
   pinning are now covered in the next finding.
3. **Partially resolved 2026-08-03 — social publishing now uses the
   governed action surface at runtime.** `main.go` constructs a tokenless
   `GovernedPublisher` from the existing integration client, so LinkedIn,
   Facebook Page, and Instagram post writes now call
   `integration-corev2`'s authenticated `actions/execute` endpoint rather
   than leasing a raw third-party OAuth token into social-core. The Instagram
   create/status/publish sequence remains action-mediated. Regressions prove
   the LinkedIn and Meta/Instagram calls carry only a connection ID and
   bounded action payload; a provider lacking an action contract is blocked
   before any action or token lease occurs. X, TikTok, and Snapchat are
   therefore intentionally unavailable for organic publishing until their
   complete action contracts—including Snapchat's media-upload protocol—are
   implemented in integration-corev2. The legacy raw-token `HTTPPublisher`
   remains compiled for isolated compatibility tests but is not constructed by
   the production server; deleting it after those contracts land is still
   required to prevent future wiring drift.

**One more HIGH finding has been partially resolved 2026-08-03, but remains
systemic outside the hardened direct transports**: `PageRunner` now carries Quarry-v2's
public-address-checked `ResolvedTarget` into `StaticDriver`; its dedicated
Reqwest resolver returns only those pinned socket addresses and fails closed
for any unpreflighted hostname. Unit coverage proves the resolver cannot
fall back to DNS or accept a private pin, while a StaticDriver regression
proves preflight selects that no-fallback client. `TlsProfileDriver` now
carries that same target to a per-request `wreq` DNS override, preserving the
origin hostname for TLS SNI and HTTP authority while disabling process/system
proxy inheritance for this direct-egress transport. Its pin constructor
rejects private or empty address sets and a regression proves a mismatched
target fails before connection. **Imports-core's
customer-controlled connectors are now pinned too:** `network_policy.py`
returns the normalized URL plus the immediate public DNS answer set;
the CMS connector supplies that target to a proxy-free HTTPX/httpcore backend
that dials only those addresses while retaining the original hostname for
Host and TLS validation, and the Odoo JSON-RPC client receives a matching
proxy-free pinned urllib opener. The backend rejects missing/private manually
constructed pins and unpreflighted host/port pairs; focused connector and
network-policy coverage plus the complete imports-core suite pass (52 tests).
Focused Quarry TLS and runtime suites pass (9 and 418 tests respectively).
Browser and proxy-egress transports still receive a hostname after preflight.
HTTP CONNECT and SOCKS can delegate destination resolution to the proxy, so a
local resolver override would not close that path without an explicit,
proxy-side address-authority contract. Those paths need their own
connection-level enforcement before this can be called a full cross-plane
DNS-rebinding closure.

**Everything else from this pass** (29 findings total; the remaining 25 are
HIGH/MEDIUM/LOW) is real and worth doing, but does not carry the same
urgency as the three above. Highlights, grouped by theme rather than listed
individually — read the audit output for full file:line detail if picking
one up:

- **Trust-boundary drift across Application Plane siblings**: conversation-
  core-go has a mature HMAC-delegation-with-replay-protection pattern;
  insight-core and social-core instead trust a client-supplied `org_id`
  behind one fleet-wide shared static key (today's live exploit path is
  closed because the gateway resolves org_id server-side before calling
  them, but the services themselves have no defense if that changes); convex-
  core exposes tenant-scoped mutations as public Convex functions gated by a
  non-constant-time secret compare instead of `internalMutation`.
  **Resolved 2026-08-04 —** Control Session Core now also has the signed,
  bounded single-use nonce guard, including a cross-language Gateway vector
  and a replay rejection regression. The remaining sibling-service findings
  are still open.
- **Resolved 2026-08-03 — browser-agent risk-label integrity.** Deterministic
  browser evidence now runs before the planner's advisory category. A model
  cannot relabel a checkout or destructive action to make the human approval
  prompt less informative; its self-report is retained only when no concrete
  browser signal exists. Unit and approval-gate regressions prove a planner
  `login` label on a checkout action is recorded and approved as `checkout`.
- **Architecture-rule bypass, smaller scale**: user-core calls Microsoft
  Graph directly with a raw user token instead of through integration-
  corev2's existing `microsoft.profile` operation — the same class of
  violation as social-core above, one plane over.
- **A DSAR/erasure-relevant correctness gap**: letta-bridge's Postgres memory
  tier makes `DeleteMemory` a permanent silent no-op (the table has no
  per-user ownership column), indistinguishable on the wire from "already
  deleted." Any erasure workflow that trusts this response leaves rows
  behind indefinitely.
- **Resolved 2026-08-03 — Capability Core false-success mutations.**
  Routing, Safety, Skills, MCP, Plugin, Task, and Cron PATCH/DELETE handlers
  now use one tenant-scoped mutation, require exactly one affected row, return
  404 for a wrong/foreign/deleted id, and expose only a generic database
  failure. Task cancellation additionally rejects an existing terminal task
  with 409 instead of claiming a cancellation; a Cron expression edit validates
  against its stored timezone and atomically recalculates `next_fire_at`.
  Focused API regressions cover zero-row writes, atomic multi-field changes,
  and terminal-task cancellation. This closes the dangerous case where an
  operator could be told a policy, task, or schedule changed while it remained
  untouched.
- **Duplicated trust-critical code**: 4 Go services in Data Plane v2 each
  independently reimplement ~350-460 lines of JWT-verification middleware
  (already drifted — two of the four carry claims the other two don't); the
  same weak org_id-trust boilerplate is duplicated verbatim between insight-
  core and social-core.
- **One inconsistency in an otherwise well-hardened plane**: Data Plane v2's
  embedding and graph-extraction calls were correctly hardened to route
  through Model Plane token-minting; reranking (Cohere/Azure) was not, and
  still holds a locally-held API key with no audit trail for non-ZDR content.
- **Maintainability, same pattern as verevonv3**: several more god-files past
  the 800-line rule (user-core's handlers.go at 1,944 lines; conversation-
  core-go's repository.go at 2,769; capability-core's registry_apis.go at
  1,574) — the same file-size-correlates-with-test-coverage-gap pattern
  found in Phase A item 11 recurs down the stack too.

## 3b. Cross-plane Verified Outcome Foundation

This is not a new product plane or microservice. It is a shared contract and evaluation layer implemented through existing owners:

```text
Quarry evidence/browser receipts
+ Data Plane source and memory provenance
+ Model Plane plan/skill/model/harness decisions
+ Control/Application identity, policy, approval, and human corrections
+ Integration/provider effect receipts
        ↓
Runtime Evidence Manifest
        ↓
Verevon Proof Bundle
        ↓
Agent Quality OS: replay, simulation, shadow, canary, promotion, rollback
```

**Pilot exit criteria**

- every consequential demo action produces a reconstructable proof bundle;
- a false provider/browser success is caught by a postcondition test;
- a human correction creates or updates a deterministic regression case;
- the same procedure is tested against at least one provider failure and one page/layout mutation;
- memory created from the outcome is tied to provider-verified evidence, can be superseded, and can be fully erased;
- the dashboard reports verified completion and cost per verified outcome rather than raw model/tool activity.

---

## 4. Phase B — Make the moat visible (GTM, from the vision doc)

Per `verevon-vision.md` §3: the Trust Center is the most rigorous of the
entire competitor set and was, until 2026-07-20, invisible. A first pass
shipped (an eyebrow line, a second CTA, a named-partner line). What remains,
in priority order:

1. **Decide the `BrandLogosSection` connector-carousel question** (open per
   `COMPETITOR-ANALYSIS.md`'s "Open, not yet done" note): confirm which
   connectors are genuinely live/credentialed in production (not mock
   fallback) before adding Vipps/Bring/UPS/DHL/FedEx/Meta/X/LinkedIn, and
   drop Altinn (zero code found). Bring specifically was flagged
   "unverified" in an earlier audit — re-check before including it.
2. **Do not upgrade the sovereignty claim** from "EU/EØS" to a
   Norwegian-infrastructure claim until the Telenor AI Factory conversation
   closes (still open, per project memory — this is a hold, not a task).
3. **Apply the curated-wedge per-competitor counter-table**
   (`verevon-feature-map.md` §6.6) concretely: Ayfie (acting agent they lack),
   Mimir (visible approval step they don't show), Chatbase (match the <15min
   time-to-value number, it's testable), Intercom (same shape, SMB-priced,
   paused-approved action instead of a trust-us number). These are
   marketing/positioning tasks, not engineering ones, and belong on a GTM
   team's plan, referencing this roadmap as the source.
4. **"Verevon Support" packaging** (vision §4): name and package the
   existing Inbox + Ticketing + AI-draft-HITL + embeddable-widget work as a
   curated Intercom-equivalent, once Phase A's ticketing fixes land and the
   post-approval-continuation P0 closes — packaging a differentiator before
   the differentiator itself is fully solid would be premature.

## 5. Phase C — The pilot security gate (unchanged in substance from feature-map §5 Phase 3, updated)

Everything `verevon-feature-map.md` §5 Phase 3 already named, now with two
additions from this week/today:

- Full IDOR closure — **now explicitly including the cross-tenant approval
  gap found today (§3.2 above)**, not only the header-based org-id class
  already closed.
- RLS active + enforce-mode auth; erasure/DSAR purge subscriber (a
  product/legal decision on scope is still the blocker, not code); Data
  Plane credential rotation (operator-owned, out of agent scope by design);
  a real CI gate on the cross-tenant regression tests that exist but are not
  wired into any workflow GitHub Actions actually reads.
- Image provenance: extend the git-SHA-tag + `/version` pattern already
  done for 5 slice services (feature-map §6.7) to the rest of the estate.

## 6. Phase D — Open pilot, then pull-driven expansion (unchanged from feature-map §5 Phase 4-5)

No new information this week changes this sequencing. Restated once, briefly,
so this doc is self-contained: self-serve onboarding, NOK pricing, Norwegian
UI pass, 10 external users active weekly with honest Insights numbers — then
expansion strictly pull-driven (Agents canvas only on demonstrated
preset-modification demand; Insights depth; more inbox channels; Social
live-proof; Norwegian AI-visibility as a new bet; Zendesk-parity extras).
`verevon-feature-map.md` §5 has the full gate language; it is not repeated
here to avoid the two docs drifting out of sync — treat that section as the
canonical detail, this roadmap as the "what changed, what's next" layer on
top of it.

## 7. The code-health audit — status, and how the rest resumes

This roadmap was supposed to be grounded in a full code audit "starting
with verevonv3, working down the stack." **Update: verevonv3 (Stage 1) is now
complete.** The section below is kept as the honest record of how it
actually went, including the interruption, rather than rewritten as if it
had gone smoothly — the interruption and recovery are themselves useful
information for whoever runs the next one of these.

### 7.1 What actually ran

A parallel 8-agent pass across verevonv3 covering tickets, agents-console,
settings-integrations, social-studio, onboarding-auth, shared-infra,
gateway-bff, and a cross-cutting dead-code/test-coverage sweep. **7 of 8
agents hit a hard subagent-usage quota mid-investigation** (each had already
made 16-31 tool calls) with the error `You've hit your session limit ·
resets 2am (Europe/Oslo)`. Only the `tickets` agent completed on the first
pass — its findings are folded into §3.1 and `verevon-feature-map.md` §1.4.

**The quota reset and the remaining 6 areas were re-run successfully**,
producing 42 more findings — folded into §3 items 5-11 above and
`verevon-feature-map.md` §1.2/§1.6/§1.7/§1.8/§1.10. Stage 2 (the "down the
stack" pass across Model Plane, Data Plane v2, Control Plane, Ingestion
Plane, and Application Plane) was launched immediately after and its status
is recorded in §7.3.

### 7.2 What was done manually instead

Rather than wait idle or grind through the remaining 7 verevonv3 areas plus
5-6 more planes entirely by hand (an enormous, context-expensive undertaking
better suited to the parallel approach once it's available again), one
targeted manual pass was done on the single highest-security-value item: the
Rust gateway BFF (`apps/gateway`), since CLAUDE.md explicitly calls out its
secret/token/header-forwarding hygiene as a hard requirement. That surfaced
the cross-tenant approval IDOR in §3.2. The adjacent org-id/token-minting
paths (`mcp.rs`, `model_token`/`required_capability_token` in
`chat/shared.rs`) were checked and confirmed clean — correctly keyed off
`user_id` + verified session, not a client-controllable org field.

### 7.3 Final status: complete

Stage 2 ran successfully on the first attempt after the quota reset — 6/6
service groups (Model Plane Rust, Model Plane Go, Data Plane v2, Control
Plane, Ingestion Plane, Application Plane), 29 findings, folded into §3a
above and `verevon-feature-map.md` §2 item 8. **Both stages of the
"verevonv3 first, then work your way down" audit are now done**: 8 verevonv3
areas (7 findings from the first pass + 42 from the continuation after the
quota reset, 49 total) + 1 manual gateway spot-check (the cross-tenant
approval IDOR) + 6 down-the-stack service groups (29 findings) — **79
findings in total** across the two source workflow outputs. Not every one
made it into these docs verbatim; the highest-severity and most
consequential ones did, per this section's own §7.1/§7.2 process — the full
per-finding detail (file:line, exact recommendation) lives in the workflow
transcripts if a smaller item needs picking up later.

This roadmap's §1-§6 now rest on a genuinely broad evidence base spanning
the full monorepo, not just this session's own chat-feature work — the
"partial evidence" caveat earlier versions of this section carried no longer
applies. What was NOT done, and would be the natural next pass: acting on
the findings themselves (this audit found and documented, it did not fix,
with the exception of the chat-resume/edit-version work and doc corrections
already shipped this session) — that is what §3/§3a/§5 are for.

## 8. Explicitly not now

Unchanged from `verevon-feature-map.md` §5 and `verevon-vision.md` §5:
n8n-parity builder, a big-bang GraphPlan rewrite, effectful MCP code mode, A2A swarms, full Google-class search, classic SEO, Studio Plane
services, Channel Plane, meeting notes/voice/video memory, enterprise-only items
(mTLS/SPIFFE, HA/DR, multi-region, SOC2) — sequenced after revenue, not
before. Upgrading the sovereignty claim beyond "EU/EØS" — held on the
Telenor AI Factory outcome, not a timeline.

## 9. Next-steps synthesis across the six strategy docs (2026-08-04)

This section answers "given `verevon-vision.md`, `VEREVON.md`,
`Verevon-ai-first.md`, `verevon-feature-map.md`, `verevon-inbox.md`, and this
roadmap, what happens next" as one ordered list. It sequences across §3/§3a/
§3b/§4 rather than restating their detail.

### 9.1 The one action that unblocks the most — do this first

The **post-approval continuation dispatcher** (§3 item 3, P0-2 in §3.0).
This week's descriptor-ingress work made the remaining gap precise: it was no
longer missing application code, it was a **cross-plane provisioning
decision**. Integration Core requires a scoped `integration:write` bearer
plus an Ed25519 write attestation for any effectful provider action.

**(a) Resolved 2026-08-04 — the signer is provisioned and registered.**
Integration-corev2's verifier was generalized from a single hardcoded issuer
(`conversation-core` only) to a small allow-listed set including
`model-execution`, keyed so a compact JWS's signing key is looked up by `kid`
alone and its claimed issuer is then bound to that specific key's registered
issuer — closing the one real gap that made a second issuer impossible to
add safely. execution-core (Rust) gained its own attestation signer
(`src/attestation.rs`), byte-for-byte matching conversation-core-go's
canonicalization (verified against the same Go-produced fixed-vector test
data conversation-core's own verifier tests use — struct field order, Go's
recursive alphabetical map-key sort, and its default `<`/`>`/`&`/U+2028/U+2029
HTML-escaping all reproduced and cross-checked). `IntegrationActionsClient`
now looks up a connection's `provider_key` before signing (the model only
ever supplies `connection_id`) and attaches a real `writeAttestation` +
`idempotencyKey` on every write. The bootstrap script provisions the keypair
and assembles the combined trusted-key registry — which surfaced a second,
independent pre-existing bug while wiring it: the script had always written
conversation-core's public key to `apps/Ingestion Plane/.env`, a file the
real Ingestion Plane runtime never reads (it retired that shared file for a
per-service `.env` model, per `run-ingestion-plane.sh`'s own header comment);
the real target is `apps/Ingestion Plane/integration-corev2/.env`, confirmed
by the fact that file already held a live conversation-core key nothing in
this script could have produced. Both the new model-execution write and the
pre-existing conversation-core write now target the real file. Verified: the
full Go and Rust test suites for both services pass (integration-corev2: all
packages green; execution-core: 282/282 including new attestation tests),
plus an isolated dry-run of the provisioning script's new functions checked
against the actual `ParseTrustedKeysJSON` and against `ed25519-dalek`'s own
key derivation for OpenSSL-generated key material.

**Prod key rotation — provisioned 2026-08-04, not yet exercised (still dev).**
The trusted-key registry is a list keyed by `kid`, and every attestation JWS
has a 30-second TTL, so rotation is additive by construction: stage a new key
alongside the active one, cut the signer over, prune the old key after a
short bake period. `scripts/bootstrap_runtime_environment.sh
--rotate-model-execution-attestation-key=stage|promote|prune` implements the
three steps; each was verified against a full lifecycle (including the
multi-kid-same-issuer registry shape a rotation window produces, checked
against the real `ParseTrustedKeysJSON`) without touching any real secret.
Full runbook: `apps/Model Plane/docs/ATTESTATION_KEY_ROTATION.md`.

**(b) Resolved 2026-08-04 — the dispatcher itself.** execution-core now runs
`approval_delivery_worker`, a detached background task (mirrors
`health_attest`'s spawn-and-idle shape) that claims due
`approval_delivery_outbox` leases, fetches the exact immutable action
descriptor under that lease, and resumes it: `ClaimApprovalDeliveries` →
`GetApprovalContinuation` → parse/validate the descriptor →
`RecordApprovalContinuationStarted` (never executes twice — a prior worker's
`already_started` is honored) → execute via the now-attestable
`IntegrationActionsClient::execute_action` → `RecordApprovalContinuationOutcome`
→ `AcknowledgeApprovalDelivery`. `execute_action` was extended to surface a
provider-extracted receipt id (a Rust port of integration-corev2's own
`actionProviderMessageIDAtDepth` heuristic) — required because
`RecordApprovalContinuationOutcome`'s `Completed` case rejects an empty
`provider_receipt_id`, and a response with no extractable id is treated as a
terminal `invalid_continuation` rather than fabricated or silently dropped. A
pure gRPC/HTTP failure at any step is never acknowledged, by design — the
lease simply expires and a later poll reclaims it.

**Resolved 2026-08-04 — `book_shipment` also cold-resumes now.** The
dispatcher generalized from a single hardcoded tool to a small
`ResumableAction` enum (`ProviderAction` | `ShipmentBooking`), matched by
`tool_name` at parse time; each variant has its own outcome→`Disposition`
mapping (`disposition_for_provider_action_result` /
`disposition_for_shipment_booking_result`), sharing one
`disposition_for_transient_failure` helper for the identical "network/5xx →
`transient_dependency`, retry" case. A resumed booking reconstructs the exact
same `idempotency_key` (`{run_id}:{step_id}`) `execute_book_shipment` used the
first time, so shipping-core recognizes the retry rather than double-booking.
`book_shipment` qualifies for the same reason `execute_provider_action` did:
a single idempotent-key-gated HTTP call with a durable receipt
(`booking_id`) on the other end. **Scope, stated plainly:** those two are the
only resumable tools — `is_risky_tool` gates a wider set (`browser_agent`,
any MCP tool, …) whose state (a live session, arbitrary third-party
semantics) can't be safely reconstructed from a cold descriptor; an
unsupported `tool_name` still fails closed as `invalid_continuation`.
Verified: 20 unit tests (descriptor parsing/validation and the
outcome/failure-code/acknowledgement decision mapping for both tools against
session-core's own exact allowlist and validation rules), plus the full
execution-core suite (322/322) and clippy clean on every file touched.

**Resolved 2026-08-04 — which orgs the worker services is now auto-discovered,
not a manual list.** `ClaimApprovalDeliveries` requires a credential scoped to
the exact org being claimed (no "all orgs" bypass exists for it, unlike the
internal-only empty-org-id path `ListPendingApprovals` offers), which had
looked like a reason to fall back to an explicit org list. It wasn't: orgs
are Verevon's own data (org-core), not a third party, and the only real gap
was that org-core exposed no list-all-organizations endpoint — the
underlying `Repository.ListOrganizations` query already existed with zero
callers. Added `GET /internal/orgs` (`org:read:any`, the same scope
session-core/integration-corev2/billing-core already hold for this class of
read) and a small paginating Rust client (`org_directory`) the worker uses by
default. `EXECUTION_CORE_APPROVAL_DELIVERY_ORG_IDS` remains only as an
explicit override for staged rollout.

This closes the literal gap in `verevon-vision.md` §2 property #2 — Verevon can
now both pause an approval _and_ resume it — which unblocks the "Verevon
Support" GTM packaging named in §4 item 4.

**(c) Resolved 2026-08-04 — every built-in tool now has skill + knowledge-base
content, and skill injection reaches BOTH tool-calling surfaces, not just
one.** Two gaps closed here, and they are different in kind:

_Skills (`agent_skills`) — LIVE, changes model behaviour today._
model-gateway's inline chat loop already matched an org's `agent_skills`
against the user's message and injected the top 3 as system context
(`skills.rs`/`fetch_skill_context` in `sse.rs`) — but execution-core's
`RunAgent` loop, where the actually-risky tools live (`book_shipment`,
`execute_provider_action`, arguably needing this steering _more_ than the
read-only inline tools), never did. Ported the same mechanism into
`runtime_loop::agent::fetch_skill_context` (new, ~70 lines): fetch this org's
enabled skills via `SessionCoreClient::list_agent_skills` (reusing the SAME
delegated user session-bearer `RunAgent` already carries — no new credential
or Auth Core registry grant needed), score by keyword overlap, inject the top
3 as one system message right after `AGENT_PREAMBLE`, once per run. Advisory
by design: any failure (no bearer, no org, RPC error) yields an empty Vec so
the run proceeds unaffected, exactly like the gateway path. 3 new unit tests
(keyword match, no-bearer short-circuit, no-match) against the file's
existing in-process mock gRPC harness; full suite 308/308, clippy clean.

_Content — authored for the REAL tool catalogues, not a generic restatement._
`model-gateway::tool_loop::builtin_tool_defs` (13 inline-chat tools) and
`execution-core::runtime_loop::agent::offered_tool_defs` (17 `RunAgent`
tools, overlapping but genuinely NOT identical — e.g. `web_fetch`/`fetch_url`,
`get_shipping_quotes`/`shipping_get_quotes` are the same capability under
different names per surface) were both read in full to ground every word of
content in what the tool actually does today, not what its name implies.
Organized as 6 domain-level skills (shipping & freight, social publishing,
provider integrations, org knowledge base, public web research, Norway
reference lookups) carrying workflow-order guidance no single tool's own
`description` field can (quote-before-book, discover-before-execute,
search-vs-list-vs-metrics disambiguation) — deliberately NOT a 1:1 skill per
tool, since each tool's own `description` is already thorough and a skill
restating it would just be redundant text competing for the same 3 injection
slots.

_Knowledge base (`agent_memory`) — seeded, honestly NOT yet live._ Also
seeded one atomic `kind='instruction'`, `scope='org'` fact per distinct tool
(23 entries) into `agent_memory` — the literal "every tool has a knowledge-base
entry" ask. Checked before claiming this: `agent_memory`'s only current
readers are `dreaming.rs`'s own dedup pass and GDPR erasure — nothing reads
it into a live chat/agent turn. Seeded anyway (Dreaming's dedup sees it, and
the data will be there the day a retrieval path exists) but this half does
**not** change model behaviour yet; do not describe it as live. Wiring an
actual read path is a separate, not-yet-scoped follow-up.

_Mechanism_: `apps/Model Plane/scripts/seed_tool_knowledge.sql` +
`seed_tool_knowledge.sh` — idempotent (`ON CONFLICT` upserts keyed by
`(org_id, name)` for skills, `(org_id, scope, owner, key)` for memory,
matching `agent_memory_org_scope_key_uq` as redefined by migration `0011`,
_not_ migration `0004`'s original shape — caught by validating against the
real migrations rather than trusting the first CREATE INDEX found), safe to
re-run whenever the tool catalogue changes. Both the skills' `origin='user'`
guard and an equivalent `review_state='rejected'` guard on memory rows
protect a human's own edit from being clobbered by a re-seed — verified
against a throwaway (non-dev, torn down after) Postgres container running
the real session-core migrations end to end: fresh insert, idempotent
re-run (unchanged counts, `updated_at` genuinely advances), cross-org
isolation, and both protection guards, all confirmed before deleting the
container. Seeding into the actual dev stack happens as part of §9.1(d)'s
rebuild, not here — this pass built and validated the mechanism in
isolation, per this session's own standing rule of not touching real
environments to test logic that a scratch driver can prove first.

**(d) Resolved 2026-08-04 — rebuilt and verified end to end; found and fixed
3 real config bugs no amount of unit testing could have caught.** Rebuilding
Model Plane's execution-core and Control Plane's org-core (the two services
this session's Rust/Go changes touched) and provisioning the credentials
§9.1(a)/(b) built surfaced three genuine, previously-dormant gaps — each
found by actually exercising the new code against the running stack, not by
re-reading it:

1. **`ensure_model_execution_attestation_key`/`promote_model_execution_attestation_key`
   never wrote the active kid to `MODEL_ENV`, only to `INTEGRATION_COREV2_ENV`.**
   `attestation.rs::Attestation::from_env()` reads its own kid from its OWN
   environment (Model Plane's `deploy/.env`), not integration-corev2's — so
   the private key existed but the signer stayed silently disabled
   (`from_env()` → `None`). Fixed in `scripts/bootstrap_runtime_environment.sh`
   (both functions now write the kid to both files, and `ensure_...`'s
   short-circuit guard now also requires the local kid to be present, not
   just a valid-length private key, so a half-provisioned state like this one
   self-heals on the next run instead of being masked by the guard).
2. **Model Plane's `AUTH_CORE_ISSUER` was `http://auth-core:3011/...`
   (a Docker-Compose service name); Auth Core actually issues tokens with
   `iss: http://localhost:3011/...`** (confirmed from its own `/jwks`
   response's `planeTokenIssuer` field — `localhost` is deliberate, since
   browser-facing consumers of the same issuer can't resolve `auth-core`).
   `session-core::auth.rs::verify_token` does an exact string match on
   issuer, so EVERY JWT Auth Core mints for a Model Plane service audience
   would have failed this check — not a bug specific to approval delivery,
   a bug in the shared config every Model Plane Rust service reads. One-line
   fix in `deploy/.env`, all four affected services (`session-core`,
   `execution-core`, `inference-core`, `model-gateway`) recreated to pick it
   up.
3. **`execution-core`'s Auth Core registry principal had `retentionByAudience.
session-core = "zdr"` and lacked the `approval:deliver` scope entirely.**
   Missing scope → Auth Core refused to mint the token at all (403). Adding
   the scope surfaced the retention issue: `session-core::auth.rs::
authorize_operation` unconditionally rejects any non-`:read` scope from a
   zdr-flagged caller ("ZDR credentials cannot access durable session
   writes") — correct in general (a credential that must not retain durable
   content should not be trusted with durable writes), but wrong for this
   credential specifically, since approval-delivery outbox rows are Verevon's
   own durable operational records, not customer content. Changed to
   `"persistent"` in Control Plane's `PLANE_SERVICE_PRINCIPALS_JSON`
   (`.env.generated-secrets`); execution-core's _existing_ `session:terminalize`/
   `session:heartbeat` scopes were under the exact same (wrong) zdr posture,
   so this fix plausibly unblocks those too, not just approval delivery —
   unverified, since nothing exercises them the same way, but worth checking
   if either is later found to have been silently failing.

**What was actually proven, not just claimed:**
`/internal/orgs` returns the real 4 orgs with the real `execution-core`
credential (200, not a fixture). `approval_delivery_worker`'s auto-discovery
correctly enumerated all 4 real org ids in its log output (matching the
endpoint's response byte for byte) before any of the 3 fixes above, proving
§9.1(b)'s discovery client works independent of the credential issues found
alongside it. After all 3 fixes, the worker's per-org claim loop went
completely silent across 4+ poll cycles (60+s) with zero warnings — the
expected steady state when `ClaimApprovalDeliveries` succeeds against orgs
with no pending deliveries (nothing has HITL-paused a tool call against
these orgs yet in this environment). `seed_tool_knowledge.sh` ran against
all 4 real orgs; a direct query of the real `agent_skills` table for one org
returned exactly the 6 seeded rows with correctly-parsed JSONB
`trigger_keywords` arrays, matching what `ListAgentSkills`/both
`fetch_skill_context` implementations expect byte-for-byte. **Not yet
proven**: an actual HITL-paused tool call being claimed, resumed, and
completed by the worker end to end (no real approval exists in this
environment to resume — creating one requires a live RunAgent call with a
connected provider or shipping-core booking, which this pass did not set
up), and a live chat/RunAgent turn actually receiving an injected skill
block (proven at the unit-test and raw-SQL-query level, not via an actual
inference call). Both are the natural next verification step, not done here.

Two unrelated pre-existing issues surfaced in passing and flagged as
separate follow-ups (not fixed here — out of scope for this pass): a flaky
`execution-core` test
(`approval_delivery_worker::tests::no_override_and_no_directory_is_an_honest_empty_list`)
that fails only under full-suite parallelism (an env-var race between
tests, not a real bug in the worker), and `model-gateway`'s finetune poller
failing every ~60s with "verified caller credential required" (a missing
credential, unrelated to anything this session touched).

### 9.2 Run in parallel — same urgency band, independent owners

1. **Quarry SSRF, close the remaining gaps** (§3a item 1): request-level
   containment for non-Chromium browser providers, an end-to-end
   HTTP-redirect fixture, DNS pinning after preflight, and a proxy-side
   address-authority contract for CONNECT/SOCKS egress.
2. **Corrected 2026-08-06 — this item's premise was wrong; do not wire it
   in as originally written.** Investigated before touching anything: the
   real production durable-dispatch layer for crawl/batch work already
   exists and is live — a Go control-plane job table + Temporal workflows
   (`quarry-control` + `quarry-orchestrator`). `POST /v1/crawl`/`/v1/batch`
   hand off to `quarry-control`'s `/v1/jobs`, which drives a Temporal
   workflow that calls back into quarry-edge once per page
   (`/v1/internal/run_page`) for the actual browser/fetch work; Temporal
   itself owns retries, crash recovery, and history for the whole job.
   `PostgresRequestQueue` (`crates/quarry-runtime/src/postgres_queue.rs`)
   and its `RequestQueue` trait are a separate, fully-implemented,
   fully-tested abstraction that has *zero* production call sites anywhere
   in the workspace — not a config flag or missing feature-gate, just never
   constructed. `quarry-control`'s own `MountRequestQueues` endpoint is an
   explicit stub (`emptyPage()`) whose comment says a future "cycle 24"
   would surface it — that cycle apparently never happened. Wiring
   `PostgresRequestQueue` into `quarry-edge` today, the way this item
   originally asked, would build a second, fully disconnected queue that
   nothing reads from — dead weight, not the P0 fix this line claimed it
   was. Leave both implementations in place (removing them is a separate,
   bigger decision than this roadmap item), but do not deploy either one
   without first identifying a real Rust-native use case Temporal doesn't
   already cover.
3. **Fix Letta's silent-no-op `DeleteMemory`** (§3a) — a DSAR-relevant
   correctness bug: an erasure request can appear to succeed while the row
   remains, because the table has no per-user ownership column to scope the
   delete. Small, contained, high-consequence.
4. **Resolved 2026-08-04 — Session Core anti-replay nonce** (§3a). The
   deployed local Gateway and Control Session Core now require a single-use
   signed nonce for their delegated-user contract; the remaining higher-order
   P0 work is the approval-continuation dispatcher, not this replay gap.
5. **Resolved 2026-08-06 — the three-way `AUTHCTX_ENFORCE` contradiction.**
   `apps/Data Plane v2/docker-compose.yml:879` hardcodes `AUTHCTX_ENFORCE: "1"`
   (not an overridable default) with a "JWKS unavailability fails closed"
   comment; confirmed identical in the live `documents-api` container's
   actual env. `pkg/authctx/verify.go` is a complete RS256 + `kid`-resolved
   JWKS implementation (audience/issuer/expiry + `org_id`/`zdr`/principal
   claims checks) — not a stub. Live-tested directly against the running
   container: an unauthenticated request to `/v1/documents` returns `401`,
   a garbage bearer token also `401` — real enforce behavior, not a
   blanket 503. Verdict: **§6.7 was correct**; **§1.1 was stale** (observe
   mode exists in code but needs both `AUTHCTX_ENFORCE=0` *and*
   `ALLOW_INSECURE_DEV_DEFAULTS=1`, neither set here); **`Verevon-ai-first.md`
   was wrong** (a 503 only occurs if neither a static key file nor a JWKS
   URL is configured at all — not this deployment's state). Safe to assert
   documents-api-go enforces auth by default going forward.

### 9.3 Quick human-only unblocks (minutes, not engineering)

Three items last recorded open in `verevon-feature-map.md` §6.7/§6.8 (2026-07-20)
and not contradicted by anything since, each explicitly flagged in its own
source section as something an agent should not self-authorize: **(a)** the
Phase 0 live-smoke-test credential (supply the real `SEED_DEV_PASSWORD`, or
temporarily flip the dev-bypass flag, or do one manual login) — needed to
turn "verified by code inspection" into one live HTTP proof; **(b)** the
Aquatiq-AS demo-crawl trigger — no service-principal path can legitimately
ingest into a non-ZDR org, so a real Aquatiq-AS member must log in once and
run the crawl; **(c)** executing a real Gmail send from the now-working test
mailbox (§1 above, "Local provider-proof status — 2026-08-04") and retaining
its authoritative acceptance/delivery evidence. None of these need more engineering — they need
five minutes each
from someone with the right access, and each one is currently the sole thing
standing between "verified in source" and "verified live" for its area.

### 9.4 Start now, don't wait — the Verified Outcome Foundation (P1, §3b)

Define the shared `Effect`/`ExecutionReceipt`/`VerificationResult`/failure
taxonomy now rather than after 9.1-9.2 land, so the approval dispatcher and
Quarry's browser/fetch receipts converge on one contract instead of each
inventing its own. Ship the first Verevon Proof Bundle in the Agent Run
Console. This is also the exact precondition `verevon-inbox.md` names for its
own most-advanced ask (§9.5 item 5 below) — building it once now avoids
blocking two separate consumers later instead of one.

**Resolved 2026-08-04 (backend half) — the shared contract exists and has a
real, tested, durably-persisting first producer.** New
`model_plane/v1/verified_outcome.proto`: `Effect` (the generic "what
side-effecting action was attempted" envelope), `ExecutionReceipt` (what the
boundary itself reported — the raw, possibly-misleading claim), and
`VerificationResult` (`VerificationStatus`: `unknown` | `verified_success` |
`verified_failure` | `partially_verified`, plus a `method` field so a
mechanical check is never later confused with a real postcondition one).
Deliberately types-only and per-plane-persisted, not a shared cross-plane
table: Quarry lives in a different plane/database entirely, and this
platform has no cross-plane-DB mechanism (nor should it) — the contract that
converges is the proto shape, each domain still owns storing its own
instances.

`RecordApprovalContinuationOutcomeRequest` gained an optional `verification`
field (additive — an older caller omitting it stores NULLs, never a
fabricated default). The approval-delivery worker (§9.1(b)) is the first
real producer: `verification_result_for()` derives a `VerificationResult`
from the same `Disposition` it already computes — `Completed` with a
non-empty `provider_receipt_id` → `VERIFIED_SUCCESS`, `FailedRetryable` →
`VERIFIED_FAILURE`, method `"structural"` either way. **Read that word
literally**: this is receipt-id-presence, not an independent postcondition
check against the provider's own state (e.g. actually querying shipping-core
to confirm the booking exists) — that is separately-scoped future work (§3b
P1 item 3), and the `method` field exists specifically so today's mechanical
judgment is never later mistaken for that deeper one. session-core persists
it in `approval_continuation_outcomes` (migration `0020_verification_result.sql`,
3 new nullable columns, all-or-nothing via a CHECK constraint) — extending
the _existing_ receipt/outcome tables rather than inventing a parallel
"effects" schema, since they already are this domain's `Effect`+
`ExecutionReceipt` in substance. Verified: `buf generate` regenerated Rust/Go/
Python/TS bindings clean (an incidental, unrelated diff from floating `remote:`
buf plugin versions bumping across ~15 untouched files — cosmetic version-stamp
comments only, confirmed by inspection, not something this pass fixes); the
full Model Plane Rust workspace compiles; execution-core (23 approval_delivery_
worker tests, 3 new) and session-core (242 tests, 4 new) both pass; clippy
clean on every file touched.

**Not done — explicitly deferred, not silently skipped**: a read RPC/gateway
route/frontend surfacing (the "display it in the Agent Run Console" half of
this same roadmap line) — `approval_continuation_receipts`/`_outcomes` are
write-only today, nothing reads them back yet, and building a fetch path a
UI can render deserves its own visual verification pass rather than being
bolted on unverified at the end of an already-long session. Also deferred,
per the roadmap's own P1 ordering: postcondition verifiers (item 3),
stateful simulators/fault injection/shadow replay/CI gates (item 4), the
Agent Quality OS metrics suite (item 5), Surface/API/Agent parity tests
(item 6), and wiring Quarry's browser/fetch receipts onto this same contract
(a separate plane, not started here).

**Resolved 2026-08-04 (read path + console surfacing) — the deferred half
above is done: a verified outcome is now live on the Agent Run Console the
moment it's recorded, with no separate fetch.** Reused the existing
`OrchestrationEvent` stream rather than inventing a parallel read API — the
same mechanism `PlanTransitioned`/`ApprovalStateChanged`/etc. already use to
reach the console live, so this is one more oneof variant on a proven path,
not new plumbing:

- **New oneof variant** `ApprovalContinuationVerified` (tag 23, additive) in
  `orchestration.proto`, carrying `run_id`/`delivery_id`/`approval_id`/
  `receipt_id` plus the `VerificationResult` itself.
- **`record_continuation_outcome`** now returns the owning `run_id` on a
  fresh write (a second, indexed lookup by `receipt_id` — the
  `INSERT...SELECT...RETURNING` can't reach the joined receipt row's
  `run_id` directly), and the gRPC handler broadcasts the event through the
  same `broadcast_event`/replay-buffer path every other orchestration event
  already uses — only on a genuinely fresh write with a verification present,
  never on the idempotent-replay no-op path.
- **`mp-orchestration`'s native mirror** (the mirror this crate already
  maintains for every event type, used by its replay buffer) gained a
  matching `VerificationStatus` enum and `ApprovalContinuationVerified`
  variant, with full proto round-trip + rejection tests (unspecified status,
  missing verification result).
- **model-gateway's SSE layer**: the wire event name
  (`approval_continuation_verified`), the JSON `data:` payload, and chat's
  own unified `step_update` projection (status `done|failed|partial|unknown`
  → localized labels) all handle the new variant.
- **verevonv3**: `run-console-client.ts` parses the event
  (`onApprovalContinuationVerified`); `AgentRunConsole.tsx` renders it as a
  new `verification` timeline row (`ShieldCheck` icon, the verification's own
  `reason` as detail, correctly tinted — a `verified_success` reads as green,
  matching the rest of the timeline's status-tone convention, not conflated
  with a tool/step's unrelated plain `'done'`).

**Verified**: full Model Plane Rust workspace compiles; all 5 Go services
consuming the regenerated proto build clean; Python bindings parse; the
combined `session-core`/`model-gateway`/`mp-orchestration`/`execution-core`
test suite passes in full (no regressions, new round-trip/rejection/broadcast
tests included); clippy clean on every file touched; verevonv3 `pnpm
typecheck` and `pnpm lint` both clean (0 errors); the new
`run-console-client.test.ts` case passes.

**Resolved 2026-08-05 — live browser render confirmed, plus 6 pre-existing
bugs found and fixed along the way.** Drove a real shipping-booking task
(`get_shipping_quotes` → HITL pause on `book_shipment` → grant → cold-resume
continuation) end-to-end through the actual running dev stack and confirmed
via screenshot that the Agent Run Console renders a live `Verified` timeline
row (`ShieldCheck` icon, reason text, correctly tinted status badge) the
moment `ApprovalContinuationVerified` broadcasts — no unit test, the real SSE
event landing in a real browser tab. The observed outcome was `FAILED /
transient_dependency` (the model picked "helthjem," a mock-only carrier, as
cheapest; shipping-core correctly refuses to book a carrier with no real
integration) — a **correct** verified-failure, not a defect; it exercises the
timeline's pre-existing fail-tone path. The success-tone path
(`verified_success`, the one genuinely new branch added to
`normalizeStatusTone`) was not separately exercised — doing so would require
steering the model onto a real carrier (Bring/DHL/UPS/FedEx) and accepting an
actual booking side effect, which was deliberately left to the user's call
rather than forced.

Getting to that one screenshot required finding and fixing six independent,
pre-existing bugs the whole chain had never been exercised against before:

1. `capability-core`'s `AUTH_CORE_ISSUER` env was stale in the running
   container (an earlier session had already fixed `.env` but never
   recreated the container) — every capability-core auth check, including
   `cap.command.shell`/`cap.command.sandbox` health attestation, was silently
   failing closed. Recreating the container fixed it immediately.
2. `shipping-core`'s `AUTH_CORE_URL`/`AUTH_CORE_JWKS_URL`/`PLANE_TOKEN_ISSUER`
   all defaulted to an `auth-core:3011` hostname unreachable from its own
   docker network (it needs `host.docker.internal`), and the issuer value
   didn't match what Auth Core actually stamps (`http://localhost:3011/...`,
   a fixed self-referential value, not derived from the caller). Same root
   cause as #1, a second, independent occurrence.
3. Nothing anywhere ever attested `cap.tool.shipping.read`/`.book`'s runtime
   health — `execution-core`'s own `health_attest.rs` is deliberately scoped
   to only the two capabilities it's the runtime authority for. Built a new
   `shipping-core/internal/capabilityhealth` package (mirrors
   `health_attest.rs`'s pattern exactly: real synthetic-quote probe every
   2 minutes, attest only on genuine non-mock-carrier success, never blind)
   plus the matching `shipping-core` service-principal registry entry.
4. Auth Core's principal registry had no `shipping-core` and no `ingestion`
   audience for the (unrelated, initially-assumed) `execution-core`
   principal — a red herring; see #6.
5. Shipping-core's own scope check (`serviceScopeAllows`) treated every
   non-GET method as write-only, but `POST /api/quotes` (and
   `/api/quotes/recommend`) are read-shaped queries-with-a-body, not
   mutations — made the check path-aware for exactly those two routes.
6. **The one that actually gated `get_shipping_quotes`/`book_shipment`**:
   `shipping_tools.rs`'s `ShippingToolsClient` mints its shipping-core tokens
   under the `INGESTION_SERVICE_ID`/`_API_KEY` identity (`model-execution`),
   not `EXECUTION_CORE_SERVICE_ID` — a different principal than the one #4
   extended. `model-execution` had `shipping:read` but not `shipping:write`,
   so every booking attempt 403'd at token-mint time, before shipping-core
   ever saw a request; the generic `Err(_) => transient_dependency`
   classification in `disposition_for_shipment_booking_result` swallowed the
   real reason, making it look identical to a genuine transient failure.
   Added `shipping:write` to `model-execution`'s scopes (and corrected its
   retention posture from `zdr` to `persistent` — a real booking is
   durable, and `zdr` would have silently suppressed the post-delivery
   evidence writeback).

Also fixed, unrelated to the render but on the same critical path: the
persisted approval continuation descriptor (`pause_for_approval` →
`continuation_descriptor` in `runtime_loop/agent.rs`) never captured
`book_shipment`'s `booked_by` field — it's system-injected (the run's acting
user), never a model-supplied tool argument, and the live/same-session
resume path (`execute_book_shipment`) injects it in-memory but the
_persisted_ descriptor never got the same treatment. Every cold-resume
`book_shipment` continuation failed closed with `invalid_continuation` until
this was fixed at the source (2 new tests: injects when absent, preserves
when the caller does supply one).

**Resolved 2026-08-05 (same day, later) — the `verified_success` path,
exercised for real.** At the user's explicit request, steered a fresh run
onto Bring specifically instead of leaving carrier choice to the model. That
surfaced a genuine, previously-unknown product gap: Bring's real booking API
rejects a recipient with neither `email` nor `phone` (`BOOK_VALIDATION-011`)
— confirmed against `developer.bring.com`'s booking schema, not guessed. The
model has no way to know a contact for a recipient it has never met, so at
the user's direction the fix sources it from the run's own acting user via
Control Plane's `user-core` (this system's actual use today: the acting user
is the real recipient for every booking it places) — primary email always,
phone too if user-core has one on file. Built a minimal hand-rolled
`user_core_client.rs` (forward-compatible partial-field `prost::Message`
structs, authenticated via `user-core`'s gRPC static-credential scheme) and
wired it into both `continuation_descriptor` (pre-pause enrichment, so a
cold-resumed booking carries the same contact a live one would) and
`execute_book_shipment` (the live path).

The first live retry surfaced a 7th genuine bug, caught before it could
waste a second real-carrier cycle: `get_contact`'s doc comment promised
"never returns `Err` for 'not found' or 'no profile'" but `get_phone`
propagated _any_ `tonic::Status` — including a plain `NotFound` for a user
with no profile row — as a hard error via `?`, which silently discarded the
email `get_contact` had _already_ fetched. Confirmed live: the log read
`book_shipment: could not enrich recipient contact from user-core` with a
`GetUserProfile ... NotFound` cause, and the persisted continuation
descriptor's `to` object had no `email` at all despite the user definitely
having one. Fixed by matching `tonic::Code::NotFound` in both `get_email`
and `get_phone` to `Ok(None)` instead of `Err`, matching the contract the
doc comment already promised.

Rebuilt, redeployed, resubmitted — confirmed end-to-end via direct
inspection, not just the UI:

- Persisted continuation descriptor's `to.email` = the acting user's real
  user-core email (`e2e@verevon.dev` for this dev account); no `phone` key,
  correctly omitted since user-core has none on file for this user.
- `shipping_core.bookings`: exactly one row, `status = 'booked'`,
  `carrier_code = 'bring'`, a real tracking number (`LC652849244NO`) and
  booking ref, keyed by an `idempotency_key` of `{run_id}:{step_id}`.
- `approval_continuation_outcomes` (joined via `approval_continuation_receipts`
  on the approval id): `outcome = 'completed'`, **`verification_status =
'verified_success'`** — the one branch the earlier same-day entry above
  left unexercised — `verification_method = 'structural'`,
  `provider_receipt_id` matching the booking row exactly.
- The live browser DOM (`get_page_text`, mid-run) rendered `Verified / DONE /
provider returned authoritative receipt id <id>` for this same id.

Both the `verified_failure` (real carrier refused, earlier same-day entry)
and `verified_success` (real carrier accepted, this entry) branches of the
Verified Outcome Foundation are now proven against the live stack, not just
unit-tested.

**Finding from this same incident — now fixed (2026-08-07):** the approval
"decide" (`POST /api/v1/orchestration/approvals/{id}/decide`) request that
drove this exact approval returned `502 Bad Gateway` to the browser (gateway
logs show a cluster of 502/503s with 30-55s latency in the same window,
correlated with host CPU/disk contention from the `execution-core` image
rebuild moments earlier — not a code defect in the approval path itself).
The frontend surfaced "Kunne ikke registrere avgjørelsen din — prøv igjen"
("could not register your decision — try again") and left a `Gjenoppta
kjøring` (resume) affordance, even though the request had _already_ reached
execution-core and fully completed server-side (the DB rows above prove it).
A user hitting this would have every reason to retry a decision that already
succeeded. It didn't cause a duplicate booking here only because
`bookings`'s `idx_bookings_org_idempotency` unique index
(`org_id, idempotency_key`) would reject a second insert for the same
`{run_id}:{step_id}` — a real safety net, but the UI-level confusion (and
the wasted duplicate _delivery attempt_ the retry would still trigger before
hitting that constraint) was worth closing separately, and now is: two
merged fixes now on `main` close both the deterministic and transient cases.
`model-gateway`'s `decide_approval` handler (`http_routes.rs`) no longer
propagates the `quarantine_granted_approval_continuation` placeholder's
`Err` as an HTTP failure — a granted approval's outcome is durable the
moment `DecideApproval` returns, so the placeholder's result now only fills
an informational `continuation_delivery` field on an otherwise-`Ok`
response (merge `b6736ebd`). Separately, `verevonv3`'s gateway gained
`mg_post_decide_with_retry` (`apps/gateway/src/domains/orchestration.rs`) —
a bounded retry around exactly this class of transient 502/503 — paired
with `AgentRunConsole.tsx`'s `applyDecisionFollowThrough`, which reconciles
the UI against the server's actual state instead of asserting failure on a
retryable response (merge `60198193`). Together these mean neither a
deterministic placeholder quirk nor ordinary host-load transients can any
longer show a user a false "could not register your decision."

### 9.5 Fold in — `verevon-inbox.md`'s own gates, now a standing checklist

`verevon-inbox.md` is current and extensive, but its "Delivery gates still
required" and "Definition of done for the complete loop" were not yet
cross-referenced as a checklist from this roadmap. In the source doc's own
priority order:

1. Live provider-backed acceptance/delivery/bounce proof — blocked only on
   9.3(c); everything else in this category is shipped.
2. Bounded relationship/context proposals, then merge/split/reopen and
   controlled customer communications — each deliberately gated on an
   explicit policy + preview that does not exist yet.
3. A structured Chat↔Ticketing handoff receipt — waiting on the Chat team's
   side of the contract; a cross-team dependency, not an Inbox-side gap.
4. Autopilot/Proactive modes and macro/action bundles — deliberately not
   started; Review-mode-only remains correct until a separately governed
   policy for them exists.
5. Semantic recurrence / knowledge-gap clustering — correctly blocked on
   §9.4 landing first. Both docs independently name the same dependency,
   which is a consistency check that passed, not a new finding.

### 9.6 Not engineering — decisions this roadmap is blocked on, not tasks

- The Telenor AI Factory sovereignty claim — hold for deal close (`verevon-vision.md`
  §3), not a build item.
- Data Plane erasure/DSAR purge scope (private-only vs. everything the user
  owns regardless of visibility) — needs a product/legal call before the
  cross-plane purge subscriber gets written at all.
- Data Plane credential rotation — operator/secrets-manager access,
  explicitly out of agent scope by design.
- convex-core's fate (fix its placeholder HMAC vs. retire the service) —
  decide, then build; do not build before deciding.
- The 6-plane-topology-for-3-people sustainability question
  (`Verevon-ai-first.md`'s minority report) — a team/founder call, not a
  sprint item.

### 9.7 Everything above stays local

Per the 2026-08-03/04 local-Docker banners now present in all six documents:
rebuild, apply migrations, verify the real flow, fix what breaks, repeat.
None of the above is prerequisite work for an external launch, and none of
it should be read as one.
