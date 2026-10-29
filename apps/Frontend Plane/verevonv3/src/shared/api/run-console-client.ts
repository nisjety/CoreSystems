import { requestJson } from './http'
import { readSseStream, type SseEvent } from './sse'

// ── Wire contract ───────────────────────────────────────────────────────────
// `GET /api/v1/runs/:run_id/events` is an SSE stream of an agentic run's live
// lifecycle (plans, todos, approvals, sub-agents, browser actions) plus a
// unified human-friendly `step_update` view. The gateway forwards the
// model-gateway / session-core / execution-core events verbatim (snake_case).
// This client owns the wire→logical translation so the run console renders
// camelCase, parse-defensively (snake_case OR camelCase) like the rest of the
// API layer (see orchestration-client `normalizeApproval`).
//
// The endpoint supports `Last-Event-ID` resume, so passing `lastEventId`
// reconnects without replaying events the console already saw.

// ── Typed events (camelCase) ─────────────────────────────────────────────────

export type PlanTransitionedEvent = {
  planId?: string
  runId?: string
  from?: string
  to?: string
  at?: string
}

export type TodoTransitionedEvent = {
  todoId?: string
  threadId?: string
  from?: string
  to?: string
  at?: string
}

export type ApprovalStateChangedEvent = {
  approvalId?: string
  runId?: string
  approvalKind?: string
  to?: string
  decidedBy?: string
  at?: string
}

export type SubagentAttachedEvent = {
  parentRunId?: string
  childRunId?: string
  role?: string
  at?: string
}

export type SubagentStoppedEvent = {
  childRunId?: string
  status?: string
  at?: string
}

export type RunPausedForApprovalEvent = {
  runId?: string
  approvalId?: string
  at?: string
}

export type RunResumedAfterApprovalEvent = {
  runId?: string
  approvalId?: string
  at?: string
}

export type BrowserActionDispatchedEvent = {
  runId?: string
  planId?: string
  actionId?: string
  actionType?: string
  url?: string
  /** The model's rationale for choosing this action (Phase 2). Empty for the
   * deterministic fallback (no LLM reason available). */
  reason?: string
  at?: string
}

export type BrowserObservationReceivedEvent = {
  runId?: string
  planId?: string
  actionId?: string
  status?: string
  pageUrl?: string
  pageTitle?: string
  /** Artifact reference id (Phase 2) — never inlined bytes; fetch evidence via
   * the existing artifact endpoint. */
  screenshotRef?: string
  domSnapshotRef?: string
  at?: string
}

/** User-initiated browser-run pause/resume (Phase 2 B5) — distinct from the
 * HITL-approval `RunPausedForApprovalEvent`/`RunResumedAfterApprovalEvent`. */
export type BrowserRunPausedEvent = {
  runId?: string
  planId?: string
  at?: string
}

export type BrowserRunResumedEvent = {
  runId?: string
  planId?: string
  at?: string
}

/** Phase 5 HITL gate: a specific browser action (or a run-level condition
 * like persistent-cookie reuse) requires a human decision before it
 * proceeds. `approvalId` lets the UI decide it via the existing generic
 * `POST /api/v1/orchestration/approvals/:id/decide` route — this event only
 * adds the browser-specific "what and why" the generic `Approval` record
 * doesn't carry. `actionId` is empty for a run-level gate (e.g.
 * persistent_cookie_use, gated once at run start). */
export type BrowserActionApprovalRequiredEvent = {
  runId?: string
  planId?: string
  actionId?: string
  actionType?: string
  url?: string
  selector?: string
  reason?: string
  /** login|checkout|posting_form|destructive|cross_domain_navigation|persistent_cookie_use */
  riskCategory?: string
  approvalId?: string
  at?: string
}

/** Phase 5 HITL gate: the companion "what happened to it" event for a
 * `BrowserActionApprovalRequiredEvent` — granted resumes the same action
 * unchanged, denied/timed-out aborts the run. */
