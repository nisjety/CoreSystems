import { Bot, MessageCircle, Pin, PinOff } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import {
  AWAITING_APPROVAL_RUN_STATUS,
  FAILED_RUN_STATUSES,
  LIVE_RUN_STATUSES,
  formatRelativeWhen,
  formatWhen,
  stripMarkdownPreview,
  threadStatus,
} from '@/features/spaces/lib/space-thread-presentation'
import { AgentAvatar } from './AgentAvatar'
import { SpaceApprovalPanel } from './SpaceApprovalPanel'
import { agentAvatarColor, agentForName } from '../lib/agent-avatar'
import { agentCardModel, memberCardModel } from '../lib/agent-activity'
import type { IdentityCardModel } from '../lib/agent-activity'
import { PostTurnHeader, SpaceTurnList, initials, normalizeTranscriptTurns, resolveAuthorName } from './sidepane/SpaceTurnList'
import type { TranscriptTurn } from './sidepane/SpaceTurnList'
import { SpaceRestrictedNotice } from './SpaceRestrictedNotice'
import { SpacePendingNewPost, SpacePendingReplies } from './SpacePendingPost'
import type { SpaceJournalEntry } from '../lib/space-send-journal'
import { SpaceArtifactChip } from './SpaceArtifactChip'
import { SpaceArtifactSheet } from './SpaceArtifactSheet'
import { roomItems } from '@/features/spaces/lib/room-artifact-items'
import { roomArtifactsEnabled } from '@/features/spaces/lib/room-artifacts'
import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { loadSpaceTranscript, updateSpaceThreadPresentation } from '@/shared/api/spaces-client'
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

// The turn model, the transcript normaliser and the author-name resolver moved
// to the extracted turn list with the rows themselves; re-exported so existing
// importers keep working.
export { normalizeTranscriptTurns, resolveAuthorName } from './sidepane/SpaceTurnList'

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
  /** When provided, every post offers "Open thread" — the read-only side pane
   * bound to `?thread_id=`. Absent while the pane is switched off. */
  readonly onOpenThread?: (thread: SpaceThread) => void
  /**
   * Whether this reader may read the room's other conversations (the listing's
   * `shared_read.available`). `false` shows one restricted notice above the
   * timeline and makes no transcript call at all. Absent means available, which
   * is today's behaviour on a gateway that does not say.
   */
  readonly sharedReadAvailable?: () => boolean
  /**
   * Artifacts the sender's own live stream produced in a thread, held in memory
   * by the page until the next transcript read carries them. Absent while the
   * room artifact surfaces are off.
   */
  readonly liveArtifacts?: (threadId: string) => readonly ChatArtifact[]
  /**
   * This browser's own posts that the room's projection does not carry yet (WS01-S6, the send
   * journal's entries). A new post is drawn as a post of its own at the end; a reply is drawn
   * after the real turns of the thread it goes to. Absent means no optimistic rows.
   */
  readonly pendingPosts?: () => readonly SpaceJournalEntry[]
  /**
   * `features.human_posts` (WS01-S7b, reshaped in U2). On, a thread with replies shows its root
   * message and one reply-summary row (who replied, how many, the latest reply, how long ago) that
   * opens the pane, the way Slack and Teams fold a thread. Takes effect only with `onOpenThread`
   * (a folded turn must be reachable); absent or off, every turn is drawn as before.
   */
  readonly collapseLongThreads?: () => boolean
}

/** Distinct reply authors drawn in the summary row's avatar stack; the rest read as "+N". */
const SUMMARY_AVATARS = 3

/** The room-level name for an assistant turn that recorded no persona: the
 * single active agent when there is exactly one, else the product name. */
export function roomAgentName(agents: readonly SpaceAgent[]): string {
  const active = agents.filter((agent) => agent.status === 'active' && agent.name?.trim())
  return active.length === 1 ? (active[0]?.name as string) : 'Verevon'
}

