import { MessageCircle } from '@/shared/icons'
import { Match, Show, Switch } from 'solid-js'

import { threadStatus, formatWhen } from '@/features/spaces/lib/space-thread-presentation'
import { ApiError } from '@/shared/api/http'
import { getSpaceThreadTranscript } from '@/shared/api/spaces-client'
import type { SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { SpaceRestrictedNotice } from '../SpaceRestrictedNotice'
import { SpaceTurnList, normalizeTranscriptTurns } from './SpaceTurnList'
import type { TranscriptTurn } from './SpaceTurnList'

/**
 * One thread, read-only, in the room's side pane. Reads through the Space
 * transcript route, which asks Control whether the caller is a current
 * recipient of the Space; a thread id in the URL grants nothing by itself.
 *
 * Every way the read can end is a named state, never a blank pane and never
 * "log in again": the member may be in the room without being admitted to this
 * thread (restricted), the thread may not exist for them (missing), or the read
 * may simply have failed (error, with a retry). No transcript content is stored
 * in the browser.
 */

// A pane shows the whole conversation, not the timeline's per-post preview cap.
const MAX_PANE_TURNS = 500

type PanelRead =
  | { readonly threadId: string; readonly state: 'ok'; readonly turns: readonly TranscriptTurn[] }
  | { readonly threadId: string; readonly state: 'restricted' | 'missing' | 'error' }

export interface SpaceThreadPanelProps {
  readonly spaceRef: string
  readonly threadId: string
  /** The room's own projection of this thread, when it is in the listing. Used
   * for the status line and as the revision that refreshes the transcript. */
  readonly thread: () => SpaceThread | undefined
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
  /** Room-level fallback name for an assistant turn that recorded no persona. */
  readonly agentName: string
}

export function SpaceThreadPanel(props: SpaceThreadPanelProps) {
  const i18n = useI18n()
  const [read, { refetch }] = createResource(
    // The revision suffix refetches when the room's poll reports the thread
    // changed, the same keying the timeline uses. NUL-joined because ids may
    // contain any printable character.
    () => {
      const thread = props.thread()
      return `${props.spaceRef}\u0000${props.threadId}\u0000${thread?.updated_at ?? thread?.latest_run_updated_at ?? ''}`
    },
    async (key): Promise<PanelRead> => {
      const [spaceRef = '', threadId = ''] = key.split('\u0000')
      try {
        const snapshot = await getSpaceThreadTranscript(spaceRef, threadId)
        return {
          threadId,
          state: 'ok',
          turns: normalizeTranscriptTurns(snapshot.turns as readonly unknown[], MAX_PANE_TURNS),
        }
      } catch (error) {
        if (error instanceof ApiError && error.status === 403) return { threadId, state: 'restricted' }
        if (error instanceof ApiError && error.status === 404) return { threadId, state: 'missing' }
        return { threadId, state: 'error' }
      }
    },
  )
  // A read for a different thread is not an answer for this one: while the new
  // read is in flight the pane shows loading, not the previous thread.
  const current = (): PanelRead | undefined => {
    const value = read()
    return value && value.threadId === props.threadId ? value : undefined
  }
  const when = () => props.thread()?.updated_at ?? props.thread()?.latest_run_updated_at

  return (
    <div class="verevon-space-thread-panel" aria-busy={current() === undefined ? 'true' : 'false'}>
      <Show when={props.thread()}>
        {(thread) => (
          <p class="verevon-space-thread-panel__meta">
            <span class="verevon-space-status">{threadStatus(thread(), i18n.tr)}</span>
            <Show when={when()}>
              {(at) => <time>{formatWhen(at())}</time>}
            </Show>
          </p>
        )}
      </Show>
      <Switch
        fallback={
          <p class="verevon-space-inline-status" role="status">
            {i18n.tr('Henter samtalen …', 'Loading the conversation…')}
          </p>
        }
      >
        <Match when={current()?.state === 'restricted'}>
          <SpaceRestrictedNotice variant="thread" />
        </Match>
        <Match when={current()?.state === 'missing'}>
          <div class="verevon-space-empty-state" role="status">
            <MessageCircle size={20} aria-hidden="true" />
            <div>
              <h3>{i18n.tr('Samtalen er ikke tilgjengelig', 'This conversation is not available')}</h3>
              <p>{i18n.tr(
                'Den finnes ikke i dette rommet, eller den er ikke åpen for deg.',
                'It does not exist in this room, or it is not open to you.',
              )}</p>
            </div>
          </div>
        </Match>
        <Match when={current()?.state === 'error'}>
          <div class="verevon-space-empty-state" role="alert">
            <MessageCircle size={20} aria-hidden="true" />
            <div>
              <h3>{i18n.tr('Kunne ikke hente samtalen', 'Could not load the conversation')}</h3>
              <p>{i18n.tr('Prøv igjen om litt. Ingenting ble endret.', 'Try again shortly. Nothing was changed.')}</p>
              <button type="button" class="verevon-space-secondary-action" onClick={() => { void refetch() }}>
                {i18n.tr('Prøv igjen', 'Try again')}
              </button>
            </div>
          </div>
        </Match>
        <Match when={current()?.state === 'ok'}>
          <Show
            when={okTurns(current()).length > 0}
            fallback={
              <p class="verevon-space-inline-status" role="status">
                {i18n.tr('Samtalen har ikke noe lesbart innhold ennå.', 'This conversation has no readable content yet.')}
              </p>
            }
          >
            <div class="verevon-space-thread-panel__turns">
              <SpaceTurnList
                turns={() => okTurns(current())}
                roster={props.roster}
                viewerSubjectId={props.viewerSubjectId}
                agentName={props.agentName}
              />
            </div>
          </Show>
        </Match>
      </Switch>
    </div>
  )
}

function okTurns(value: PanelRead | undefined): readonly TranscriptTurn[] {
  return value?.state === 'ok' ? value.turns : []
}
