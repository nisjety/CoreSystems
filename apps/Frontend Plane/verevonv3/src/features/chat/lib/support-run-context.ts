import { createEffect, onCleanup } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getOrganizationZdr } from '@/shared/api/organization-client'
import { reportProblemEnabled, setSupportRunContext, type SupportRunContext } from '@/shared/api/support-correlation'

/** The run of the latest answer in a thread, or null when no answer has one yet. A problem report names this run. */
export function latestAnswerRunId(turns: readonly { readonly role: string; readonly runId?: string }[]): string | null {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (turn?.role === 'assistant' && turn.runId) return turn.runId
  }
  return null
}

/**
 * Publishes the chat run on screen for the feedback widget's problem report (`VITE_REPORT_PROBLEM`). The run is offered
 * only while the chat keeps content: a temporary chat keeps none, and neither does an organization with zero retention.
 * Until the organization's setting has been read for the organization on screen, and when it cannot be read at all, no
 * run is offered either. Unmounting clears the published run, so a report never names a run from a page left behind.
 *
 * With the switch off nothing is read and nothing is published.
 */
export function useSupportRunContext(input: {
  /** The organization the chat belongs to. */
  orgId: () => string | null | undefined
  /** The run of the answer on screen, or null when there is none. */
  runId: () => string | null
  /** True while the chat on screen is temporary: the toggle for the next send, or the thread itself. */
  temporary: () => boolean
}): void {
  // Each answer names the organization it is for, so an answer read for another organization never applies after a switch.
  const [orgSetting] = createResource(
    () => (reportProblemEnabled() ? input.orgId() ?? null : null),
    (orgId) => getOrganizationZdr(orgId).then(
      (zdr) => ({ orgId, zdr }),
      () => ({ orgId, zdr: true }),
    ),
  )
  createEffect(
    () => {
      const runId = reportProblemEnabled() ? input.runId() : null
      if (!runId) return null
      const setting = orgSetting()
      const orgId = input.orgId()
      // Fail closed: an unread setting, a setting for another organization, or a zero-retention organization offers no run.
      // The organization must be named before a setting can match it, so an absent organization never matches an unread one.
      const retainsContent = !input.temporary()
        && Boolean(orgId)
        && setting?.orgId === orgId
        && setting?.zdr === false
      const context: SupportRunContext = { runId, retainsContent }
      return context
    },
    (context) => setSupportRunContext(context),
  )
  onCleanup(() => setSupportRunContext(null))
}