export type BrowserActionDecidedEvent = {
  runId?: string
  planId?: string
  actionId?: string
  approvalId?: string
  /** granted|denied|timed_out */
  decision?: string
  decidedBy?: string
  at?: string
}

export type RunStepEvent = {
  id?: string
  title?: string
  detail?: string
  status?: string
}

/** Verified Outcome Foundation (verevon-roadmap.md §3b): a resumed
 * continuation's independent verification, live on the run's own event
 * stream. `verificationStatus` is one of `verified_success` |
 * `verified_failure` | `partially_verified` | `unknown` — never
 * `unspecified`, since the server only emits this event once a real
 * verification was produced. */
export type ApprovalContinuationVerifiedEvent = {
  runId?: string
  deliveryId?: string
  approvalId?: string
  receiptId?: string
  verificationStatus?: string
  verificationMethod?: string
  verificationReason?: string
  at?: string
}

export type RunEventHandlers = {
  onPlan?: (event: PlanTransitionedEvent) => void
  onTodo?: (event: TodoTransitionedEvent) => void
  onApproval?: (event: ApprovalStateChangedEvent) => void
  onRunPaused?: (event: RunPausedForApprovalEvent) => void
  onRunResumed?: (event: RunResumedAfterApprovalEvent) => void
  onSubagentAttached?: (event: SubagentAttachedEvent) => void
  onSubagentStopped?: (event: SubagentStoppedEvent) => void
  onBrowserAction?: (event: BrowserActionDispatchedEvent) => void
  onBrowserObservation?: (event: BrowserObservationReceivedEvent) => void
  onBrowserRunPaused?: (event: BrowserRunPausedEvent) => void
  onBrowserRunResumed?: (event: BrowserRunResumedEvent) => void
  onBrowserActionApprovalRequired?: (event: BrowserActionApprovalRequiredEvent) => void
  onBrowserActionDecided?: (event: BrowserActionDecidedEvent) => void
  onApprovalContinuationVerified?: (event: ApprovalContinuationVerifiedEvent) => void
  onStep?: (event: RunStepEvent) => void
  onError?: (err: unknown) => void
  onDone?: () => void
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

function dispatch(event: SseEvent, handlers: RunEventHandlers): void {
  if (!event.data) return
  let payload: Record<string, unknown>
  try {
    payload = JSON.parse(event.data) as Record<string, unknown>
  } catch {
    return
  }

  switch (event.event) {
    case 'plan_transitioned':
      handlers.onPlan?.({
        planId: str(payload.plan_id) ?? str(payload.planId),
        runId: str(payload.run_id) ?? str(payload.runId),
        from: str(payload.from),
        to: str(payload.to),
        at: str(payload.at),
      })
      break
    case 'todo_transitioned':
      handlers.onTodo?.({
        todoId: str(payload.todo_id) ?? str(payload.todoId),
        threadId: str(payload.thread_id) ?? str(payload.threadId),
        from: str(payload.from),
        to: str(payload.to),
        at: str(payload.at),
      })
      break
    case 'approval_state_changed':
      handlers.onApproval?.({
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        runId: str(payload.run_id) ?? str(payload.runId),
        approvalKind: str(payload.approval_kind) ?? str(payload.approvalKind) ?? str(payload.kind),
        to: str(payload.to) ?? str(payload.state) ?? str(payload.status),
        decidedBy: str(payload.decided_by) ?? str(payload.decidedBy),
        at: str(payload.at),
      })
      break
    case 'subagent_attached':
      handlers.onSubagentAttached?.({
        parentRunId: str(payload.parent_run_id) ?? str(payload.parentRunId),
        childRunId: str(payload.child_run_id) ?? str(payload.childRunId),
        role: str(payload.role),
        at: str(payload.at),
      })
      break
    case 'subagent_stopped':
      handlers.onSubagentStopped?.({
        childRunId: str(payload.child_run_id) ?? str(payload.childRunId),
        status: str(payload.status),
        at: str(payload.at),
      })
      break
    case 'run_paused_for_approval':
      handlers.onRunPaused?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        at: str(payload.at),
      })
      break
    case 'run_resumed_after_approval':
      handlers.onRunResumed?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        at: str(payload.at),
      })
      break
    case 'browser_action_dispatched':
      handlers.onBrowserAction?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        actionId: str(payload.action_id) ?? str(payload.actionId),
        actionType: str(payload.action_type) ?? str(payload.actionType),
        url: str(payload.url),
        reason: str(payload.reason),
        at: str(payload.at),
      })
      break
    case 'browser_observation_received':
      handlers.onBrowserObservation?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        actionId: str(payload.action_id) ?? str(payload.actionId),
        status: str(payload.status),
        pageUrl: str(payload.page_url) ?? str(payload.pageUrl),
        pageTitle: str(payload.page_title) ?? str(payload.pageTitle),
        screenshotRef: str(payload.screenshot_ref) ?? str(payload.screenshotRef),
        domSnapshotRef: str(payload.dom_snapshot_ref) ?? str(payload.domSnapshotRef),
        at: str(payload.at),
      })
      break
    case 'browser_run_paused':
      handlers.onBrowserRunPaused?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        at: str(payload.at),
      })
      break
    case 'browser_run_resumed':
      handlers.onBrowserRunResumed?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        at: str(payload.at),
      })
      break
    case 'browser_action_approval_required':
      handlers.onBrowserActionApprovalRequired?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        actionId: str(payload.action_id) ?? str(payload.actionId),
        actionType: str(payload.action_type) ?? str(payload.actionType),
        url: str(payload.url),
        selector: str(payload.selector),
        reason: str(payload.reason),
        riskCategory: str(payload.risk_category) ?? str(payload.riskCategory),
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        at: str(payload.at),
      })
      break
    case 'browser_action_decided':
      handlers.onBrowserActionDecided?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        planId: str(payload.plan_id) ?? str(payload.planId),
        actionId: str(payload.action_id) ?? str(payload.actionId),
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        decision: str(payload.decision),
        decidedBy: str(payload.decided_by) ?? str(payload.decidedBy),
        at: str(payload.at),
      })
      break
    case 'approval_continuation_verified':
      handlers.onApprovalContinuationVerified?.({
        runId: str(payload.run_id) ?? str(payload.runId),
        deliveryId: str(payload.delivery_id) ?? str(payload.deliveryId),
        approvalId: str(payload.approval_id) ?? str(payload.approvalId),
        receiptId: str(payload.receipt_id) ?? str(payload.receiptId),
        verificationStatus: str(payload.verification_status) ?? str(payload.verificationStatus),
        verificationMethod: str(payload.verification_method) ?? str(payload.verificationMethod),
        verificationReason: str(payload.verification_reason) ?? str(payload.verificationReason),
        at: str(payload.at),
      })
      break
    case 'step_update':
      handlers.onStep?.({
        id: str(payload.id) ?? str(payload.step_id),
        title: str(payload.title) ?? str(payload.name),
        detail: str(payload.detail) ?? str(payload.message),
        status: str(payload.status),
      })
      break
    // Unknown event names are ignored gracefully.
  }
}