export function SpaceRoomTimeline(props: SpaceRoomTimelineProps) {
  const i18n = useI18n()

  const orderedThreads = createMemo(() =>
    [...props.threads()].sort((a, b) =>
      (a.updated_at ?? a.latest_run_updated_at ?? '').localeCompare(b.updated_at ?? b.latest_run_updated_at ?? ''),
    ),
  )

  const agentName = createMemo(() => roomAgentName(props.agents()))
  const sharedRead = () => props.sharedReadAvailable?.() !== false
  const artifactsOn = roomArtifactsEnabled()
  const pendingNewPosts = () =>
    (props.pendingPosts?.() ?? []).filter((entry) => entry.kind === 'post_new')
  const pendingRepliesFor = (threadId: string) =>
    (props.pendingPosts?.() ?? []).filter((entry) => entry.kind === 'post_reply' && entry.threadId === threadId)

  return (
    <Show
      when={orderedThreads().length > 0 || pendingNewPosts().length > 0}
      fallback={
        <SpaceRoomIntro
          spaceName={props.spaceName}
          onStartConversation={props.onStartConversation}
          onCreateAgent={props.onCreateAgent}
        />
      }
    >
      {/* Once for the whole room, above the posts: when Control has not enabled
          shared reads, every post would otherwise repeat the same refusal. */}
      <Show when={!sharedRead()}>
        <SpaceRestrictedNotice variant="room" />
      </Show>
      <ol class="verevon-room-timeline" aria-label={i18n.tr('Samtaler i rommet', 'Room conversations')}>
        <For each={orderedThreads()}>
          {(thread) => (
            <SpaceRoomPost
              spaceRef={props.spaceRef}
              thread={thread}
              threads={props.threads}
              roster={props.roster}
              viewerSubjectId={props.viewerSubjectId}
              agentName={agentName()}
              agents={props.agents}
              transcriptEnabled={sharedRead}
              artifactsOn={artifactsOn}
              liveArtifacts={props.liveArtifacts}
              unread={() => props.unreadThreadIds?.().has(thread.thread_id) === true}
              onPresentationChanged={props.onPresentationChanged}
              onReply={props.onReply}
              onOpenThread={props.onOpenThread}
              onApprovalSettled={props.onApprovalSettled}
              pendingReplies={() => pendingRepliesFor(thread.thread_id)}
              collapse={() => props.collapseLongThreads?.() === true && props.onOpenThread !== undefined}
            />
          )}
        </For>
        <For each={pendingNewPosts()}>{(entry) => <SpacePendingNewPost entry={entry} />}</For>
      </ol>
    </Show>
  )
}

