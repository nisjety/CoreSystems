import { requestJson } from './http'

// ── Wire contract ───────────────────────────────────────────────────────────
// Phase 2 (durable browser-agent run): starting/controlling an AI browser
// loop is a thin trigger over the same durable-run backbone chat agentic
// runs already use — progress streams via `streamRunEvents` (see
// `run-console-client.ts`), not from this module. Kept separate from
// `browser-client.ts` (manual browsing / one-shot suggestion surface) the
// same way `run-console-client.ts` is kept separate from `chat-client.ts`.

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-velion-org-id': orgId }
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

/** Wire shape of the `ai-runs` response: model-gateway's `browser_run.rs`
 * constructs this JSON directly (`json!({ "run_id", "thread_id", "plan_id" })`)
 * and the Velion gateway forwards it verbatim (`start_ai_run` in
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
): Promise<StartBrowserAiRunResponse> {
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
      headers: orgHeaders(orgId),
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