/**
 * Stream an agentic run's console events from the gateway. Mirrors
 * `streamChat`: surfaces a connection-level error to `handlers.onError` even
 * though the SSE reader itself reports connection errors out-of-band.
 *
 * @param runId       The orchestration run id (from the chat `connected` event).
 * @param handlers    Per-event callbacks; all optional.
 * @param signal      Abort signal to tear down the stream.
 * @param lastEventId Resume token — replays only events after this id.
 */
export async function streamRunEvents(
  runId: string,
  handlers: RunEventHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    `/api/v1/runs/${encodeURIComponent(runId)}/events`,
    { method: 'GET', signal, lastEventId },
    (event) => dispatch(event, handlers),
    (err) => {
      connError = err
    },
    handlers.onDone,
  )

  if (connError) {
    handlers.onError?.(connError)
  }
}

// ── Verevon Proof Bundle ────────────────────────────────────────────────────
// `GET /api/v1/orchestration/runs/:run_id/proof-bundle` is the run's portable
// evidence record: what a human authorized, what execution-core actually
// started, how it finalized, and whether anything independently verified it.
//
// The bundle's whole value is that it distinguishes "not proven" from
// "proven false". Every nested record below is therefore `T | null`, never
// optional-and-collapsed: `null` is a first-class answer meaning the evidence
// for that dimension does not exist, and a renderer must show it as neither
// success nor failure. The normalizers below fold a missing key and an
// explicit JSON `null` onto the same `null` — both mean "not recorded" — but
// never invent an empty object to stand in for one.

