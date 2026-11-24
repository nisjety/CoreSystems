import { MessageCircle } from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import { Match, Show, Switch, createEffect, createMemo, onCleanup } from 'solid-js'

import { subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { threadStatus, formatWhen } from '@/features/spaces/lib/space-thread-presentation'
import { ApiError } from '@/shared/api/http'
import { presentSpaceDenialText, readSpaceDenial } from '@/shared/api/space-denials'
import { getSpaceThreadTranscript } from '@/shared/api/spaces-client'
import type {
  SpaceAgent,
  SpaceMessagePolicyHints,
  SpaceOwnHistoryWindow,
  SpaceRosterMember,
  SpaceThread,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { ThreadFollowControl } from '@/features/attention/components/ThreadFollowControl'
import { ownWindowSignature } from '../../lib/space-history-window'
import { SpaceHistoryWindowNote } from '../SpaceHistoryWindowNote'
import { SpaceRestrictedNotice } from '../SpaceRestrictedNotice'
import { SpaceTurnList, normalizeTranscriptTurns, resolveAuthorName } from './SpaceTurnList'
import { createSpaceTurnLifecycle } from '../../lib/space-message-lifecycle'
import { threadTranscriptEpoch } from '../../lib/space-transcript-epoch'
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
  // A 403 may carry a typed Space denial (why Control said no); kept so the pane can say that.
  | { readonly threadId: string; readonly state: 'restricted'; readonly denial?: { code: string | null; details: Readonly<Record<string, unknown>> | null } }
  | { readonly threadId: string; readonly state: 'missing' | 'error' }

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
  /** The room roster's agents, so an agent turn here has the same colour as in the timeline. */
  readonly agents?: () => readonly SpaceAgent[]
  /**
   * Whether this reader may read the room's conversations (the listing's `shared_read.available`), the
   * signal the timeline gates its transcripts on. `false` makes no transcript call, draws the room's
   * restricted notice and no turn text, including turns read before. Absent means available.
   */
  readonly sharedReadAvailable?: () => boolean
  /**
   * D18: the caller's own window (`membership.history_window`). Control's `hidden: true` draws the same quiet marker the
   * timeline has, above the turns, because a thread that began before the floor is read from the floor on. Absent,
   * malformed or not hidden: no marker. Presentation only: nothing here asks for the hidden turns.
   */
  readonly historyWindow?: () => SpaceOwnHistoryWindow | undefined
  /**
   * The pane's reply composer (WS01-S7b), drawn under the turns. The page passes it only while
   * `features.human_posts` is on; absent, the panel stays read-only. Not drawn for a thread that
   * is not available to this member, because there is nothing to reply to.
   */
  readonly footer?: JSX.Element
  /**
   * A message to bring into view (`?thread_id=&m=`, WS13-S4a). Only a request: when the transcript the
   * viewer may read does not hold it, the panel says so neutrally and never echoes the id.
   */
  readonly messageId?: () => string | null | undefined
  /** `features.message_revise` (WS12-S7): edit and delete controls are offered. Absent means off. */
  readonly messageReviseEnabled?: () => boolean
  readonly reactionsEnabled?: () => boolean
  /** `data.message_policy` (WS12-S7): hints for which controls to offer. Never an authorization. */
  readonly messagePolicy?: () => SpaceMessagePolicyHints | null | undefined
  readonly viewerRole?: () => string | undefined
  readonly archived?: () => boolean
  readonly postMaxChars?: () => number | undefined
  /** Called after an edit or delete landed, so the room re-reads its projection. */
  readonly onMessageChanged?: () => void
}

// How long a reached message keeps its outline.
const HIGHLIGHT_MS = 3000

export function SpaceThreadPanel(props: SpaceThreadPanelProps) {
  const i18n = useI18n()
  // Shared reads off: the same answer the timeline gives, so no transcript is asked for and none is drawn.
  const sharedRead = () => props.sharedReadAvailable?.() !== false
  const [read, { refetch, mutate }] = createResource(
    // The revision suffix refetches when the room's poll reports the thread
    // changed, the same keying the timeline uses. NUL-joined because ids may
    // contain any printable character. No key while shared reads are off: no request.
    () => {
      if (!sharedRead()) return undefined
      const thread = props.thread()
      // DEPTH-WS12-S4-1: an edit or delete the room stream signalled re-keys it too.
      return `${props.spaceRef}\u0000${props.threadId}\u0000${thread?.updated_at ?? thread?.latest_run_updated_at ?? ''}\u0000${threadTranscriptEpoch(props.threadId)}`
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
        if (error instanceof ApiError && error.status === 403) {
          return readSpaceDenial(error)
            ? {
                threadId,
                state: 'restricted',
                denial: { code: error.code, details: Array.isArray(error.details) ? null : error.details },
              }
            : { threadId, state: 'restricted' }
        }
        if (error instanceof ApiError && error.status === 404) return { threadId, state: 'missing' }
        return { threadId, state: 'error' }
      }
    },
  )
  // A switch to zero data retention while the pane is open: the turns on screen were read under the posture that has
  // just ended, so they come off now and the pane reads again under the new one.
  onCleanup(
    subscribeLocalRetention((allowed) => {
      if (allowed) return
      mutate(undefined)
      void refetch()
    }),
  )
  // D18: when the caller's floor moves (an admin changed this member's window), the turns on screen were read under the
  // old floor. They come off now and the thread is read again under the new one: read again, never filtered here.
  let floorSeen: string | undefined
  createEffect(
    () => ownWindowSignature(props.historyWindow?.()),
    (signature) => {
      const moved = floorSeen !== undefined && floorSeen !== signature
      floorSeen = signature
      if (!moved) return
      mutate(undefined)
      void refetch()
    },
  )
  // WS12-S7: edit and delete controls; a change re-reads this thread and the room's projection.
  const lifecycle = createSpaceTurnLifecycle({
    spaceRef: () => props.spaceRef,
    threadId: () => props.threadId,
    enabled: () => props.messageReviseEnabled?.() === true,
    reactionsEnabled: () => props.reactionsEnabled?.() === true,
    viewerSubjectId: () => props.viewerSubjectId?.(),
    viewerRole: () => props.viewerRole?.(),
    threadOwnerSubjectId: () => props.thread()?.owner_subject_id,
    policy: () => props.messagePolicy?.(),
    archived: props.archived,
    onChanged: () => {
      void refetch()
      props.onMessageChanged?.()
    },
  })
  // A read for a different thread is not an answer for this one: while the new
  // read is in flight the pane shows loading, not the previous thread. With shared
  // reads off there is no answer at all: the resource keeps its last value, which is not drawn.
  const current = (): PanelRead | undefined => {
    if (!sharedRead()) return undefined
    const value = read()
    return value && value.threadId === props.threadId ? value : undefined
  }
  const when = () => props.thread()?.updated_at ?? props.thread()?.latest_run_updated_at

  // `?m=`: which turn of the readable transcript it names. `missing` is decided from the transcript the
  // viewer was given, so a hidden, tombstoned or invented id all read the same.
  let root: HTMLDivElement | undefined
  const wantedId = () => props.messageId?.() ?? null
  const wantedTurn = createMemo(() => {
    const id = wantedId()
    return id ? okTurns(current()).find((turn) => turn.messageId === id) : undefined
  })
  const entryMissing = () => wantedId() !== null && (!sharedRead() || (current()?.state === 'ok' && wantedTurn() === undefined))
  const entryAnnouncement = () => {
    const turn = wantedTurn()
    if (turn) {
      const author = turn.role === 'assistant'
        ? turn.agentName ?? props.agentName
        : resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
      return i18n.tr(`Viser melding fra ${author}`, `Showing message from ${author}`)
    }
    return entryMissing() ? i18n.tr('Denne meldingen er ikke tilgjengelig for deg.', 'This message is not available to you.') : ''
  }
  // Scroll to, outline and focus the turn once it is rendered; the outline goes after three seconds
  // (and never animates: it is a static 2 px outline). Runs once per (thread, message) reached.
  createEffect(
    () => ({ id: wantedId(), found: wantedTurn() !== undefined, thread: props.threadId }),
    ({ id, found }) => {
      if (!id || !found) return undefined
      let timer: ReturnType<typeof setTimeout> | undefined
      let target: Element | null = null
      const frame = requestAnimationFrame(() => {
        target = root?.querySelector(`[data-message-id="${id}"]`) ?? null
        if (!(target instanceof HTMLElement)) return
        const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
        target.scrollIntoView({ block: 'center', behavior: reduce ? 'auto' : 'smooth' })
        target.setAttribute('data-highlight', 'true')
        target.focus({ preventScroll: true })
        timer = setTimeout(() => target?.removeAttribute('data-highlight'), HIGHLIGHT_MS)
      })
      return () => {
        cancelAnimationFrame(frame)
        if (timer) clearTimeout(timer)
        target?.removeAttribute('data-highlight')
      }
    },
  )
  onCleanup(() => { root = undefined })

  return (
    <div ref={(element) => { root = element }} class="verevon-space-thread-panel" aria-busy={sharedRead() && current() === undefined ? 'true' : 'false'}>
      <Show when={wantedId()}>
        <p class={entryMissing() ? 'verevon-space-inline-status' : 'sr-only'} role="status" aria-live="polite" data-entry-status={entryMissing() ? 'missing' : 'found'}>
          {entryAnnouncement()}
        </p>
      </Show>
      <Show when={props.thread()}>
        {(thread) => (
          <p class="verevon-space-thread-panel__meta">
            <span class="verevon-space-status">{threadStatus(thread(), i18n.tr)}</span>
            <Show when={(thread().reply_count ?? 0) > 0}>
              <span class="verevon-space-thread-panel__replies">
                {thread().reply_count === 1
                  ? i18n.tr('1 svar', '1 reply')
                  : i18n.tr(`${thread().reply_count} svar`, `${thread().reply_count} replies`)}
              </span>
            </Show>
            <Show when={when()}>
              {(at) => <time>{formatWhen(at(), { locale: i18n.locale() })}</time>}
            </Show>
          </p>
        )}
      </Show>
      <Show when={current()?.state === 'ok'}>
        <ThreadFollowControl spaceRef={props.spaceRef} threadId={props.threadId} />
        <SpaceHistoryWindowNote window={() => props.historyWindow?.()} />
      </Show>
      <Switch
        fallback={
          <p class="verevon-space-inline-status" role="status">
            {i18n.tr('Henter samtalen …', 'Loading the conversation…')}
          </p>
        }
      >
        {/* Shared reads off for this Space: the room's notice, the same one the timeline shows. */}
        <Match when={!sharedRead()}>
          <SpaceRestrictedNotice variant="room" />
        </Match>
        <Match when={current()?.state === 'restricted'}>
          <Show when={restrictedDenial(current())} fallback={<SpaceRestrictedNotice variant="thread" />}>
            {(denial) => (
              <p class="verevon-space-inline-status" role="status" data-denial="typed">
                {presentSpaceDenialText(denial(), i18n.tr)}
              </p>
            )}
          </Show>
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
                agents={props.agents}
                spaceRef={props.spaceRef}
                threadId={props.threadId}
                lifecycle={lifecycle}
                editMaxChars={props.postMaxChars?.()}
              />
            </div>
          </Show>
        </Match>
      </Switch>
      <Show when={props.footer && current()?.state !== 'missing'}>
        <div class="verevon-space-thread-panel__composer">{props.footer}</div>
      </Show>
    </div>
  )
}

function restrictedDenial(value: PanelRead | undefined) {
  return value?.state === 'restricted' ? value.denial : undefined
}

function okTurns(value: PanelRead | undefined): readonly TranscriptTurn[] {
  return value?.state === 'ok' ? value.turns : []
}
