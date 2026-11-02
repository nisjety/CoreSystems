import { Bot, MessageCircle, Pin, PinOff } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'

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
import { SpaceRunCard } from './SpaceRunCard'
import { spaceRunStore, trackLiveRuns } from '../lib/space-run-store'
import { agentAvatarColor, agentForName } from '../lib/agent-avatar'
import { agentCardModel, memberCardModel } from '../lib/agent-activity'
import type { IdentityCardModel } from '../lib/agent-activity'
import {
  MAX_TURNS_PER_POST,
  MemberAvatar,
  PostTurnHeader,
  SpaceTurnList,
  normalizeTranscriptTurns,
  resolveAuthorName,
} from './sidepane/SpaceTurnList'
import type { PostHeadMeta, PostStatus, TranscriptTurn } from './sidepane/SpaceTurnList'
import { SpaceRestrictedNotice } from './SpaceRestrictedNotice'
import { SpacePendingNewPost, SpacePendingReplies } from './SpacePendingPost'
import type { SpaceJournalEntry } from '../lib/space-send-journal'
import { spaceRoomFeatureWithDevOverride } from '../lib/space-room-features'
import { postPosition } from '../lib/timeline-anchor'
import { startAnchoring } from '../lib/timeline-anchor-dom'
import './space-timeline.css'
import { SpaceArtifactChip } from './SpaceArtifactChip'
import { SpaceArtifactSheet } from './SpaceArtifactSheet'
import { roomItems } from '@/features/spaces/lib/room-artifact-items'
import { roomArtifactsEnabled } from '@/features/spaces/lib/room-artifacts'
import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { presentSpaceDenialText, isSpaceDenial } from '@/shared/api/space-denials'
import { loadSpaceTranscript, updateSpaceThreadPresentation } from '@/shared/api/spaces-client'
import { markSpaceUnread } from '@/shared/api/spaces-attention-client'
import { refreshSpaceAttention, spaceAttentionRoom } from '../lib/space-attention'
import { firstUnreadIndex, markUnreadBeforeMs } from '../lib/space-unread'
import type { SpaceAgent, SpaceAgentWork, SpaceRoomPin, SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { announce } from '@/shared/a11y/announcer'
import { narrateNewPosts, narrateRunChange } from '@/features/spaces/lib/space-narration'
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
  /** The reader's room role, for the approval card's scope picker. Presentation only. */
  readonly viewerRole?: () => string | undefined
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
  /** The thread open in the side pane, so its post stays marked while the pane is up. */
  readonly openThreadId?: () => string | null | undefined
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
  /**
   * The room's pins from the thread listing (WS13-S8, `features.room_pins`). Pinned posts are lifted
   * above the time order, newest pin first, and say who pinned them. Absent or empty: the order and
   * the posts read as before.
   */
  readonly roomPins?: () => readonly SpaceRoomPin[]
  /**
   * The listing's `data.agent_work` (WS09-S2). With `live` true, a post whose agent is working shows the
   * run card from the room's live view instead of the polled "is working" line. Absent or off: the line,
   * exactly as before. A rollout answer, never an authorization.
   */
  readonly agentWork?: () => SpaceAgentWork | undefined
}

/**
 * How many of a post's newest turns the server returns in its tail window (WS15-S6 `tail=12`). One
 * export shared with the thread pane's inline collapse (WS01-S7b), which owns the same surface.
 */