/** An independent judgment of an execution's real-world outcome. */
export type ProofVerification = {
  /** `verified_success` | `verified_failure` | `partially_verified` | `unknown`
   * — the same wire values `ApprovalContinuationVerifiedEvent` carries. */
  status?: string
  /** How it was judged, e.g. `structural`. */
  method?: string
  /** Short human-readable justification (often a provider receipt id). */
  reason?: string
  verifiedAt?: string
}

/** How an execution finalized. Present only once the work stopped running. */
export type ProofOutcome = {
  /** `completed` | `failed` | `cancelled`. */
  outcome?: string
  providerReceiptId?: string
  failureCode?: string
  finalizedAt?: string
  /** `null` when nothing independently verified this outcome. That is NOT a
   * failed verification and NOT an implied success — it means no verification
   * was ever recorded. */
  verification: ProofVerification | null
}

/** Proof that work actually started for an approval. */
export type ProofExecution = {
  receiptId?: string
  deliveryId?: string
  /** 64-hex digest binding the receipt to the exact action descriptor. */
  actionFingerprint?: string
  executionServiceId?: string
  descriptorVersion?: number
  startedAt?: string
  /** `null` when the work genuinely started and has not finalized — in
   * flight, not failed. */
  outcome: ProofOutcome | null
}

/** One authorization decision plus whatever it can prove followed from it. */
export type ProofApproval = {
  approvalId?: string
  kind?: string
  status?: string
  requestedBy?: string
  decidedBy?: string
  decisionReason?: string
  requestedAt?: string
  decidedAt?: string
  /** `null` when the approval was decided but no work was ever proven to
   * start. That is NOT a failed execution. */
  execution: ProofExecution | null
}

/** An evidence dimension the bundle deliberately does not claim (e.g. what the
 * run knew, what it cost, what retention applied), with the reason it is
 * absent. Always surfaced — an unmade claim is part of the record. */
export type ProofUnavailableSection = {
  section?: string
  reason?: string
}

export type ProofRunSummary = {
  goal?: string
  agentId?: string
  status?: string
  createdAt?: string
}

export type ProofBundle = {
  bundleVersion?: number
  runId?: string
  orgId?: string
  generatedAt?: string
  run: ProofRunSummary | null
  approvals: ProofApproval[]
  unavailable: ProofUnavailableSection[]
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined
}

