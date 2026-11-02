import { For, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import {
  RETRYABLE_WAKE_CODES,
  spaceWakeBoard,
  spaceWakeSequencer,
  type WakeBoard,
  type WakeEntry,
  type WakeSequencer,
} from '../lib/wake-sequencer'

/**
 * Under a post: where each addressed agent stands (WS08-S6). Waiting its turn, answering, done,
 * not allowed to answer (with the reason in words), failed (Retry), or left unstarted when this
 * session stopped ("Wake now"). It reads this browser's wake board, so only the person who
 * posted ever sees it: the wakes are theirs to drive, and nothing here is offered to anyone else.
 * An agent the server recorded but this browser never woke stays `expected` on the server.
 */

export interface WakeStatusProps {
  readonly messageId: string | undefined
  readonly board?: WakeBoard
  readonly sequencer?: WakeSequencer
}

export function WakeStatus(props: WakeStatusProps) {
  const i18n = useI18n()
  const board = () => props.board ?? spaceWakeBoard
  const sequencer = () => props.sequencer ?? spaceWakeSequencer
  const entries = () => (props.messageId ? board().forMessage(props.messageId) : [])
  const reason = (entry: WakeEntry) =>
    translateApiError({ code: entry.code ?? 'wake_not_authorized' }, i18n.tr, {
      no: 'Agenten kunne ikke vekkes.',
      en: 'The agent could not be woken.',
    })
  const line = (entry: WakeEntry): string => {
    const name = entry.agentName
    switch (entry.state) {
      case 'queued':
        return i18n.tr(`${name} er neste`, `${name} is next`)
      case 'waiting':
        return i18n.tr(`${name} venter: en agent jobber allerede i samtalen`, `${name} is waiting: an agent is already working in this conversation`)
      case 'answering':
        return i18n.tr(`${name} svarer …`, `${name} is answering…`)
      case 'done':
        return i18n.tr(`${name} har svart`, `${name} answered`)
      case 'refused':
        return `${name}: ${reason(entry)}`
      case 'failed':
        return `${name}: ${reason(entry)}`
      case 'interrupted':
        return i18n.tr(`${name} er ikke startet`, `${name} has not started`)
    }
  }
  const anyInterrupted = () => entries().some((entry) => entry.state === 'interrupted')
  return (
    <Show when={entries().length > 0}>
      <ul class="verevon-space-wake" aria-label={i18n.tr('Agenter som er nevnt', 'Agents mentioned')} role="status" aria-live="polite">
        <For each={entries()}>
          {(entry) => (
            <li class="verevon-space-wake__item" data-state={entry.state}>
              <span>{line(entry)}</span>
              <Show when={entry.state === 'failed' && (RETRYABLE_WAKE_CODES.has(entry.code ?? '') || entry.code === 'thread_run_active')}>
                <button
                  type="button"
                  class="verevon-space-wake__action"
                  onClick={() => props.messageId && sequencer().retry(props.messageId, entry.agentSubjectId)}
                >
                  {i18n.tr('Prøv igjen', 'Retry')}
                </button>
              </Show>
            </li>
          )}
        </For>
        <Show when={anyInterrupted()}>
          <li class="verevon-space-wake__item">
            <button
              type="button"
              class="verevon-space-wake__action"
              onClick={() => props.messageId && sequencer().wakeNow(props.messageId)}
            >
              {i18n.tr('Vekk nå', 'Wake now')}
            </button>
          </li>
        </Show>
      </ul>
    </Show>
  )
}