export const TAIL_WINDOW_TURNS = 12

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

  const chronologicalThreads = createMemo(() =>
    [...props.threads()].sort((a, b) =>
      (a.updated_at ?? a.latest_run_updated_at ?? '').localeCompare(b.updated_at ?? b.latest_run_updated_at ?? ''),
    ),
  )
  // Room pins by thread id. A pin whose thread is not in the listing (archived, erased) lifts nothing.
  const pinByThread = createMemo(() => new Map((props.roomPins?.() ?? []).map((pin) => [pin.thread_id, pin])))
  // Room-pinned posts first (newest pin first), then the rest in time order.
  const orderedThreads = createMemo(() => {
    const pins = pinByThread()
    if (pins.size === 0) return chronologicalThreads()
    const pinned = chronologicalThreads()
      .filter((thread) => pins.has(thread.thread_id))
      .sort((a, b) => (pins.get(b.thread_id)?.pinned_at ?? 0) - (pins.get(a.thread_id)?.pinned_at ?? 0))
    return [...pinned, ...chronologicalThreads().filter((thread) => !pins.has(thread.thread_id))]
  })

  const agentName = createMemo(() => roomAgentName(props.agents()))

  // ── Screen-reader narration (WS16-S4) ────────────────────────────────────────────────────────────
  // The poll replaces the thread listing; the difference between two listings is what changed under a
  // reader who cannot see it. A listing for another room, or the first one, is a baseline, not news.
  let narratedRef: string | undefined
  let narratedThreads: readonly SpaceThread[] | undefined
  createEffect(
    () => ({ ref: props.spaceRef, threads: props.threads() }),
    ({ ref, threads }) => {
      const prev = narratedRef === ref ? narratedThreads : undefined
      narratedRef = ref
      narratedThreads = threads
      if (!prev) return
      const roster = props.roster()
      const context = {
        roomName: props.spaceName,
        nameOf: (subjectId: string | undefined) =>
          resolveAuthorName(subjectId, roster, props.viewerSubjectId?.(), i18n.tr),
      }
      const items = [
        ...narrateNewPosts(prev, threads, props.viewerSubjectId?.(), i18n.tr, context),
        ...narrateRunChange(prev, threads, i18n.tr),
      ]
      for (const item of items) announce(item.message, item.options)
    },
  )
  // The "new messages" divider goes once, above the first post (oldest first) that is new since the
  // reader arrived; the posts after it are new too, so one line says it for all of them.
  const firstUnreadId = createMemo(() => {
    const unread = props.unreadThreadIds?.()
    if (!unread || unread.size === 0) return undefined
    return orderedThreads()[firstUnreadIndex(orderedThreads(), unread)]?.thread_id
  })
  const unreadCount = () => props.unreadThreadIds?.().size ?? 0
  // WS11-S4: bring the first new post into view and move focus onto it (the reader's keyboard and a
  // screen reader land where the news starts), without a second scroll by the focus itself.
  function jumpToUnread(): void {
    const id = firstUnreadId()
    if (!id || typeof document === 'undefined') return
    const post = document.getElementById(`post-${id}`)
    if (!post) return
    post.scrollIntoView({ block: 'start', behavior: reduceMotion() ? 'auto' : 'smooth' })
    if (!post.hasAttribute('tabindex')) post.setAttribute('tabindex', '-1')
    post.focus({ preventScroll: true })
  }
  const sharedRead = () => props.sharedReadAvailable?.() !== false
  const artifactsOn = roomArtifactsEnabled()
  const pendingNewPosts = () =>
    (props.pendingPosts?.() ?? []).filter((entry) => entry.kind === 'post_new')
  const pendingRepliesFor = (threadId: string) =>
    (props.pendingPosts?.() ?? []).filter((entry) => entry.kind === 'post_reply' && entry.threadId === threadId)

  // ── Scroll anchoring (WS15-S1, `features.timeline_anchor`) ───────────────────────────────────────
  // Off, none of this runs and the list renders exactly as before.
  const anchored = () => spaceRoomFeatureWithDevOverride('timeline_anchor', import.meta.env.VITE_SPACE_TIMELINE_ANCHOR_UI)
  const [unseenNew, setUnseenNew] = createSignal(0)
  const [jumpVisible, setJumpVisible] = createSignal(false)
  let listEl: HTMLOListElement | undefined
  let runTick: (() => void) | undefined
  let ownSendPending = false
  let ownSendTimer: ReturnType<typeof setTimeout> | undefined

  const reduceMotion = () =>
    typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
  function jumpToLatest(): void {
    if (!listEl) return
    listEl.scrollTo({ top: listEl.scrollHeight, behavior: reduceMotion() ? 'auto' : 'smooth' })
    setUnseenNew(0)
    setJumpVisible(false)
  }

  // The reader's own send always goes to the bottom: a new journal entry means this browser just posted.
  let lastPendingCount = props.pendingPosts?.().length ?? 0
  createEffect(
    () => props.pendingPosts?.().length ?? 0,
    (count) => {
      const grew = count > lastPendingCount
      lastPendingCount = count
      if (!grew || !anchored()) return
      ownSendPending = true
      clearTimeout(ownSendTimer)
      ownSendTimer = setTimeout(() => { ownSendPending = false }, 2000)
      if (runTick) requestAnimationFrame(runTick)
    },
  )

  // The controller. Solid v2 has no onMount: this effect runs once the list is in the document, and
  // again if the flag flips; the function it returns tears the observers down.
  createEffect(
    () => ({ on: anchored(), has: orderedThreads().length > 0 }),
    ({ on, has }) => {
      const el = listEl
      if (!on || !has || !el) return undefined
      return startAnchoring(el, {
        startAtBottom: !props.openThreadId?.(),
        takeOwnSend: () => {
          const own = ownSendPending
          ownSendPending = false
          return own
        },
        setUnseen: setUnseenNew,
        setJump: setJumpVisible,
        unseen: unseenNew,
        register: (tick) => { runTick = tick },
      })
    },
  )
  onCleanup(() => {
    clearTimeout(ownSendTimer)
    runTick = undefined
  })

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
      <Show when={firstUnreadId()}>
        <button type="button" class="verevon-room-jump-unread" onClick={jumpToUnread}>
          {i18n.tr('Hopp til første uleste', 'Jump to unread')}
        </button>
      </Show>
      <ol
        class="verevon-room-timeline"
        ref={(element) => { listEl = element }}
        data-anchored={anchored() ? '' : undefined}
        aria-label={i18n.tr('Samtaler i rommet', 'Room conversations')}
      >
        <For each={orderedThreads()}>
          {(thread, index) => (
            <>
            <Show when={firstUnreadId() === thread.thread_id}>
              <li class="verevon-room-new-divider-item" role="presentation">
                <div
                  class="verevon-room-new-divider"
                  role="separator"
                  aria-label={i18n.tr(`${unreadCount()} nye`, `${unreadCount()} new`)}
                >
                  <span aria-hidden="true">{i18n.tr(`${unreadCount()} nye`, `${unreadCount()} new`)}</span>
                </div>
              </li>
            </Show>
            <SpaceRoomPost
              spaceRef={props.spaceRef}
              thread={thread}
              position={anchored() ? postPosition(index(), orderedThreads().length) : undefined}
              threads={props.threads}
              roster={props.roster}
              viewerSubjectId={props.viewerSubjectId}
              viewerRole={props.viewerRole}
              agentName={agentName()}
              agents={props.agents}
              transcriptEnabled={sharedRead}
              artifactsOn={artifactsOn}
              liveArtifacts={props.liveArtifacts}
              unread={() => props.unreadThreadIds?.().has(thread.thread_id) === true}
              roomPin={() => pinByThread().get(thread.thread_id)}
              agentWork={props.agentWork}
              open={() => props.openThreadId?.() === thread.thread_id}
              onPresentationChanged={props.onPresentationChanged}
              onReply={props.onReply}
              onOpenThread={props.onOpenThread}
              onApprovalSettled={props.onApprovalSettled}
              pendingReplies={() => pendingRepliesFor(thread.thread_id)}
              collapse={() => props.collapseLongThreads?.() === true && props.onOpenThread !== undefined}
            />
            </>
          )}
        </For>
        <For each={pendingNewPosts()}>{(entry) => <SpacePendingNewPost entry={entry} />}</For>
        {/* The jump pill: sticks to the bottom edge of the list while the reader is scrolled up. */}
        <Show when={anchored() && jumpVisible()}>
          <li class="verevon-room-jump-item" role="presentation">
            <button
              type="button"
              class="verevon-room-jump"
              data-new={unseenNew() > 0 ? 'true' : undefined}
              onClick={jumpToLatest}
            >
              {unseenNew() > 0
                ? unseenNew() === 1
                  ? i18n.tr('1 ny', '1 new')
                  : i18n.tr(`${unseenNew()} nye`, `${unseenNew()} new`)
                : i18n.tr('Hopp til siste', 'Jump to latest')}
            </button>
          </li>
        </Show>
      </ol>
    </Show>
  )
}

