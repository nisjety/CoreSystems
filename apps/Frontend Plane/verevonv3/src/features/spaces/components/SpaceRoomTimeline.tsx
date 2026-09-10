import { Bot, MessageCircle, Pin, PinOff, Sparkles } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import {
  AWAITING_APPROVAL_RUN_STATUS,
  FAILED_RUN_STATUSES,
  LIVE_RUN_STATUSES,
  formatWhen,
  stripMarkdownPreview,
  threadStatus,
} from '@/features/spaces/lib/space-thread-presentation'
import { SpaceApprovalPanel } from './SpaceApprovalPanel'
import { getSpaceThreadTranscript, updateSpaceThreadPresentation } from '@/shared/api/spaces-client'
import type { SpaceAgent, SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * The room's shared record, buzz-style: every Space thread renders as a post
 * with its actual turns inline (the MS Teams channel model — a channel is a
 * list of expanded thread-posts), oldest at the top, the composer below.
 * Nothing here links a room conversation out to `/chat`; the room is the
 * workspace.
 *
 * Turn attribution is server-side. Each turn carries the subject Model Plane
 * recorded as its author at append time, and this component resolves that id
 * against Control's roster for a display name. It never infers an author from
 * who is looking: an unrecorded author renders as unnamed, because the one
 * thing worse than an unlabelled turn is a turn labelled with the wrong person.
 *
 * The transcript itself comes from the Space route, not Chat's — Chat resolves
 * a thread only inside the caller's own durable list, which is why a
 * colleague's post used to render as its preview and nothing else.
 */

type TranscriptTurn = {
  readonly role: 'user' | 'assistant'
  readonly content: string
  /** The persona this turn answered as, recorded at the time of the turn by
   * session-core. Absent on turns older than the attribution field and on
   * un-personified assistant turns. */
  readonly agentName?: string
  /** Who wrote this turn, as recorded server-side. Absent on turns written
   * before authorship existed — those stay unnamed rather than borrowing a
   * name from the reader. */
  readonly authorSubjectId?: string
}

const MAX_TURNS_PER_POST = 30

export function normalizeTranscriptTurns(turns: readonly unknown[] | undefined): TranscriptTurn[] {
  if (!turns) return []
  const normalized: TranscriptTurn[] = []
  for (const turn of turns) {
    if (!turn || typeof turn !== 'object') continue
    const record = turn as Record<string, unknown>
    const role = record.role
    const content = typeof record.content === 'string' ? record.content.trim() : ''
    if ((role !== 'user' && role !== 'assistant') || !content) continue
    const agentName = typeof record.agentName === 'string' && record.agentName.trim()
      ? record.agentName.trim()
      : undefined
    const authorSubjectId = typeof record.authorSubjectId === 'string' && record.authorSubjectId.trim()
      ? record.authorSubjectId.trim()
      : undefined
    normalized.push({
      role,
      content,
      ...(agentName ? { agentName } : {}),
      ...(authorSubjectId ? { authorSubjectId } : {}),
    })
    if (normalized.length >= MAX_TURNS_PER_POST) break
  }
  return normalized
}

/**
 * Resolve one recorded author id to something a person can read.
 *
 * Three outcomes, kept apart on purpose: a roster match gives the member's
 * name; an id with no roster entry gives a truthful "former member" rather
 * than a blank, since someone did write it; and no id at all gives an unnamed
 * author. `viewerSubjectId` only ever adds "(you)" to a name the server
 * already attributed — it never supplies one.
 */
export function resolveAuthorName(
  authorSubjectId: string | undefined,
  roster: readonly SpaceRosterMember[],
  viewerSubjectId: string | undefined,
  tr: (no: string, en: string) => string,
): string {
  if (!authorSubjectId) return tr('Ukjent avsender', 'Unknown author')
  const member = roster.find((entry) => entry.subject_id === authorSubjectId)
  const isViewer = viewerSubjectId !== undefined && viewerSubjectId === authorSubjectId
  const name = member?.display_name?.trim()
  if (!name) {
    return isViewer
      ? tr('Du', 'You')
      : tr('Tidligere medlem', 'Former member')
  }
  return isViewer ? `${name} ${tr('(deg)', '(you)')}` : name
}

export interface SpaceRoomTimelineProps {
  readonly spaceRef: string
  readonly spaceName: string
  readonly threads: () => readonly SpaceThread[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly agents: () => readonly SpaceAgent[]
  /** The reading member's own Control subject, used only to mark their own
   * turns as theirs. Never used to attribute an unattributed turn. */
  readonly viewerSubjectId?: () => string | undefined
  /** Focuses the room composer — the intro block's primary action. */
  readonly onStartConversation?: () => void
  /** When provided, the intro block offers the in-room create-agent flow. */
  readonly onCreateAgent?: () => void
  /** When provided, every post offers "Reply" — the composer then continues
   * that thread through the append authority instead of starting a new one. */
  readonly onReply?: (thread: SpaceThread) => void
  /** Called after an approval settles, so the room re-reads its projection and
   * the post's status stops saying it is waiting. */
  readonly onApprovalSettled?: () => void
  /**
   * Posts new since the reader arrived (item 4b), derived by the page from its
   * arrival-time read marker. Absent means "unknown", and nothing is badged.
   */
  readonly unreadThreadIds?: () => ReadonlySet<string>
  /** Called after a pin or retitle lands, so the room re-reads its projection
   * and the new order and name come from the server rather than this browser. */
  readonly onPresentationChanged?: () => void
}

export function SpaceRoomTimeline(props: SpaceRoomTimelineProps) {
  const i18n = useI18n()

  const orderedThreads = createMemo(() =>
    [...props.threads()].sort((a, b) =>
      (a.updated_at ?? a.latest_run_updated_at ?? '').localeCompare(b.updated_at ?? b.latest_run_updated_at ?? ''),
    ),
  )

  const agentName = createMemo(() => {
    const active = props.agents().filter((agent) => agent.status === 'active' && agent.name?.trim())
    return active.length === 1 ? (active[0]?.name as string) : 'Verevon'
  })

  return (
    <Show
      when={orderedThreads().length > 0}
      fallback={
        <SpaceRoomIntro
          spaceName={props.spaceName}
          onStartConversation={props.onStartConversation}
          onCreateAgent={props.onCreateAgent}
        />
      }
    >
      <ol class="verevon-room-timeline" aria-label={i18n.tr('Samtaler i rommet', 'Room conversations')}>
        <For each={orderedThreads()}>
          {(thread) => (
            <SpaceRoomPost
              spaceRef={props.spaceRef}
              thread={thread}
              roster={props.roster}
              viewerSubjectId={props.viewerSubjectId}
              agentName={agentName()}
              unread={() => props.unreadThreadIds?.().has(thread.thread_id) === true}
              onPresentationChanged={props.onPresentationChanged}
              onReply={props.onReply}
              onApprovalSettled={props.onApprovalSettled}
            />
          )}
        </For>
      </ol>
    </Show>
  )
}

function SpaceRoomPost(props: {
  readonly spaceRef: string
  readonly thread: SpaceThread
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
  readonly agentName: string
  readonly onReply?: (thread: SpaceThread) => void
  readonly onApprovalSettled?: () => void
  /** New since the reader arrived — see `SpaceRoomTimelineProps.unreadThreadIds`. */
  readonly unread?: () => boolean
  readonly onPresentationChanged?: () => void
}) {
  const i18n = useI18n()
  const [transcript] = createResource(
    // The revision suffix keeps a settled reply visible: refetching the thread
    // list bumps updated_at, which re-keys this source and refetches the
    // transcript for exactly the post that changed. NUL-joined because thread
    // ids may themselves contain any printable character, including spaces.
    () => `${props.spaceRef}\u0000${props.thread.thread_id}\u0000${props.thread.updated_at ?? props.thread.latest_run_updated_at ?? ''}`,
    async (key) => {
      const [spaceRef = '', threadId = ''] = key.split('\u0000')
      const snapshot = await getSpaceThreadTranscript(spaceRef, threadId).catch(() => null)
      return normalizeTranscriptTurns(snapshot?.turns as readonly unknown[] | undefined)
    },
  )
  const turns = () => (transcript.error ? [] : transcript() ?? [])
  const authorOf = (turn: TranscriptTurn) =>
    resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // Before any turn has loaded, the post is still attributable: the thread's
  // own owner started it.
  const starterName = () =>
    resolveAuthorName(props.thread.owner_subject_id, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  const isFailed = () => FAILED_RUN_STATUSES.has(props.thread.latest_run_status ?? '')
  // Someone's agent is producing output in this post right now, per the
  // server's projection. This is what every OTHER member sees while a turn
  // streams — the sender has the live exchange in their own composer, but
  // until this row existed the rest of the room saw nothing until the poll
  // delivered a finished reply, and the room went dark exactly when it was
  // busiest. Paused-for-approval is deliberately not "working": nobody is
  // producing anything, a person is being waited for.
  const isWorking = () => LIVE_RUN_STATUSES.has(props.thread.latest_run_status ?? '')
  // Only a run that is actually paused gets a decision surface, and only when
  // the projection named the run — an approval card with no run to decide on
  // would be a control that cannot do anything.
  const awaitingApproval = () =>
    props.thread.latest_run_status === AWAITING_APPROVAL_RUN_STATUS
    && Boolean(props.thread.latest_run_id?.trim())
  const when = () => props.thread.updated_at ?? props.thread.latest_run_updated_at
  // Pinning is owner-bound in Session Core, so the control is offered only to
  // the member who started the post. Anyone else sees the pin, not the button:
  // a control that would be refused every time is a control that lies.
  const isAuthor = () => {
    const viewer = props.viewerSubjectId?.()?.trim()
    return Boolean(viewer) && props.thread.owner_subject_id?.trim() === viewer
  }
  const [pinBusy, setPinBusy] = createSignal(false)
  const [pinError, setPinError] = createSignal<string | undefined>(undefined)
  async function togglePin(): Promise<void> {
    if (pinBusy()) return
    setPinBusy(true)
    setPinError(undefined)
    try {
      await updateSpaceThreadPresentation(props.spaceRef, props.thread.thread_id, {
        pinned: !props.thread.pinned,
      })
      // The server owns the order and the flag; re-read rather than flipping a
      // local copy that the next poll would then have to agree with.
      props.onPresentationChanged?.()
    } catch {
      setPinError(
        i18n.tr(
          'Innlegget kunne ikke festes. Ingenting ble endret.',
          'The post could not be pinned. Nothing was changed.',
        ),
      )
    } finally {
      setPinBusy(false)
    }
  }

  return (
    <li
      class="verevon-room-post"
      data-failed={isFailed() || undefined}
      data-pinned={props.thread.pinned === true ? 'true' : undefined}
      data-unread={props.unread?.() ? 'true' : undefined}
    >
      {/* Pinned and new are facts about the post, stated before its content so
          a reader scanning the room sees them first — the way Slack's pin and
          "new" markers sit above a message rather than in its footer. */}
      <Show when={props.thread.pinned === true || props.unread?.()}>
        <div class="verevon-room-post__flags">
          <Show when={props.thread.pinned === true}>
            <span class="verevon-room-post__flag verevon-room-post__flag--pinned">
              <Pin size={12} aria-hidden="true" />
              {i18n.tr('Festet', 'Pinned')}
            </span>
          </Show>
          <Show when={props.unread?.()}>
            <span class="verevon-room-post__flag verevon-room-post__flag--new">
              {i18n.tr('Ny', 'New')}
            </span>
          </Show>
        </div>
      </Show>
      <Show
        when={turns().length > 0}
        fallback={
          <div class="verevon-room-turn">
            <span class="verevon-room-turn__avatar" aria-hidden="true">
              <MessageCircle size={14} />
            </span>
            <div class="verevon-room-turn__content">
              <PostTurnHeader author={starterName()} />
              <p class="verevon-room-turn__plain">
                {(props.thread.preview && stripMarkdownPreview(props.thread.preview))
                  || props.thread.title?.trim()
                  || (transcript.loading
                    ? i18n.tr('Henter samtalen …', 'Loading the conversation…')
                    : i18n.tr('Samtalen har ikke noe lesbart innhold ennå.', 'This conversation has no readable content yet.'))}
              </p>
            </div>
          </div>
        }
      >
        <For each={turns()}>
          {(turn) => (
            <div class="verevon-room-turn" data-role={turn.role}>
              <span
                class={[
                  'verevon-room-turn__avatar',
                  { 'verevon-room-turn__avatar--agent': turn.role === 'assistant' },
                ]}
                aria-hidden="true"
              >
                <Show when={turn.role === 'assistant'} fallback={initials(authorOf(turn))}>
                  <Sparkles size={14} />
                </Show>
              </span>
              <div class="verevon-room-turn__content">
                <PostTurnHeader
                  // The turn's own recorded persona wins; the room-level
                  // heuristic (single active agent, else 'Verevon') covers
                  // turns persisted before attribution existed. A human turn
                  // is named by the subject the server recorded, never by who
                  // happens to be reading.
                  author={turn.role === 'assistant' ? turn.agentName ?? props.agentName : authorOf(turn)}
                  isAgent={turn.role === 'assistant'}
                />
                <Show
                  when={turn.role === 'assistant'}
                  fallback={<p class="verevon-room-turn__plain">{turn.content}</p>}
                >
                  <div class="verevon-room-turn__markdown">
                    <ChatMarkdown content={turn.content} />
                  </div>
                </Show>
              </div>
            </div>
          )}
        </For>
      </Show>

      <Show when={awaitingApproval() ? props.thread.latest_run_id : undefined}>
        {(runId) => (
          <SpaceApprovalPanel runId={runId()} onSettled={props.onApprovalSettled} />
        )}
      </Show>

      {/* The live row. `aria-live="polite"` so a reader using assistive tech
          hears that work started without the row shouting over the transcript;
          it disappears on its own when the projection reports a terminal
          status, which is the "mutate in place" rule from the activity grammar
          rather than a new line per transition. */}
      <Show when={isWorking()}>
        <div class="verevon-room-turn verevon-room-turn--working" role="status" aria-live="polite">
          <span class="verevon-room-turn__avatar verevon-room-turn__avatar--agent" aria-hidden="true">
            <Sparkles size={14} />
          </span>
          <div class="verevon-room-turn__content">
            <PostTurnHeader author={props.agentName} isAgent />
            <p class="verevon-room-turn__working">
              <span class="verevon-room-turn__working-dots" aria-hidden="true" />
              {props.thread.latest_run_status === 'queued'
                ? i18n.tr('venter på å starte', 'waiting to start')
                : i18n.tr('jobber …', 'is working…')}
            </p>
          </div>
        </div>
      </Show>

      <div class="verevon-room-post__meta">
        <span
          class={[
            {
              'verevon-space-status': true,
              'verevon-space-status--failed': isFailed(),
              // Defined in the sheet since the cockpit shipped and never
              // applied by anything — a working post read in the same grey as
              // a finished one.
              'verevon-space-status--active': isWorking(),
            },
          ]}
        >
          {threadStatus(props.thread, i18n.tr)}
        </span>
        <Show when={when()}>
          {(at) => <time>{formatWhen(at())}</time>}
        </Show>
        <Show when={props.onReply}>
          {(reply) => (
            <button type="button" class="verevon-room-post__reply" onClick={() => reply()(props.thread)}>
              {i18n.tr('Svar', 'Reply')}
            </button>
          )}
        </Show>
        <Show when={isAuthor()}>
          <button
            type="button"
            class="verevon-room-post__pin"
            onClick={() => { void togglePin() }}
            disabled={pinBusy()}
            aria-pressed={props.thread.pinned === true ? 'true' : 'false'}
          >
            <Show when={props.thread.pinned === true} fallback={<Pin size={12} aria-hidden="true" />}>
              <PinOff size={12} aria-hidden="true" />
            </Show>
            {props.thread.pinned === true ? i18n.tr('Løsne', 'Unpin') : i18n.tr('Fest', 'Pin')}
          </button>
        </Show>
        <Show when={pinError()}>
          {(message) => <span class="verevon-room-post__pin-error" role="alert">{message()}</span>}
        </Show>
      </div>
    </li>
  )
}

function PostTurnHeader(props: { readonly author: string; readonly isAgent?: boolean }) {
  const i18n = useI18n()
  return (
    <div class="verevon-room-turn__header">
      <strong class="verevon-room-turn__author">{props.author}</strong>
      <Show when={props.isAgent}>
        <span class="verevon-room-turn__agent-chip">
          <Bot size={12} aria-hidden="true" />
          {i18n.tr('Agent', 'Agent')}
        </span>
      </Show>
    </div>
  )
}

function SpaceRoomIntro(props: {
  readonly spaceName: string
  readonly onStartConversation?: () => void
  readonly onCreateAgent?: () => void
}) {
  const i18n = useI18n()
  return (
    <div class="verevon-room-intro">
      <span class="verevon-room-intro__mark" aria-hidden="true">
        <MessageCircle size={26} />
      </span>
      <h3>{props.spaceName}</h3>
      <p>
        {i18n.tr('Dette er begynnelsen på det delte arkivet for', 'This is the beginning of the shared record for')}{' '}
        <strong>{props.spaceName}</strong>.
      </p>
      <div class="verevon-room-intro__actions">
        <Show when={props.onStartConversation}>
          {(start) => (
            <button type="button" class="verevon-room-intro__action" onClick={() => start()()}>
              <span class="verevon-room-intro__action-icon" aria-hidden="true">
                <MessageCircle size={16} />
              </span>
              <span>
                <strong>{i18n.tr('Start en samtale', 'Start a conversation')}</strong>
                <small>{i18n.tr('Skriv til rommet, eller nevn en agent med @.', 'Write to the room, or mention an agent with @.')}</small>
              </span>
            </button>
          )}
        </Show>
        <Show when={props.onCreateAgent}>
          {(create) => (
            <button type="button" class="verevon-room-intro__action" onClick={() => create()()}>
              <span class="verevon-room-intro__action-icon" aria-hidden="true">
                <Bot size={16} />
              </span>
              <span>
                <strong>{i18n.tr('Opprett en agent', 'Create an agent')}</strong>
                <small>{i18n.tr('Gi rommet en kollega som kan svare når den nevnes.', 'Give the room a teammate that answers when mentioned.')}</small>
              </span>
            </button>
          )}
        </Show>
      </div>
    </div>
  )
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  const first = parts[0]?.[0] ?? ''
  const last = parts.length > 1 ? parts[parts.length - 1]?.[0] ?? '' : ''
  return (first + last).toLocaleUpperCase() || '?'
}
