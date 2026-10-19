import { ApiError, requestJson } from './http'

// ── Wire contract ───────────────────────────────────────────────────────────
// Phase 2 (durable browser-agent run): starting/controlling an AI browser
// loop is a thin trigger over the same durable-run backbone chat agentic
// runs already use — progress streams via `streamRunEvents` (see
// `run-console-client.ts`), not from this module. Kept separate from
// `browser-client.ts` (manual browsing / one-shot suggestion surface) the
// same way `run-console-client.ts` is kept separate from `chat-client.ts`.

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-verevon-org-id': orgId }
}

export type StartBrowserAiRunParams = {
  goal: string
  allowedDomains?: string[]
  maxSteps?: number
  maxRuntimeSeconds?: number
  stopCriteria?: string
  requireApproval?: boolean
  maxCostUsd?: number
}

export type StartBrowserAiRunResponse = {
  runId: string
  threadId: string
  planId: string
}

export type BrowserAiRunControlAction = 'pause' | 'resume' | 'stop'

export type StartBrowserAiRunOptions = {
  /** Sent as the `Idempotency-Key` header. The gateway hands it to Model Gateway,
   * which answers a repeat of the same start with the same run and the same
   * derived grant instead of a second run. Omitted: no header, a plain start. */
  idempotencyKey?: string
}

/** The gateway's own shape for the header (`client_idempotency_key` in
 * `apps/gateway/src/domains/browser.rs`): 1 to 128 of `A-Z a-z 0-9 - _ . :`.
 * Checked here as well so a malformed key is a local mistake, not a round trip. */
const IDEMPOTENCY_KEY_SHAPE = /^[A-Za-z0-9._:-]{1,128}$/

export function isValidAiRunIdempotencyKey(key: string): boolean {
  return IDEMPOTENCY_KEY_SHAPE.test(key)
}

function newAiRunIdempotencyKey(): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`
  return `ai-run-${random}`
}

/** Whether a failed start may nevertheless have started a run, so that the same
 * start has to be retried under the same key. A reply with a 4xx status is a
 * definite "no run" (that includes the 409 key refusals); no reply at all, a
 * timeout and any 5xx (the 503 `browser_idempotency_unverified` among them) leave
 * it open. */
export function isAmbiguousAiRunStartFailure(failure: unknown): boolean {
  if (failure instanceof ApiError) return failure.status === 408 || failure.status >= 500
  return true
}

export type AiRunStartKeyring = {
  /** The key for starting `params` on `sessionId`: the one held from an ambiguous
   * failure of this same start, otherwise a fresh one. */
  keyFor: (sessionId: string, params: StartBrowserAiRunParams) => string
  /** The start was accepted: the next start is a new submit and gets a fresh key. */
  succeeded: () => void
  /** The start failed. The key is kept only when the failure is ambiguous, so the
   * person's retry of the same start reaches the same run; any definite answer
   * drops it (a 409 says in as many words that a new key is needed). */
  failed: (failure: unknown) => void
}

function startFingerprint(sessionId: string, params: StartBrowserAiRunParams): string {
  return JSON.stringify([
    sessionId,
    params.goal.trim(),
    params.allowedDomains ?? null,
    params.maxSteps ?? null,
    params.maxRuntimeSeconds ?? null,
    params.stopCriteria ?? null,
    params.requireApproval ?? null,
    params.maxCostUsd ?? null,
  ])
}

/** One keyring per composer: it remembers the key of the start that last failed
 * ambiguously, so a retry is one logical start however many requests it takes. */
export function createAiRunStartKeyring(): AiRunStartKeyring {
  let held: { fingerprint: string; key: string } | null = null
  return {
    keyFor(sessionId, params) {
      const fingerprint = startFingerprint(sessionId, params)
      if (held?.fingerprint !== fingerprint) held = { fingerprint, key: newAiRunIdempotencyKey() }
      return held.key
    },
    succeeded() {
      held = null
    },
    failed(failure) {
      if (!isAmbiguousAiRunStartFailure(failure)) held = null
    },
  }
}


/** Wire shape of the `ai-runs` response: model-gateway's `browser_run.rs`
 * constructs this JSON directly (`json!({ "run_id", "thread_id", "plan_id" })`)
 * and the Verevon gateway forwards it verbatim (`start_ai_run` in
 * `apps/gateway/src/domains/browser.rs` wraps it in the `{ data }` envelope
 * but does not rename fields) — snake_case, not camelCase. */
type StartBrowserAiRunWireResponse = {
  run_id: string
  thread_id: string
  plan_id: string
}

/** `POST /api/v1/browser/sessions/:sessionId/ai-runs` — start a durable,
 * server-side, multi-step browser-agent run for the given browser session.
 * The response's `runId` feeds `streamRunEvents` for live progress.
 *
 * Maps the wire response's snake_case fields onto this module's camelCase
 * `StartBrowserAiRunResponse` explicitly — `requestJson<T>`'s generic only
 * asserts a TypeScript shape, it never transforms the actual JSON, so
 * declaring `{ runId, threadId, planId }` as the generic here previously
 * left every field `undefined` at runtime. That silently broke every
 * "AI-loop" run started through the SPA: `streamRunEvents(run.runId, ...)`
 * was called with `undefined`, so the client never received a single
 * progress event for a run it had, in fact, started successfully
 * server-side — including the Phase 5 HITL approval-required event, which
 * is why the UI looked stuck at "suggesting" forever with no visible error. */
export async function startBrowserAiRun(
  orgId: string,
  sessionId: string,
  params: StartBrowserAiRunParams,
  signal?: AbortSignal,
  options?: StartBrowserAiRunOptions,
): Promise<StartBrowserAiRunResponse> {
  const idempotencyKey = options?.idempotencyKey
  if (idempotencyKey !== undefined && !isValidAiRunIdempotencyKey(idempotencyKey)) {
    throw new Error('Invalid idempotency key for the AI run start')
  }
  const wire = await requestJson<StartBrowserAiRunWireResponse>(
    `/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/ai-runs`,
    {
      method: 'POST',
      body: JSON.stringify({
        goal: params.goal,
        allowedDomains: params.allowedDomains,
        maxSteps: params.maxSteps,
        maxRuntimeS: params.maxRuntimeSeconds,
        stopCriteria: params.stopCriteria,
        requireApproval: params.requireApproval,
        maxCostUsd: params.maxCostUsd,
      }),
      headers: idempotencyKey === undefined ? orgHeaders(orgId) : { ...orgHeaders(orgId), 'Idempotency-Key': idempotencyKey },
      signal,
    },
  )
  return {
    runId: wire.run_id,
    threadId: wire.thread_id,
    planId: wire.plan_id,
  }
}

/** `POST /api/v1/browser/runs/:runId/control` — pause / resume / stop a
 * durable browser-agent run (Phase 2 B5). The resulting state change is
 * observed via the corresponding `browser_run_paused`/`browser_run_resumed`
 * (or terminal) event on the run's event stream, not this call's response. */
export async function controlBrowserAiRun(
  orgId: string,
  runId: string,
  action: BrowserAiRunControlAction,
  signal?: AbortSignal,
): Promise<void> {
  await requestJson<{ status: string }>(`/api/v1/browser/runs/${encodeURIComponent(runId)}/control`, {
    method: 'POST',
    body: JSON.stringify({ action }),
    headers: orgHeaders(orgId),
    signal,
  })
}