/** Narrow to a plain record; an explicit `null` (or any non-object) stays absent. */
function record(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

function normalizeVerification(raw: unknown): ProofVerification | null {
  const value = record(raw)
  if (!value) return null
  return {
    status: str(value.status),
    method: str(value.method),
    reason: str(value.reason),
    verifiedAt: str(value.verified_at) ?? str(value.verifiedAt),
  }
}

function normalizeOutcome(raw: unknown): ProofOutcome | null {
  const value = record(raw)
  if (!value) return null
  return {
    outcome: str(value.outcome),
    providerReceiptId: str(value.provider_receipt_id) ?? str(value.providerReceiptId),
    failureCode: str(value.failure_code) ?? str(value.failureCode),
    finalizedAt: str(value.finalized_at) ?? str(value.finalizedAt),
    verification: normalizeVerification(value.verification),
  }
}

function normalizeExecution(raw: unknown): ProofExecution | null {
  const value = record(raw)
  if (!value) return null
  return {
    receiptId: str(value.receipt_id) ?? str(value.receiptId),
    deliveryId: str(value.delivery_id) ?? str(value.deliveryId),
    actionFingerprint: str(value.action_fingerprint) ?? str(value.actionFingerprint),
    executionServiceId: str(value.execution_service_id) ?? str(value.executionServiceId),
    descriptorVersion: num(value.descriptor_version) ?? num(value.descriptorVersion),
    startedAt: str(value.started_at) ?? str(value.startedAt),
    outcome: normalizeOutcome(value.outcome),
  }
}

function normalizeApproval(raw: unknown): ProofApproval | null {
  const value = record(raw)
  if (!value) return null
  return {
    approvalId: str(value.approval_id) ?? str(value.approvalId),
    kind: str(value.kind),
    status: str(value.status),
    requestedBy: str(value.requested_by) ?? str(value.requestedBy),
    decidedBy: str(value.decided_by) ?? str(value.decidedBy),
    decisionReason: str(value.decision_reason) ?? str(value.decisionReason),
    requestedAt: str(value.requested_at) ?? str(value.requestedAt),
    decidedAt: str(value.decided_at) ?? str(value.decidedAt),
    execution: normalizeExecution(value.execution),
  }
}

function normalizeUnavailable(raw: unknown): ProofUnavailableSection | null {
  const value = record(raw)
  if (!value) return null
  return { section: str(value.section), reason: str(value.reason) }
}

function normalizeRunSummary(raw: unknown): ProofRunSummary | null {
  const value = record(raw)
  if (!value) return null
  return {
    goal: str(value.goal),
    agentId: str(value.agent_id) ?? str(value.agentId),
    status: str(value.status),
    createdAt: str(value.created_at) ?? str(value.createdAt),
  }
}

function list(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

function normalizeBundle(raw: unknown): ProofBundle | null {
  const value = record(raw)
  if (!value) return null
  return {
    bundleVersion: num(value.bundle_version) ?? num(value.bundleVersion),
    runId: str(value.run_id) ?? str(value.runId),
    orgId: str(value.org_id) ?? str(value.orgId),
    generatedAt: str(value.generated_at) ?? str(value.generatedAt),
    run: normalizeRunSummary(value.run),
    approvals: list(value.approvals)
      .map(normalizeApproval)
      .filter((item): item is ProofApproval => item !== null),
    unavailable: list(value.unavailable)
      .map(normalizeUnavailable)
      .filter((item): item is ProofUnavailableSection => item !== null),
  }
}

/**
 * Fetch a run's Verevon Proof Bundle. Read-only; model-gateway scopes it to
 * the caller's verified org.
 *
 * Resolves to `null` when the run has no bundle at all — distinct from a
 * bundle that exists but proves little, which comes back populated with the
 * `null` fields and `unavailable` reasons that say so.
 *
 * @param runId  The orchestration run id.
 * @param signal Abort signal, for teardown when the console re-pins.
 */
export async function getRunProofBundle(runId: string, signal?: AbortSignal): Promise<ProofBundle | null> {
  const payload = await requestJson<{ bundle?: unknown }>(
    `/api/v1/orchestration/runs/${encodeURIComponent(runId)}/proof-bundle`,
    { signal },
  )
  // The gateway forwards model-gateway's `{ bundle }` envelope verbatim;
  // tolerate a bare bundle body too, like the rest of the API layer.
  const envelope = record(payload)
  if (!envelope) return null
  return normalizeBundle('bundle' in envelope ? envelope.bundle : envelope)
}