function SpaceRoomPost(props: {
  readonly spaceRef: string
  readonly thread: SpaceThread
  /** Every thread in the room, for the identity hover card's activity line. */
  readonly threads: () => readonly SpaceThread[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
  readonly agentName: string
  /** Roster agents, for the avatar colour of an assistant turn (exactly-one-name match, else neutral). */
  readonly agents: () => readonly SpaceAgent[]
  /** False when the room's shared read is unavailable: no transcript call is made. */
  readonly transcriptEnabled: () => boolean
  /** `features.room_artifacts`: chips, sheet and artifact-only turns. Off, the post reads as before. */
  readonly artifactsOn: boolean
  readonly liveArtifacts?: (threadId: string) => readonly ChatArtifact[]
  readonly onReply?: (thread: SpaceThread) => void
  readonly onApprovalSettled?: () => void
  /** New since the reader arrived — see `SpaceRoomTimelineProps.unreadThreadIds`. */
  readonly unread?: () => boolean
  readonly onPresentationChanged?: () => void
  readonly onOpenThread?: (thread: SpaceThread) => void
  /** This browser's own replies to this thread that the transcript does not show yet. */
  readonly pendingReplies?: () => readonly SpaceJournalEntry[]
  /** Collapse a long thread to first + last three and show the reply count (needs the pane). */
  readonly collapse?: () => boolean
}) {
  const i18n = useI18n()
  const [transcript, { refetch: refetchTranscript }] = createResource(
    // The revision suffix keeps a settled reply visible: refetching the thread
    // list bumps updated_at, which re-keys this source and refetches the
    // transcript for exactly the post that changed. NUL-joined because thread
    // ids may themselves contain any printable character, including spaces.
    // `undefined` (shared read unavailable) makes no request at all.
    () => props.transcriptEnabled()
      ? `${props.spaceRef}\u0000${props.thread.thread_id}\u0000${props.thread.updated_at ?? props.thread.latest_run_updated_at ?? ''}`
      : undefined,
    async (key) => {
      const [spaceRef = '', threadId = ''] = key.split('\u0000')
      // Each way the read can end is a named state: `restricted` (Control
      // declined, a real answer), `missing` (404) and `error` (retryable) are
      // said out loud instead of being swallowed into the preview.
      const load = await loadSpaceTranscript(spaceRef, threadId)
      return load.state === 'ok'
        ? { state: 'ok' as const, turns: normalizeTranscriptTurns(load.turns as readonly unknown[] | undefined) }
        : { state: load.state, turns: [] as readonly TranscriptTurn[] }
    },
  )
  // With the artifact surfaces off an artifact-only turn has nothing to show,
  // so it is dropped here and the post reads exactly as it did before.
  const turns = (): readonly TranscriptTurn[] => {
    const all = transcript()?.turns ?? []
    return props.artifactsOn ? all : all.filter((turn) => turn.content.length > 0)
  }
  const [openArtifactId, setOpenArtifactId] = createSignal<string | undefined>(undefined)
  // Live artifacts from this browser's own stream that the transcript does not
  // carry yet (same id and version). In memory only, never stored.
  const pendingLive = (): readonly ChatArtifact[] => {
    if (!props.artifactsOn) return []
    const persisted = new Set(
      turns().flatMap((turn) => (turn.artifacts ?? []).map((artifact) => `${artifact.id}\u0000${artifact.version}`)),
    )
    return (props.liveArtifacts?.(props.thread.thread_id) ?? []).filter(
      (artifact) => !persisted.has(`${artifact.id}\u0000${artifact.version}`),
    )
  }
  const artifactItems = createMemo(() =>
    props.artifactsOn ? roomItems([...turns(), { artifacts: pendingLive() }]) : [],
  )
  const readState = () => transcript()?.state
  // A thread with replies folds to its root message plus one summary row that opens the pane. The
  // count is what the pane will show (every turn after the root, agent answers included); until the
  // transcript has loaded the projection's `reply_count` stands in.
  const replyCount = () => {
    if (props.collapse?.() !== true) return 0
    if (turns().length > 0) return turns().length - 1
    const count = props.thread.reply_count
    return typeof count === 'number' && count > 0 ? count : 0
  }
  const summarised = () => replyCount() > 0
  const headTurns = (): readonly TranscriptTurn[] => (summarised() && turns().length > 0 ? turns().slice(0, 1) : turns())
  const replyTurns = (): readonly TranscriptTurn[] => turns().slice(1)
  const replyAuthorName = (turn: TranscriptTurn): string =>
    turn.role === 'assistant'
      ? turn.agentName ?? props.agentName
      : resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // Distinct reply authors in the order they first spoke. Agents and members are told apart by what
  // the server recorded; an unnamed author is one participant, not one per turn.
  const replyParticipants = () => {
    const seen = new Set<string>()
    const out: { key: string; agent: boolean; name: string }[] = []
    for (const turn of replyTurns()) {
      const agent = turn.role === 'assistant'
      const name = replyAuthorName(turn)
      const key = `${agent ? 'agent' : 'user'}\u0000${agent ? name : turn.authorSubjectId ?? ''}`
      if (seen.has(key)) continue
      seen.add(key)
      out.push({ key, agent, name })
    }
    return out
  }
  const lastReply = (): TranscriptTurn | undefined => {
    const replies = replyTurns()
    return replies[replies.length - 1]
  }
  const replySummaryLabel = () =>
    replyCount() === 1
      ? i18n.tr('Åpne tråd · 1 svar', 'Open thread · 1 reply')
      : i18n.tr(`Åpne tråd · ${replyCount()} svar`, `Open thread · ${replyCount()} replies`)
  // Before any turn has loaded, the post is still attributable: the thread's
  // own owner started it.
  const starterName = () =>
    resolveAuthorName(props.thread.owner_subject_id, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // Identity hover cards: the post's starter (a member) and the working agent.
  // No card when the author or agent cannot be pinned down.
  const starterCard = (): IdentityCardModel | undefined =>
    props.thread.owner_subject_id
      ? memberCardModel({
          name: starterName(),
          subjectId: props.thread.owner_subject_id,
          roster: props.roster(),
          threads: props.threads(),
        })
      : undefined
  const workingCard = (): IdentityCardModel | undefined => {
    const agents = props.agents()
    const agent = agentForName(agents, props.agentName)
    if (!agent) return undefined
    return agentCardModel({
      name: props.agentName,
      agent,
      agents,
      threads: props.threads(),
      color: agentAvatarColor(agent),
      roster: props.roster(),
      turns: turns().map((turn) => ({
        threadId: props.thread.thread_id,
        role: turn.role,
        agentName: turn.agentName,
        content: turn.content,
      })),
    })
  }
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
            <span class="verevon-room-turn__avatar" aria-hidden="true">{initials(starterName())}</span>
            <div class="verevon-room-turn__content">
              <PostTurnHeader author={starterName()} card={starterCard()} />
              <p class="verevon-room-turn__plain">
                {(props.thread.preview && stripMarkdownPreview(props.thread.preview))
                  || props.thread.title?.trim()
                  || (transcript.loading
                    ? i18n.tr('Henter samtalen …', 'Loading the conversation…')
                    : i18n.tr('Samtalen har ikke noe lesbart innhold ennå.', 'This conversation has no readable content yet.'))}
              </p>
              <Show when={readState() === 'restricted'}>
                <SpaceRestrictedNotice variant="thread" />
              </Show>
              <Show when={readState() === 'missing'}>
                <p class="verevon-space-inline-status" role="status">
                  {i18n.tr('Denne samtalen er ikke lenger tilgjengelig.', 'This conversation is no longer available.')}
                </p>
              </Show>
              <Show when={readState() === 'error'}>
                <div class="verevon-space-inline-status" role="alert">
                  <span>{i18n.tr('Kunne ikke hente denne samtalen.', 'Could not load this conversation.')}</span>{' '}
                  <button type="button" class="verevon-space-secondary-action" onClick={() => { void refetchTranscript() }}>
                    {i18n.tr('Prøv igjen', 'Retry')}
                  </button>
                </div>
              </Show>
            </div>
          </div>
        }
      >
        <SpaceTurnList
          turns={headTurns}
          roster={props.roster}
          viewerSubjectId={props.viewerSubjectId}
          agentName={props.agentName}
          agents={props.agents}
          threads={props.threads}
          threadId={props.thread.thread_id}
          onOpenArtifact={props.artifactsOn ? setOpenArtifactId : undefined}
        />
      </Show>

      {/* The folded thread: who replied, how many, the latest reply and how long ago, as one row that
          opens the pane (Slack's reply summary, Teams' collapsed replies). Its accessible name is the
          count alone; the preview is a glance, not the label. */}
      <Show when={summarised()}>
        <button
          type="button"
          class="verevon-room-post__summary"
          aria-label={replySummaryLabel()}
          onClick={() => props.onOpenThread?.(props.thread)}
        >
          <Show when={replyParticipants().length > 0}>
            <span class="verevon-room-post__summary-avatars" aria-hidden="true">
              <For each={replyParticipants().slice(0, SUMMARY_AVATARS)}>
                {(participant) => (
                  <Show
                    when={participant.agent}
                    fallback={<span class="verevon-room-turn__avatar">{initials(participant.name)}</span>}
                  >
                    <AgentAvatar
                      kind="agent"
                      size={22}
                      name={participant.name}
                      color={agentAvatarColor(agentForName(props.agents(), participant.name))}
                    />
                  </Show>
                )}
              </For>
              <Show when={replyParticipants().length > SUMMARY_AVATARS}>
                <span class="verevon-room-post__summary-more">+{replyParticipants().length - SUMMARY_AVATARS}</span>
              </Show>
            </span>
          </Show>
          <span class="verevon-room-post__summary-count" aria-hidden="true">
            {replyCount() === 1 ? i18n.tr('1 svar', '1 reply') : i18n.tr(`${replyCount()} svar`, `${replyCount()} replies`)}
          </span>
          <Show when={lastReply()}>
            {(reply) => (
              <span class="verevon-room-post__summary-last" aria-hidden="true">
                <strong>{replyAuthorName(reply())}</strong>{' '}
                {stripMarkdownPreview(reply().content)}
              </span>
            )}
          </Show>
          <Show when={formatRelativeWhen(when())}>
            {(ago) => <span class="verevon-room-post__summary-when" aria-hidden="true">{ago()}</span>}
          </Show>
        </button>
      </Show>

      <SpacePendingReplies entries={props.pendingReplies?.() ?? []} />

      <Show when={pendingLive().length > 0}>
        <div class="verevon-space-artifact-chips">
          <For each={pendingLive()}>
            {(artifact) => <SpaceArtifactChip artifact={artifact} onOpen={setOpenArtifactId} />}
          </For>
        </div>
      </Show>

      <Show when={openArtifactId()}>
        {(selectedId) => (
          <SpaceArtifactSheet
            items={artifactItems}
            selectedId={selectedId()}
            threadTitle={props.thread.title}
            onClose={() => setOpenArtifactId(undefined)}
          />
        )}
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
          <AgentAvatar
            kind="agent"
            size={30}
            name={props.agentName}
            color={agentAvatarColor(agentForName(props.agents(), props.agentName))}
          />
          <div class="verevon-room-turn__content">
            <PostTurnHeader author={props.agentName} isAgent card={workingCard()} />
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
        {/* With replies the summary row above is the way in; this is for a thread nobody has answered. */}
        <Show when={props.onOpenThread && !summarised()}>
          <button type="button" class="verevon-room-post__reply" onClick={() => props.onOpenThread?.(props.thread)}>
            {i18n.tr('Åpne tråd', 'Open thread')}
          </button>
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
