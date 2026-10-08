import { createEffect, createSignal, onCleanup, type Accessor } from 'solid-js'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { listChatThreads } from '@/shared/api/chat-client'
import { getSession } from '@/shared/session/session-store'

/**
 * Whether the workspace allows a local copy of what a person typed or searched
 * for. It is the server's verdict (see chat-retention), so it follows the
 * posture when the next listing changes it. Allowed until the server says
 * otherwise, like the write gate it mirrors.
 */
export function useLocalRetentionAllowed(): Accessor<boolean> {
  const [allowed, setAllowed] = createSignal(isLocalRetentionAllowed())
  onCleanup(subscribeLocalRetention((next) => { setAllowed(next) }))
  return allowed
}

/**
 * Makes sure Home has been told the workspace's retention verdict.
 *
 * The verdict rides on the threads listing (see chat-client), and the gate stays
 * open until one arrives. Only the recent-work card lists threads on its own, and
 * it is not on screen on Nett or Hent ut, which a reload or a shared link can open
 * first; without a listing, a workspace that denies local copies would keep the
 * draft, the saved search and the Nett history until Chat or the sidebar happened
 * to list. So when `listedElsewhere` is false Home asks once for the workspace and
 * user it is showing. What was written before the answer is removed when it says
 * no. A failed request is not retried: any later listing states the verdict.
 *
 * `listedElsewhere` is true while a Home card lists the threads itself; that
 * counts as having asked for the identity it was showing.
 */
export function useRetentionVerdictProbe(listedElsewhere: Accessor<boolean>): void {
  const session = getSession()
  let asked: string | null = null
  createEffect(
    () => ({ user: session.user?.id, org: session.activeOrg?.id, elsewhere: listedElsewhere() }),
    ({ user, org, elsewhere }) => {
      if (!user || !org) {
        asked = null
        return
      }
      const identity = `${encodeURIComponent(org)}:${encodeURIComponent(user)}`
      if (identity === asked) return
      asked = identity
      if (!elsewhere) void listChatThreads().catch(() => undefined)
    },
  )
}
