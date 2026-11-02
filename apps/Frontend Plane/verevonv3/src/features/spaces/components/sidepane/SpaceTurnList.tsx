import { Bot } from '@/shared/icons'
import { createSignal, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { SpaceArtifactChip } from '../SpaceArtifactChip'
import { spaceReactionsFrom, spaceRunWorkFrom } from '@/shared/api/spaces-client'
import type {
  SpaceAgent,
  SpaceReactionChip,
  SpaceRosterMember,
  SpaceRunWork,
  SpaceThread,
} from '@/shared/api/spaces-client'
import { SpaceRunWorkSummary } from '../SpaceRunCard'
import { SpaceMessageEdit } from '../SpaceMessageEdit'
import { SpaceMessageToolbar } from '../SpaceMessageToolbar'
import { SpaceReactionBar } from '../SpaceReactionBar'
import type { SpaceTurnLifecycle } from '../../lib/space-message-lifecycle'
import { translateApiError, useI18n } from '@/shared/i18n'
import { AgentAvatar } from '../AgentAvatar'
import { SpaceIdentityHoverCard } from '../SpaceIdentityHoverCard'
import { agentAvatarColor, agentForName, memberTint } from '../../lib/agent-avatar'
import { splitRoomMessage } from '../../lib/room-attachments'
import { spaceRoomFeature, spaceRoomFeatureWithDevOverride } from '../../lib/space-room-features'
import { MentionText } from '../MentionText'
import { WakeStatus } from '../WakeStatus'
import { agentCardModel, memberCardModel } from '../../lib/agent-activity'
import type { IdentityCardModel } from '../../lib/agent-activity'
import { continuesTurnGroup, formatTurnTime } from '../../lib/space-thread-presentation'
import {
  asSpaceRetrievalDeclineReason,
  spaceRetrievalDeclineCopy,
  type SpaceRetrievalDeclineReason,
} from '../../lib/space-retrieval-decline'

/**
 * The rows of one thread, read-only. Extracted from the room timeline so the
 * same rows render inline in a post and in the side pane.
 *
 * Turn attribution is server-side. Each turn carries the subject Model Plane
 * recorded as its author at append time, and this list resolves that id against
 * Control's roster for a display name. It never infers an author from who is
 * looking: an unrecorded author renders as unnamed, because the one thing worse
 * than an unlabelled turn is a turn labelled with the wrong person.
 */

export type TranscriptTurn = {
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
  /** The server's message id, when it sent one. */
  readonly messageId?: string
  /** When the server stored this turn, as an ISO-8601 UTC string. Absent when it sent none. */
  readonly createdAt?: string
  /** Artifacts this assistant turn persisted (already through the room's gate). */
  readonly artifacts?: readonly ChatArtifact[]
  /** Why this assistant turn answered without the organisation's knowledge (D1-242), when Control
   * declined retrieval for the Space. A reason from the closed set; absent otherwise. */
  readonly retrievalDeclined?: SpaceRetrievalDeclineReason
  /** The content-free work summary of this assistant turn (WS09-S3 `metadata.work`), re-read through the closed shape. */
  readonly work?: SpaceRunWork
  /** WS12-S7: edited or deleted. A deleted turn has no text and is drawn as a tombstone. */
  readonly revisionState?: 'edited' | 'deleted'
  /** When the latest change was applied (ISO-8601 UTC). */
  readonly revisedAt?: string
  /** Who applied it, as the server recorded. */
  readonly revisedBySubjectId?: string
  /** A deleted message an agent run had already answered. */
  readonly agentFollowedUp?: boolean
  /** WS12-S8: emoji reactions on this message, as the server merged them. */
  readonly reactions?: readonly SpaceReactionChip[]
}

export const MAX_TURNS_PER_POST = 30

export function normalizeTranscriptTurns(
  turns: readonly unknown[] | undefined,
  maxTurns: number = MAX_TURNS_PER_POST,
): TranscriptTurn[] {
  if (!turns) return []
  const normalized: TranscriptTurn[] = []
  for (const turn of turns) {
    if (!turn || typeof turn !== 'object') continue
    const record = turn as Record<string, unknown>
    const role = record.role
    const content = typeof record.content === 'string' ? record.content.trim() : ''
    if (role !== 'user' && role !== 'assistant') continue
    const artifacts = role === 'assistant' && Array.isArray(record.artifacts)
      ? (record.artifacts as readonly ChatArtifact[])
      : []
    // A deleted message arrives with empty text and stays as a tombstone row (WS12-S7).
    const revisionState = record.revisionState === 'edited' || record.revisionState === 'deleted'
      ? record.revisionState
      : undefined
    // An artifact-only assistant turn is kept: it is the turn the chip hangs on.
    if (!content && artifacts.length === 0 && revisionState !== 'deleted') continue
    const revisedAt = typeof record.revisedAt === 'string' && Number.isFinite(Date.parse(record.revisedAt))
      ? new Date(record.revisedAt).toISOString()
      : undefined
    const revisedBySubjectId = typeof record.revisedBySubjectId === 'string' && record.revisedBySubjectId.trim()
      ? record.revisedBySubjectId.trim()
      : undefined
    const messageId = typeof record.messageId === 'string' && record.messageId.trim()
      ? record.messageId.trim()
      : undefined
    const agentName = typeof record.agentName === 'string' && record.agentName.trim()
      ? record.agentName.trim()
      : undefined
    const authorSubjectId = typeof record.authorSubjectId === 'string' && record.authorSubjectId.trim()
      ? record.authorSubjectId.trim()
      : undefined
    // Already normalized by the transcript client; re-checked here so a turn from any other source
    // can never carry something that is not a time.
    const createdAt = typeof record.createdAt === 'string' && Number.isFinite(Date.parse(record.createdAt))
      ? new Date(record.createdAt).toISOString()
      : undefined
    // Already a bare reason from the transcript client; re-checked against the closed set here so a turn from
    // any other source can only carry a reason the room has words for.
    const retrievalDeclined = role === 'assistant' && typeof record.retrievalDeclined === 'string'
      ? asSpaceRetrievalDeclineReason(record.retrievalDeclined)
      : undefined
    const work = role === 'assistant' ? spaceRunWorkFrom(record.work) : undefined
    const reactions = revisionState === 'deleted' ? [] : spaceReactionsFrom(record.reactions)
    normalized.push({
      role,
      content,
      ...(agentName ? { agentName } : {}),
      ...(authorSubjectId ? { authorSubjectId } : {}),
      ...(messageId ? { messageId } : {}),
      ...(createdAt ? { createdAt } : {}),
      ...(artifacts.length > 0 ? { artifacts } : {}),
      ...(retrievalDeclined ? { retrievalDeclined } : {}),
      ...(work ? { work } : {}),
      ...(revisionState ? { revisionState } : {}),
      ...(revisionState && revisedAt ? { revisedAt } : {}),
      ...(revisionState && revisedBySubjectId ? { revisedBySubjectId } : {}),
      ...(revisionState === 'deleted' && record.agentFollowedUp === true ? { agentFollowedUp: true } : {}),
      ...(reactions.length > 0 ? { reactions } : {}),
    })
  }
  // Over the cap, keep the first turn (the post's opening message) and the NEWEST turns, never the
  // oldest thirty: a long thread used to show its start and silently lose its latest answers.
  if (maxTurns >= 2 && normalized.length > maxTurns) {
    return [normalized[0] as TranscriptTurn, ...normalized.slice(normalized.length - (maxTurns - 1))]
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

/** A post's run status, said beside the author of its first row (nothing is said for a finished post). */
export type PostStatus = {
  readonly label: string
  readonly tone: 'active' | 'failed' | 'neutral'
}

/** What the first row of a post carries beside its author, when the turn itself has no time. */
export type PostHeadMeta = {
  /** The post's date ("27. okt."), shown only when the first turn has no time of its own. */
  readonly dateLabel?: string
  readonly status?: PostStatus
}

export interface SpaceTurnListProps {
  readonly turns: () => readonly TranscriptTurn[]
  readonly roster: () => readonly SpaceRosterMember[]
  readonly viewerSubjectId?: () => string | undefined
  /** Room-level fallback name for an assistant turn that recorded no persona. */
  readonly agentName: string
  /**
   * The room roster's agents, for the avatar colour of an assistant turn. A turn
   * maps to an agent only when its name matches exactly one; absent (the side
   * pane) every agent avatar is neutral.
   */
  readonly agents?: () => readonly SpaceAgent[]
  /**
   * The room's threads and this list's own thread id. When `threads` is given,
   * the author names open the identity hover card (the side pane omits it and
   * keeps plain names).
   */
  readonly threads?: () => readonly SpaceThread[]
  readonly threadId?: string
  /**
   * When provided, an assistant turn's artifacts render as chips under it and
   * pressing one calls this with the artifact id. Absent (the side pane), the
   * artifacts are not shown and an artifact-only turn renders no row at all.
   */
  readonly onOpenArtifact?: (artifactId: string) => void
  /** The room timeline's post date and status, drawn in the header of the first row. */
  readonly headMeta?: () => PostHeadMeta | undefined
  /**
   * Turns left out between the first row and the rest (WS15-S1). When present, a row "N earlier turns"
   * is drawn under the first turn and calls `onExpand`; the turn after the gap never groups with the
   * one above it.
   */
  readonly earlierTurns?: () => { readonly count: number; readonly onExpand: () => void } | undefined
  /**
   * Edit and delete controls (WS12-S7). Absent, the rows are read-only and an edited or deleted
   * message still shows its marker or tombstone.
   */
  readonly lifecycle?: SpaceTurnLifecycle
  /** The longest edit the room accepts, in characters. */
  readonly editMaxChars?: number
}

export function SpaceTurnList(props: SpaceTurnListProps) {
  const i18n = useI18n()
  const humanMarkdownOn = () =>
    spaceRoomFeatureWithDevOverride('human_markdown', import.meta.env.VITE_SPACE_HUMAN_MARKDOWN)
  const authorOf = (turn: TranscriptTurn) =>
    resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // A turn with no text is only a row when its artifacts can be shown.
  const cardFor = (turn: TranscriptTurn): IdentityCardModel | undefined => {
    const threads = props.threads?.()
    if (!threads) return undefined
    const name = turn.role === 'assistant' ? turn.agentName ?? props.agentName : authorOf(turn)
    if (turn.role === 'user') {
      return turn.authorSubjectId
        ? memberCardModel({ name, subjectId: turn.authorSubjectId, roster: props.roster(), threads })
        : undefined
    }
    const agents = props.agents?.() ?? []
    const agent = agentForName(agents, name)
    if (!agent) return undefined
    return agentCardModel({
      name,
      agent,
      agents,
      threads,
      color: agentAvatarColor(agent),
      roster: props.roster(),
      turns: props.turns().map((entry) => ({
        threadId: props.threadId,
        role: entry.role,
        agentName: entry.agentName,
        content: entry.content,
      })),
    })
  }
  const visibleTurns = () =>
    props.turns().filter(
      (turn) =>
        turn.content.length > 0
        || turn.revisionState === 'deleted'
        || (props.onOpenArtifact && (turn.artifacts?.length ?? 0) > 0),
    )
  // The turn's own recorded persona wins; the room-level heuristic (single active agent, else
  // 'Verevon') covers turns persisted before attribution existed. A human turn is named by the
  // subject the server recorded, never by who happens to be reading.
  const nameOf = (turn: TranscriptTurn) => (turn.role === 'assistant' ? turn.agentName ?? props.agentName : authorOf(turn))
  return (
    <For each={visibleTurns()}>
      {(turn, index) => {
        // Slack-style grouping: a turn by the same author within five minutes of the one above it
        // drops its avatar and name and sits under that turn's text.
        const grouped = () =>
          !(index() === 1 && props.earlierTurns?.() !== undefined)
          && continuesTurnGroup(visibleTurns()[index() - 1], turn, props.agentName)
        const time = () => formatTurnTime(turn.createdAt)
        // WS12-S7: per-row edit state. A refetch after a change builds new turn objects, which
        // closes the editor and clears any error.
        const [editing, setEditing] = createSignal(false)
        const [busy, setBusy] = createSignal(false)
        const [lifecycleError, setLifecycleError] = createSignal<string | undefined>(undefined)
        const deleted = () => turn.revisionState === 'deleted'
        // WS12-S8: the chips after this viewer's own toggle; a refetch builds new turn objects and a
        // fresh row, so the server's merged list takes over again.
        const [chipsOverride, setChipsOverride] = createSignal<readonly SpaceReactionChip[] | undefined>(undefined)
        const [reactBusy, setReactBusy] = createSignal(false)
        const chips = () => chipsOverride() ?? turn.reactions ?? []
        const toggleReaction = async (emoji: string, present: boolean) => {
          const lifecycle = props.lifecycle
          if (!lifecycle || reactBusy()) return
          setReactBusy(true)
          setLifecycleError(undefined)
          try {
            setChipsOverride(await lifecycle.react(turn, emoji, present))
          } catch (error) {
            setLifecycleError(
              translateApiError(error, i18n.tr, {
                no: 'Reaksjonen kunne ikke endres. Ingenting er endret. Prøv igjen om litt.',
                en: 'The reaction could not be changed. Nothing was changed. Try again shortly.',
              }),
            )
          } finally {
            setReactBusy(false)
          }
        }
        const removeTurn = async () => {
          const lifecycle = props.lifecycle
          if (!lifecycle || busy()) return
          setBusy(true)
          setLifecycleError(undefined)
          try {
            await lifecycle.remove(turn)
          } catch (error) {
            setLifecycleError(
              translateApiError(error, i18n.tr, {
                no: 'Meldingen kunne ikke slettes. Ingenting er endret. Prøv igjen om litt.',
                en: 'The message could not be deleted. Nothing was changed. Try again shortly.',
              }),
            )
          } finally {
            setBusy(false)
          }
        }
        const tombstoneText = () => {
          const byOther = turn.role === 'user'
            && turn.revisedBySubjectId !== undefined
            && turn.revisedBySubjectId !== turn.authorSubjectId
          if (turn.role === 'assistant') return i18n.tr('Dette svaret er slettet.', 'This answer was deleted.')
          return byOther
            ? i18n.tr('Meldingen ble fjernet av en leder.', 'This message was removed by a manager.')
            : i18n.tr('Meldingen er slettet.', 'This message was deleted.')
        }
        return (
          <>
          <div
            class="verevon-room-turn"
            data-role={turn.role}
            data-grouped={grouped() ? 'true' : undefined}
            data-message-id={turn.messageId}
            tabindex={turn.messageId ? -1 : undefined}
          >
            <Show
              when={!grouped()}
              fallback={
                <span class="verevon-room-turn__gutter">
                  <Show when={time()}>
                    {(at) => (
                      <time class="verevon-room-turn__time" datetime={at().iso} title={at().full}>{at().short}</time>
                    )}
                  </Show>
                </span>
              }
            >
              <Show
                when={turn.role === 'assistant'}
                fallback={<MemberAvatar name={authorOf(turn)} subjectId={turn.authorSubjectId} />}
              >
                <AgentAvatar
                  kind="agent"
                  size={30}
                  name={turn.agentName ?? props.agentName}
                  color={agentAvatarColor(agentForName(props.agents?.() ?? [], turn.agentName ?? props.agentName))}
                />
              </Show>
            </Show>
            <div class="verevon-room-turn__content">
              <Show when={props.lifecycle && !editing() && !deleted() && turn.messageId}>
                <SpaceMessageToolbar
                  canEdit={props.lifecycle?.canEdit(turn) === true}
                  canDelete={props.lifecycle?.canDelete(turn) === true}
                  moderating={turn.role === 'user' && props.lifecycle?.canEdit(turn) !== true}
                  busy={busy()}
                  onEdit={() => {
                    setLifecycleError(undefined)
                    setEditing(true)
                  }}
                  onDelete={() => { void removeTurn() }}
                />
              </Show>
              <Show when={!grouped()}>
                <PostTurnHeader
                  author={nameOf(turn)}
                  isAgent={turn.role === 'assistant'}
                  card={cardFor(turn)}
                  time={turn.createdAt}
                  dateLabel={index() === 0 ? props.headMeta?.()?.dateLabel : undefined}
                  status={index() === 0 ? props.headMeta?.()?.status : undefined}
                />
              </Show>
              <Show when={deleted()}>
                <p class="verevon-room-turn__tombstone" data-revision-state="deleted">
                  {tombstoneText()}
                  <Show when={turn.agentFollowedUp}>
                    {' '}
                    {i18n.tr('En agent hadde allerede svart på den.', 'An agent had already answered it.')}
                  </Show>
                </p>
              </Show>
              <Show when={!deleted() && editing() && props.lifecycle}>
                <SpaceMessageEdit
                  initial={turn.content}
                  maxChars={props.editMaxChars ?? 8000}
                  onSave={async (content) => {
                    await props.lifecycle?.edit(turn, content)
                    setEditing(false)
                  }}
                  onCancel={() => setEditing(false)}
                />
              </Show>
              <Show when={!deleted() && !editing()}>
              <Show
                when={turn.role === 'assistant'}
                fallback={
                  <>
                    {/* WS15-S4: a person's post renders as markdown behind `human_markdown`. ChatMarkdown
                        has no image block and routes every link through safeHref, so human text never
                        yields an <img> or an unsafe anchor. Off, it stays the plain pre-wrap line. */}
                    <Show
                      when={humanMarkdownOn() && splitRoomMessage(turn.content).text.length > 0}
                      fallback={
                        <p class="verevon-room-turn__plain">
                          <Show
                            when={spaceRoomFeature('mention_picker')}
                            fallback={splitRoomMessage(turn.content).text}
                          >
                            <MentionText
                              text={splitRoomMessage(turn.content).text}
                              names={[
                                ...props.roster().map((member) => member.display_name),
                                ...(props.agents?.() ?? []).map((agent) => agent.name ?? ''),
                              ]}
                              viewerName={props.roster().find((member) => member.subject_id === props.viewerSubjectId?.())?.display_name}
                            />
                          </Show>
                        </p>
                      }
                    >
                      <div class="verevon-room-turn__markdown">
                        <ChatMarkdown content={splitRoomMessage(turn.content).text} />
                      </div>
                    </Show>
                    {/* WS08-S6: where the agents named in this post stand. Only the person who posted has
                        entries (their browser drives the wakes), so nobody else sees anything here. */}
                    <Show when={turn.messageId}>
                      {(messageId) => <WakeStatus messageId={messageId()} />}
                    </Show>
                    {/* A turn that carried per-turn files (WS14-S8): the words, then one chip per file
                        by name. The fenced text stays in the stored message and is not shown here. */}
                    <Show when={splitRoomMessage(turn.content).files.length > 0}>
                      <ul class="verevon-space-attachment-chips" aria-label={i18n.tr('Vedlegg', 'Attachments')}>
                        <For each={splitRoomMessage(turn.content).files}>
                          {(file) => <li class="verevon-space-attachment-chip" title={`${file.content.length} ${i18n.tr('tegn', 'characters')}`}>{file.name}</li>}
                        </For>
                      </ul>
                    </Show>
                  </>
                }
              >
                <Show when={turn.content}>
                  <div class="verevon-room-turn__markdown">
                    <ChatMarkdown content={turn.content} />
                  </div>
                </Show>
              </Show>
              <Show when={turn.revisionState === 'edited'}>
                <span
                  class="verevon-room-turn__edited"
                  data-revision-state="edited"
                  title={formatTurnTime(turn.revisedAt)?.full}
                >
                  {i18n.tr('(redigert)', '(edited)')}
                </span>
              </Show>
              </Show>
              <Show when={!deleted() && !editing()}>
                <SpaceReactionBar
                  chips={chips()}
                  canReact={props.lifecycle?.canReact(turn) === true}
                  busy={reactBusy()}
                  onToggle={(emoji, present) => { void toggleReaction(emoji, present) }}
                />
              </Show>
              <Show when={lifecycleError()}>
                {(message) => (
                  <p class="verevon-message-edit__error" role="alert">{message()}</p>
                )}
              </Show>
              {/* The same one-line note the live exchange showed (D1-242): this answer did not use the
                  organisation's knowledge, and why. Read from the stored answer, so reopening the thread keeps it. */}
              <Show when={turn.role === 'assistant' ? turn.retrievalDeclined : undefined}>
                {(reason) => (
                  <p class="verevon-space-room-exchange__stopped" data-retrieval-declined={reason()}>
                    {i18n.tr(spaceRetrievalDeclineCopy(reason()).no, spaceRetrievalDeclineCopy(reason()).en)}
                  </p>
                )}
              </Show>
              {/* What the agent did, as kinds of step only (WS09-S5): a collapsed line, never tool input or output. */}
              <Show when={turn.role === 'assistant' ? turn.work : undefined}>
                {(work) => <SpaceRunWorkSummary work={work()} />}
              </Show>
              <Show when={props.onOpenArtifact && turn.role === 'assistant' && (turn.artifacts?.length ?? 0) > 0}>
                <div class="verevon-space-artifact-chips">
                  <For each={turn.artifacts}>
                    {(artifact) => (
                      <SpaceArtifactChip artifact={artifact} onOpen={(id) => props.onOpenArtifact?.(id)} />
                    )}
                  </For>
                </div>
              </Show>
            </div>
          </div>
          <Show when={index() === 0 ? props.earlierTurns?.() : undefined}>
            {(gap) => (
              <div class="verevon-room-turns-gap">
                <button type="button" class="verevon-room-turns-gap__button" onClick={() => gap().onExpand()}>
                  {gap().count === 1
                    ? i18n.tr('1 tidligere melding', '1 earlier turn')
                    : i18n.tr(`${gap().count} tidligere meldinger`, `${gap().count} earlier turns`)}
                </button>
              </div>
            )}
          </Show>
          </>
        )
      }}
    </For>
  )
}

export function PostTurnHeader(props: {
  readonly author: string
  readonly isAgent?: boolean
  /** When present, the name opens the identity hover card. */
  readonly card?: IdentityCardModel
  /** The turn's ISO time. Shown as a short time beside the name; absent or unreadable shows none. */
  readonly time?: string
  /** A date to show in the time's place when the turn has no time of its own (an older post). */
  readonly dateLabel?: string
  /** The post's run status, when there is something to say beyond "finished". */
  readonly status?: PostStatus
}) {
  const i18n = useI18n()
  const at = () => formatTurnTime(props.time)
  return (
    <div class="verevon-room-turn__header">
      <strong class="verevon-room-turn__author">
        <Show when={props.card} fallback={props.author}>
          {(card) => <SpaceIdentityHoverCard model={card()}>{props.author}</SpaceIdentityHoverCard>}
        </Show>
      </strong>
      <Show when={props.isAgent}>
        <span class="verevon-room-turn__agent-chip">
          <Bot size={12} aria-hidden="true" />
          {i18n.tr('Agent', 'Agent')}
        </span>
      </Show>
      <Show
        when={at()}
        fallback={
          <Show when={props.dateLabel}>
            {(label) => <span class="verevon-room-turn__time">{label()}</span>}
          </Show>
        }
      >
        {(time) => <time class="verevon-room-turn__time" datetime={time().iso} title={time().full}>{time().short}</time>}
      </Show>
      <Show when={props.status}>
        {(status) => (
          <span
            class={[
              {
                'verevon-space-status': true,
                'verevon-room-turn__status': true,
                'verevon-space-status--failed': status().tone === 'failed',
                'verevon-space-status--active': status().tone === 'active',
              },
            ]}
          >
            {status().label}
          </span>
        )}
      </Show>
    </div>
  )
}

/**
 * A person's initials in their own colour: the tint the header's member stack uses, derived from the
 * subject id, so one person looks the same in every place the room draws them. Without a subject id
 * (an unattributed turn) the avatar stays neutral.
 */
export function MemberAvatar(props: { readonly name: string; readonly subjectId?: string }) {
  return (
    <span
      class="verevon-room-turn__avatar"
      aria-hidden="true"
      style={props.subjectId ? { '--avatar-hue': memberTint(props.subjectId) } : undefined}
    >
      {initials(props.name)}
    </span>
  )
}

export function initials(name: string): string {
  // The viewer's own turns read "Name (you)"; the suffix is a label, not part of the name.
  const parts = name.replace(/\s*\([^)]*\)\s*$/, '').trim().split(/\s+/).filter(Boolean)
  const first = parts[0]?.[0] ?? ''
  const last = parts.length > 1 ? parts[parts.length - 1]?.[0] ?? '' : ''
  return (first + last).toLocaleUpperCase() || '?'
}
