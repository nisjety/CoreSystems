import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import { useNavigate, useParams } from '@solidjs/router'

import { describeApiError } from '@/shared/i18n/errors'
import { useI18n } from '@/shared/i18n'
import { getSpaceFeatures, resolveSpaceEntry } from '@/shared/api/spaces-client'

import { readSpaceEntityId, spaceEntityHref, spaceEntryLinksEnabled } from '../lib/space-entity-link'
import { SpaceEntryUnavailable } from './SpaceEntryUnavailable'

import './space-entry-redirect.css'

/**
 * WS13-S4b: `/spaces/:spaceId/m/:messageId`. A bare message link names only the message id, so the
 * gateway resolves it to its thread (`GET /spaces/{ref}/entries/{id}`, a fresh Control read decision
 * behind it) and this route replaces itself with `?thread_id=&m=#chat`, where WS13-S4a scrolls to
 * and highlights the message. The link is an address, never a grant: anything the resolver does not
 * answer with a thread (403, 404, a malformed id, the rollout flag off) lands on the same neutral
 * "not available" view, which carries no title, preview or id. Only a transient failure (5xx, no
 * network) offers a retry.
 */
type EntryState = 'resolving' | 'unavailable' | 'retry'

export default function SpaceEntryRedirect() {
  const i18n = useI18n()
  const params = useParams<{ spaceId: string; messageId: string }>()
  const navigate = useNavigate()
  const [state, setState] = createSignal<EntryState>('resolving')
  const [attempt, setAttempt] = createSignal(0)
  let disposed = false
  onCleanup(() => {
    disposed = true
  })

  async function resolve(spaceRef: string, rawMessageId: string) {
    const messageId = readSpaceEntityId(rawMessageId)
    if (!spaceRef || !messageId) {
      setState('unavailable')
      return
    }
    try {
      // Production reads the BFF's rollout flag; a dev build is on unless switched off.
      if (!spaceEntryLinksEnabled() && (await getSpaceFeatures()).entry_links !== true) {
        if (!disposed) setState('unavailable')
        return
      }
      const entry = await resolveSpaceEntry(spaceRef, messageId)
      if (disposed) return
      navigate(spaceEntityHref({ kind: 'message', spaceRef: entry.spaceRef, threadId: entry.threadId, messageId: entry.messageId }), {
        replace: true,
      })
    } catch (error) {
      if (disposed) return
      // 403/404 (and an unknown route on an older gateway) are the neutral answer; only a failure
      // that can pass (5xx, lost connection) is worth a retry. Never the raw message.
      const kind = describeApiError(error, i18n.tr).kind
      setState(kind === 'server' || kind === 'network' ? 'retry' : 'unavailable')
    }
  }

  // Solid 2: `compute` holds the tracked reads, `effect` the imperative work.
  createEffect(
    () => ({ spaceRef: params.spaceId ?? '', messageId: params.messageId ?? '', attempt: attempt() }),
    ({ spaceRef, messageId }) => {
      setState('resolving')
      void resolve(spaceRef, messageId)
    },
  )

  return (
    <Show
      when={state() !== 'resolving'}
      fallback={
        <main class="verevon-space-entry" aria-busy="true">
          <p class="verevon-space-entry__loading" role="status" aria-live="polite">
            {i18n.tr('Åpner meldingen…', 'Opening the message…')}
          </p>
        </main>
      }
    >
      <SpaceEntryUnavailable
        spaceRef={params.spaceId || undefined}
        variant={state() === 'retry' ? 'retry' : 'unavailable'}
        onRetry={() => setAttempt((value) => value + 1)}
      />
    </Show>
  )
}