function SpaceRoomPost(props: {
  readonly spaceRef: string
  readonly thread: SpaceThread
  /** `aria-posinset`/`aria-setsize` for this post (`features.timeline_anchor`); absent leaves them off. */
  readonly position?: { readonly posinset: number; readonly setsize: number }
  /** Every thread in the room, for the identity hover card's activity line. */
  readonly threads: () => readonly SpaceThread[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
  readonly viewerRole?: () => string | undefined
  readonly agentName: string
  /** Roster agents, for the avatar colour of an assistant turn (exactly-one-name match, else neutral). */
  readonly agents: () => readonly SpaceAgent[]
  /** False when the room's shared read is unavailable: no transcript call is made. */
  readonly transcriptEnabled: () => boolean
  /** `features.room_artifacts`: chips, sheet and artifact-only turns. Off, the post reads as before. */
  readonly artifactsOn: boolean
  readonly liveArtifacts?: (threadId: string) => readonly ChatArtifact[]
  /** This post's room pin (WS13-S8), when the room has pinned it. */
  readonly roomPin?: () => SpaceRoomPin | undefined
  /** The listing's `data.agent_work`; `live` turns on the run card for a working post (WS09-S5). */
  readonly agentWork?: () => SpaceAgentWork | undefined
  readonly onReply?: (thread: SpaceThread) => void
  readonly onApprovalSettled?: () => void
  /** New since the reader arrived — see `SpaceRoomTimelineProps.unreadThreadIds`. */
  readonly unread?: () => boolean
  /** This post's thread is the one open in the side pane. */
  readonly open?: () => boolean
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
      if (load.state === 'ok') {
        // Every turn is kept: the post draws its first turn plus the newest ones and offers the rest
        // behind the "earlier turns" row, instead of dropping everything after the first thirty.
        return {
          state: 'ok' as const,
          turns: normalizeTranscriptTurns(load.turns as readonly unknown[] | undefined, Number.MAX_SAFE_INTEGER),
        }
      }
      // A 403 may carry a typed Space denial (why Control said no, and who can fix it). Kept so the
      // post can say that, not only the generic "not shared with you".
      const denial = load.state === 'restricted' ? load.denial : undefined
      return { state: load.state, turns: [] as readonly TranscriptTurn[], denial }
    },
  )
  // With the artifact surfaces off an artifact-only turn has nothing to show,
  // so it is dropped here and the post reads exactly as it did before.
  const allTurns = (): readonly TranscriptTurn[] => {
    const all = transcript()?.turns ?? []
    return props.artifactsOn ? all : all.filter((turn) => turn.content.length > 0)
  }
  // A long transcript is drawn as its first turn plus the newest ones, with a row to show the rest.
  const [turnsExpanded, setTurnsExpanded] = createSignal(false)
  const turnWindow = (): { readonly turns: readonly TranscriptTurn[]; readonly omitted: number } => {
    const all = allTurns()
    if (turnsExpanded() || all.length <= MAX_TURNS_PER_POST) return { turns: all, omitted: 0 }
    return {
      turns: [all[0] as TranscriptTurn, ...all.slice(all.length - (MAX_TURNS_PER_POST - 1))],
      omitted: all.length - MAX_TURNS_PER_POST,
    }
  }
  const turns = (): readonly TranscriptTurn[] => turnWindow().turns
  const [openArtifactId, setOpenArtifactId] = createSignal<string | undefined>(undefined)
  // Live artifacts from this browser's own stream that the transcript does not
  // carry yet (same id and version). In memory only, never stored.
  const pendingLive = (): readonly ChatArtifact[] => {
    if (!props.artifactsOn) return []
    const persisted = new Set(
      allTurns().flatMap((turn) => (turn.artifacts ?? []).map((artifact) => `${artifact.id}\u0000${artifact.version}`)),
    )
    return (props.liveArtifacts?.(props.thread.thread_id) ?? []).filter(
      (artifact) => !persisted.has(`${artifact.id}\u0000${artifact.version}`),
    )
  }
  const artifactItems = createMemo(() =>
    props.artifactsOn ? roomItems([...allTurns(), { artifacts: pendingLive() }]) : [],
  )
  const readState = () => transcript()?.state
  // The typed denial a 403 carried, when it carried one this build understands; else undefined and the
  // thread notice speaks (shared reads off for the Space).
  const typedDenial = (): unknown => {
    const denial = (transcript() as { denial?: unknown } | undefined)?.denial
    return isSpaceDenial(denial) ? denial : undefined
  }
  // A thread with replies folds to its root message plus one summary row that opens the pane. The
  // count is what the pane will show (every turn after the root, agent answers included); until the
  // transcript has loaded the projection's `reply_count` stands in.
  const replyCount = () => {
    if (props.collapse?.() !== true) return 0
    if (allTurns().length > 0) return allTurns().length - 1
    const count = props.thread.reply_count
    return typeof count === 'number' && count > 0 ? count : 0
  }
  const summarised = () => replyCount() > 0
  const headTurns = (): readonly TranscriptTurn[] => (summarised() && turns().length > 0 ? turns().slice(0, 1) : turns())
  const replyTurns = (): readonly TranscriptTurn[] => allTurns().slice(1)
  const earlierTurns = () => {
    const omitted = turnWindow().omitted
    return omitted > 0 && !summarised() ? { count: omitted, onExpand: () => setTurnsExpanded(true) } : undefined
  }
  const replyAuthorName = (turn: TranscriptTurn): string =>
    turn.role === 'assistant'
      ? turn.agentName ?? props.agentName
      : resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // Distinct reply authors in the order they first spoke. Agents and members are told apart by what
  // the server recorded; an unnamed author is one participant, not one per turn.
  const replyParticipants = () => {
    const seen = new Set<string>()
    const out: { key: string; agent: boolean; name: string; subjectId?: string }[] = []
    for (const turn of replyTurns()) {
      const agent = turn.role === 'assistant'
      const name = replyAuthorName(turn)
      const key = `${agent ? 'agent' : 'user'}\u0000${agent ? name : turn.authorSubjectId ?? ''}`
      if (seen.has(key)) continue
      seen.add(key)
      out.push({ key, agent, name, subjectId: turn.authorSubjectId })
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
      turns: allTurns().map((turn) => ({
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
  // The live view of someone else's run (WS09-S5). The attach is made only when the listing says the
  // live view is on and the post's agent is working; the BFF and Session Core authorize every attach.
  // The card shows once the first snapshot has arrived and only for a run somebody ELSE started: the
  // person who started it has the live exchange in their own composer, so they keep the plain row.
  const liveRunId = () => {
    if (props.agentWork?.()?.live !== true || !isWorking()) return undefined
    return props.thread.latest_run_id?.trim() || undefined
  }
  trackLiveRuns(
    () => props.spaceRef,
    () => {
      const id = liveRunId()
      return id ? [id] : []
    },
    () => liveRunId() !== undefined,
  )
  const runView = () => {
    const id = liveRunId()
    return id ? spaceRunStore.view(id) : undefined
  }
  const runCardView = () => {
    const view = runView()
    if (!view || view.status !== 'live' || view.finished) return undefined
    const viewer = props.viewerSubjectId?.()
    return viewer && view.startedBy === viewer ? undefined : view
  }
  // Only a run that is actually paused gets a decision surface, and only when
  // the projection named the run — an approval card with no run to decide on
  // would be a control that cannot do anything.
  const awaitingApproval = () =>
    props.thread.latest_run_status === AWAITING_APPROVAL_RUN_STATUS
    && Boolean(props.thread.latest_run_id?.trim())
  const when = () => props.thread.updated_at ?? props.thread.latest_run_updated_at
  // The status and the date sit beside the author of the post's first row, where Slack and Teams put
  // the time, not in a footer line. A finished post, or one with no run, says nothing visible (the
  // quiet label below keeps it in the accessibility tree), and a working post says it on the agent's
  // working row instead of on the first row.
  const quiet = () => !props.thread.latest_run_status || props.thread.latest_run_status === 'completed'
  const statusTone = (): PostStatus['tone'] => (isFailed() ? 'failed' : isWorking() ? 'active' : 'neutral')
  const statusNow = (): PostStatus | undefined =>
    quiet() ? undefined : { label: threadStatus(props.thread, i18n.tr), tone: statusTone() }
  const headMeta = (): PostHeadMeta => {
    const at = when()
    return {
      ...(at ? { dateLabel: formatWhen(at, { locale: i18n.locale() }) } : {}),
      ...(isWorking() ? {} : { status: statusNow() }),
    }
  }
  // Pinning is owner-bound in Session Core, so the control is offered only to
  // the member who started the post. Anyone else sees the pin, not the button:
  // a control that would be refused every time is a control that lies.
  const isAuthor = () => {
    const viewer = props.viewerSubjectId?.()?.trim()
    return Boolean(viewer) && props.thread.owner_subject_id?.trim() === viewer
  }
  // WS11-S4: "Mark unread" moves the room's read position to just before this post. Offered only
  // once the attention aggregate has answered for this room (so the surface is on) and the post has a
  // readable time. The page's routine `while_open` poll leaves the manual floor alone; coming back
  // into the room (`enter`) clears it.
  const unreadFrom = () => (spaceAttentionRoom(props.spaceRef) ? markUnreadBeforeMs(props.thread) : undefined)
  const [unreadBusy, setUnreadBusy] = createSignal(false)
  async function markUnreadHere(): Promise<void> {
    const before = unreadFrom()
    if (before === undefined || unreadBusy()) return
    setUnreadBusy(true)
    try {
      await markSpaceUnread(props.spaceRef, before)
      announce(i18n.tr('Merket som ulest herfra.', 'Marked unread from here.'), { key: 'space-mark-unread' })
      refreshSpaceAttention()
    } catch (error) {
      announce(translateApiError(error, i18n.tr), { politeness: 'assertive' })
    } finally {
      setUnreadBusy(false)
    }
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
      id={`post-${props.thread.thread_id}`}
      data-thread-id={props.thread.thread_id}
      data-failed={isFailed() || undefined}
      data-pinned={props.thread.pinned === true || props.roomPin?.() ? 'true' : undefined}
      data-unread={props.unread?.() ? 'true' : undefined}
      data-open={props.open?.() ? 'true' : undefined}
      data-run-status={props.thread.latest_run_status ?? undefined}
      aria-posinset={props.position?.posinset}
      aria-setsize={props.position?.setsize}
    >
      {/* Pinned and new are facts about the post, stated before its content so
          a reader scanning the room sees them first. "New" is drawn once for the whole room as the
          divider line above the first new post; each new post keeps this label for assistive tech
          only (the stylesheet hides it visually), so the fact is still stated per post. */}
      <Show when={props.thread.pinned === true || props.roomPin?.() !== undefined || props.unread?.()}>
        <div class="verevon-room-post__flags">
          <Show when={props.roomPin?.()}>
            {(pin) => (
              <span class="verevon-room-post__flag verevon-room-post__flag--pinned">
                <Pin size={12} aria-hidden="true" />
                {i18n.tr(
                  `Festet av ${resolveAuthorName(pin().pinned_by, props.roster(), props.viewerSubjectId?.(), i18n.tr)}`,
                  `Pinned by ${resolveAuthorName(pin().pinned_by, props.roster(), props.viewerSubjectId?.(), i18n.tr)}`,
                )}
              </span>
            )}
          </Show>
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
            <MemberAvatar name={starterName()} subjectId={props.thread.owner_subject_id} />
            <div class="verevon-room-turn__content">
              <PostTurnHeader
                author={starterName()}
                card={starterCard()}
                dateLabel={headMeta().dateLabel}
                status={headMeta().status}
              />
              <p class="verevon-room-turn__plain">
                {(props.thread.preview && stripMarkdownPreview(props.thread.preview))
                  || props.thread.title?.trim()
                  || (transcript.loading
                    ? i18n.tr('Henter samtalen …', 'Loading the conversation…')
                    : i18n.tr('Samtalen har ikke noe lesbart innhold ennå.', 'This conversation has no readable content yet.'))}
              </p>
              <Show when={readState() === 'restricted'}>
                <Show when={typedDenial()} fallback={<SpaceRestrictedNotice variant="thread" />}>
                  {(denial) => (
                    <p class="verevon-space-inline-status" role="status" data-denial="typed">
                      {presentSpaceDenialText(denial(), i18n.tr)}
                    </p>
                  )}
                </Show>
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
          headMeta={headMeta}
          earlierTurns={earlierTurns}
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
                    fallback={<MemberAvatar name={participant.name} subjectId={participant.subjectId} />}
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
          <Show when={formatRelativeWhen(when(), undefined, { locale: i18n.locale() })}>
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
          <SpaceApprovalPanel
            runId={runId()}
            onSettled={props.onApprovalSettled}
            spaceRef={props.spaceRef}
            viewerRole={props.viewerRole?.()}
          />
        )}
      </Show>

      {/* The live row. `aria-live="polite"` so a reader using assistive tech
          hears that work started without the row shouting over the transcript;
          it disappears on its own when the projection reports a terminal
          status, which is the "mutate in place" rule from the activity grammar
          rather than a new line per transition. */}
      <Show when={runCardView()}>
        {(view) => (
          <div class="verevon-room-turn verevon-room-turn--working" data-run-card="true">
            <AgentAvatar
              kind="agent"
              size={30}
              name={view().agentName || props.agentName}
              color={agentAvatarColor(agentForName(props.agents(), view().agentName || props.agentName))}
            />
            <div class="verevon-room-turn__content">
              <PostTurnHeader author={view().agentName || props.agentName} isAgent card={workingCard()} status={statusNow()} />
              <SpaceRunCard
                view={view()}
                startedByName={
                  view().startedBy
                    ? resolveAuthorName(view().startedBy, props.roster(), props.viewerSubjectId?.(), i18n.tr)
                    : ''
                }
              />
            </div>
          </div>
        )}
      </Show>

      <Show when={isWorking() && !runCardView()}>
        <div class="verevon-room-turn verevon-room-turn--working" role="status" aria-live="polite">
          <AgentAvatar
            kind="agent"
            size={30}
            name={props.agentName}
            color={agentAvatarColor(agentForName(props.agents(), props.agentName))}
          />
          <div class="verevon-room-turn__content">
            <PostTurnHeader author={props.agentName} isAgent card={workingCard()} status={statusNow()} />
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
        {/* Only the finished (or run-less) post keeps its status here, hidden by the stylesheet but
            read by assistive tech; any other status is said beside the author above. */}
        <Show when={quiet()}>
          <span class="verevon-space-status verevon-room-post__status-quiet">
            {threadStatus(props.thread, i18n.tr)}
          </span>
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
        <Show when={unreadFrom() !== undefined}>
          <button
            type="button"
            class="verevon-room-post__reply"
            disabled={unreadBusy()}
            onClick={() => { void markUnreadHere() }}
          >
            {i18n.tr('Merk som ulest', 'Mark unread')}
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
