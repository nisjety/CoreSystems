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

/** `POST /api/v1/browser/sessions/:sessionId/ai-runs` — start a durable,
 * server-side, multi-step browser-agent run for the given browser session.
 * The response's `runId` feeds `streamRunEvents` for live progress. */
export async function startBrowserAiRun(
  orgId: string,
  sessionId: string,
  params: StartBrowserAiRunParams,
  signal?: AbortSignal,
): Promise<StartBrowserAiRunResponse> {
  return requestJson<StartBrowserAiRunResponse>(
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
