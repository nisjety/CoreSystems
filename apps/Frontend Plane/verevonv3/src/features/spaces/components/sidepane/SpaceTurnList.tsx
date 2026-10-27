import { Bot } from '@/shared/icons'
import { For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { SpaceArtifactChip } from '../SpaceArtifactChip'
import type { SpaceAgent, SpaceRosterMember } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { AgentAvatar } from '../AgentAvatar'
import { agentAvatarColor, agentForName } from '../../lib/agent-avatar'

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
  /** Artifacts this assistant turn persisted (already through the room's gate). */
  readonly artifacts?: readonly ChatArtifact[]
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
    // An artifact-only assistant turn is kept: it is the turn the chip hangs on.
    if (!content && artifacts.length === 0) continue
    const messageId = typeof record.messageId === 'string' && record.messageId.trim()
      ? record.messageId.trim()
      : undefined
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
      ...(messageId ? { messageId } : {}),
      ...(artifacts.length > 0 ? { artifacts } : {}),
    })
    if (normalized.length >= maxTurns) break
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
   * When provided, an assistant turn's artifacts render as chips under it and
   * pressing one calls this with the artifact id. Absent (the side pane), the
   * artifacts are not shown and an artifact-only turn renders no row at all.
   */
  readonly onOpenArtifact?: (artifactId: string) => void
}

export function SpaceTurnList(props: SpaceTurnListProps) {
  const i18n = useI18n()
  const authorOf = (turn: TranscriptTurn) =>
    resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  // A turn with no text is only a row when its artifacts can be shown.
  const visibleTurns = () =>
    props.turns().filter((turn) => turn.content.length > 0 || (props.onOpenArtifact && (turn.artifacts?.length ?? 0) > 0))
  return (
    <For each={visibleTurns()}>
      {(turn) => (
        <div class="verevon-room-turn" data-role={turn.role}>
          <Show
            when={turn.role === 'assistant'}
            fallback={
              <span class="verevon-room-turn__avatar" aria-hidden="true">{initials(authorOf(turn))}</span>
            }
          >
            <AgentAvatar
              kind="agent"
              size={30}
              name={turn.agentName ?? props.agentName}
              color={agentAvatarColor(agentForName(props.agents?.() ?? [], turn.agentName ?? props.agentName))}
            />
          </Show>
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
              <Show when={turn.content}>
                <div class="verevon-room-turn__markdown">
                  <ChatMarkdown content={turn.content} />
                </div>
              </Show>
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
      )}
    </For>
  )
}

export function PostTurnHeader(props: { readonly author: string; readonly isAgent?: boolean }) {
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

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  const first = parts[0]?.[0] ?? ''
  const last = parts.length > 1 ? parts[parts.length - 1]?.[0] ?? '' : ''
  return (first + last).toLocaleUpperCase() || '?'
}
