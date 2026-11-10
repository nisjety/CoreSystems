import { For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import type { AttentionReason, AttentionSnapshot } from '@/shared/api/attention-client'
import { attentionSection } from '@/shared/attention/attention-state'
import { useI18n } from '@/shared/i18n'
import type { ChatThreadHistoryItem } from '@/features/chat/lib/chat-thread-history'

export type AttentionSectionRows = {
  /** Oldest wait first, so the longest-waiting thread is the first thing the person meets. */
  waiting: ChatThreadHistoryItem[]
  working: ChatThreadHistoryItem[]
  unseen: ChatThreadHistoryItem[]
  /** The waiting threads whose reason is a stopped goal (EP-7 S5); their row reads "Goal stopped". */
  goalStopped: ReadonlySet<string>
}

/**
 * Draws one rail row: the thread, its move action, and the reason it waits when it sits in Waiting
 * (`goal` for a stopped goal). The chat list passes the same renderer, so rows read alike in both.
 */
export type AttentionRowRenderer = (item: ChatThreadHistoryItem, onMove?: () => void, waitingReason?: AttentionReason) => JSX.Element

/**
 * Places the loaded chats into the rail sections by their server attention state. A thread
 * past the loaded page is not in the rail; the list reaches it.
 */
export function attentionSectionRows(attention: AttentionSnapshot | null, sessions: ChatThreadHistoryItem[]): AttentionSectionRows {
  const goalStopped = new Set<string>()
  const rows: AttentionSectionRows = { waiting: [], working: [], unseen: [], goalStopped }
  if (!attention?.enabled) return rows
  const entries = new Map(attention.items.map((item) => [item.threadId, item]))
  const waiting: Array<{ session: ChatThreadHistoryItem; since: number }> = []
  for (const session of sessions) {
    const entry = entries.get(session.threadId)
    if (!entry) continue
    const section = attentionSection(entry.state, entry.unseen)
    if (section === 'waiting') {
      waiting.push({ session, since: Date.parse(entry.since ?? '') || Number.POSITIVE_INFINITY })
      if (entry.reason === 'goal') goalStopped.add(session.threadId)
    } else if (section) {
      rows[section].push(session)
    }
  }
  rows.waiting = waiting.sort((left, right) => left.since - right.since).map((entry) => entry.session)
  return rows
}

/**
 * The Waiting, Working and Unseen rail above the chat list. A section with no thread is not
 * rendered at all. The announcement names only the waiting count, so a screen reader hears a
 * change in what waits on the person and not every row that moved.
 */
export function ChatAttentionSections(props: {
  sections: AttentionSectionRows
  renderRow: AttentionRowRenderer
  /** Opens the move-to-project dialog for a row; the same action the list's rows offer. */
  onMove?: (item: ChatThreadHistoryItem) => void
}) {
  const i18n = useI18n()
  const total = () => props.sections.waiting.length + props.sections.working.length + props.sections.unseen.length
  return (
    <Show when={total() > 0}>
      <section class="chat-attention" aria-label={i18n.tr('Samtaler som trenger deg', 'Chats that need you')}>
        <p class="sr-only" aria-live="polite">
          {props.sections.waiting.length > 0
            ? i18n.tr(`Venter på deg: ${props.sections.waiting.length}`, `Waiting for you: ${props.sections.waiting.length}`)
            : ''}
        </p>
        <Show when={props.sections.waiting.length > 0}>
          <AttentionGroup
            label={i18n.tr('Venter på deg', 'Waiting for you')}
            items={props.sections.waiting}
            renderRow={props.renderRow}
            onMove={props.onMove}
            goalStopped={props.sections.goalStopped}
          />
        </Show>
        <Show when={props.sections.working.length > 0}>
          <AttentionGroup label={i18n.tr('Pågår', 'Working')} items={props.sections.working} renderRow={props.renderRow} onMove={props.onMove} />
        </Show>
        <Show when={props.sections.unseen.length > 0}>
          <AttentionGroup label={i18n.tr('Ulest', 'Unseen')} items={props.sections.unseen} renderRow={props.renderRow} onMove={props.onMove} />
        </Show>
      </section>
    </Show>
  )
}

function AttentionGroup(props: {
  label: string
  items: ChatThreadHistoryItem[]
  renderRow: AttentionRowRenderer
  onMove?: (item: ChatThreadHistoryItem) => void
  /** The threads in this group that wait on a stopped goal (EP-7 S5); none outside Waiting. */
  goalStopped?: ReadonlySet<string>
}) {
  return (
    <div class="chat-attention__group">
      <span class="verevon-sidebar-group-title">{props.label}</span>
      <For each={props.items}>
        {(item) => props.renderRow(item, moveAction(props.onMove, item), props.goalStopped?.has(item.threadId) ? 'goal' : undefined)}
      </For>
    </div>
  )
}

function moveAction(onMove: ((item: ChatThreadHistoryItem) => void) | undefined, item: ChatThreadHistoryItem): () => void {
  return () => onMove?.(item)
}
