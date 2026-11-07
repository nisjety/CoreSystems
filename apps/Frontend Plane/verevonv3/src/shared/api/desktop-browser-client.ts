import { desktopBrowserSuggestionSchema } from '@/shared/actions/desktop-browser'
import { ApiError, requestJson } from './http'

export async function suggestDesktopBrowserAction(
  request: { goal: string; url: string; title: string; observation: object; history: string[] },
  signal: AbortSignal,
) {
  const plan = (history: string[]) => requestJson<{ suggestion?: unknown }>('/api/v1/browser/assistant/desktop/suggestions', {
    method: 'POST', body: JSON.stringify({ ...request, history }), signal,
  })
  const response = await plan(request.history).catch((error: unknown) => {
    if (!(error instanceof ApiError) || error.status !== 502) throw error
    signal.throwIfAborted()
    // A malformed upstream proposal can be retried without repeating a browser action.
    return plan([...request.history, 'The previous planner reply was invalid. Return exactly one JSON object.'])
  })
  return desktopBrowserSuggestionSchema.parse(response?.suggestion)
}
