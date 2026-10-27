import { Bot, Sparkles } from '@/shared/icons'
import { For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import type { SpaceRosterMember } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

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
}

export function SpaceTurnList(props: SpaceTurnListProps) {
  const i18n = useI18n()
  const authorOf = (turn: TranscriptTurn) =>
    resolveAuthorName(turn.authorSubjectId, props.roster(), props.viewerSubjectId?.(), i18n.tr)
  return (
    <For each={props.turns()}>
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
