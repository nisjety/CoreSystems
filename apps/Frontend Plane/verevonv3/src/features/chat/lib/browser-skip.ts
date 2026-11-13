// The chat side of "Skip this site" on a held browser page (EP-1 S2, D3-647). The person's denial of
// a browser help pause is recorded by the decision itself; this continues the run under
// `chat.run.continue` with the skipped-site form, so the gateway releases the page the run held and
// the model reads that the site was skipped instead of waiting for an answer that will not come.
//
// A denied approval is never consumed, so nothing is spent by the continue: a refused or failed one
// is not ambiguous, and the caller falls back to the plain resume it used before (`false` here).

import type { Approval } from '@/shared/api/orchestration-client'
import {
  chatRunContinueFrameRefusalOf,
  chatRunContinueUnflaggedRefusalOf,
} from '@/shared/actions/chat-run-continue-contract'
import type { SseEvent } from '@/shared/api/sse'

/** Whether declining this approval is "Skip this site": a browser help pause, still open to the person. */
export function isSkippableHelpPause(approval: Approval | undefined): boolean {
  return approval?.cause?.code === 'browser_human_assist'
}

export type BrowserSkipDeps = {
  continueSkippedRun: (
    runId: string,
    approvalId: string,
    claimToken: string,
    onEvent: (event: SseEvent) => void,
  ) => Promise<void>
  newClaimToken: () => string
}

function frameOf(data: string): unknown {
  try {
    return JSON.parse(data) as unknown
  } catch {
    return null
  }
}

/**
 * Continues the run of a skipped site. `true` when the continuation was accepted and nothing refused
 * it, so the run is followed as it is after any continue. `false` when the gateway or the executor
 * refused it, or the request failed: the person's denial stands and the caller resumes the run as it
 * did before this form existed.
 */
export async function runBrowserSkip(
  runId: string,
  approvalId: string,
  deps: BrowserSkipDeps,
): Promise<boolean> {
  let refused = false
  try {
    await deps.continueSkippedRun(runId, approvalId, deps.newClaimToken(), (event) => {
      const frame = frameOf(event.data)
      if (chatRunContinueFrameRefusalOf(frame) ?? chatRunContinueUnflaggedRefusalOf(frame)) {
        refused = true
      }
    })
  } catch {
    return false
  }
  return !refused
}
