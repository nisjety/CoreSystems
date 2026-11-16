import { CircleAlert, CircleCheck, Info, Loader } from '@/shared/icons'
import { For, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { AgentCardEntry as AgentCardEntryType, AgentCardTone, TurnEntryCardProps } from '../../lib/entry-registry'
import { registerTurnEntryCard } from '../../lib/entry-registry'
import './space-entries.css'

/**
 * The plain-text card a room agent posted with `space.thread.post_card` (kind `agent_card`, WS18-S7).
 * The title, the tone and the labelled fields are text, and so is the body (the message text, passed in by
 * the timeline). Nothing here is a link, an image or markup: the card has no anchor and no HTML, so a
 * link the agent wrote stays inert text. The tone is shown as an icon and its word, never as colour alone.
 */
export function AgentCardEntry(props: TurnEntryCardProps<AgentCardEntryType>) {
  const i18n = useI18n()
  const toneWord = (tone: AgentCardTone): string => {
    if (tone === 'progress') return i18n.tr('Pågår', 'In progress')
    if (tone === 'done') return i18n.tr('Ferdig', 'Done')
    if (tone === 'blocked') return i18n.tr('Blokkert', 'Blocked')
    return i18n.tr('Info', 'Info')
  }
  return (
    <div
      class="verevon-room-entry verevon-room-entry--card"
      data-entry-kind="agent_card"
      data-tone={props.entry.tone ?? 'none'}
      role="group"
      aria-label={i18n.tr('Agentkort', 'Agent card')}
    >
      <Show when={props.entry.title}>
        <p class="verevon-room-entry--card__title">{props.entry.title}</p>
      </Show>
      <Show when={props.entry.tone}>
        {(tone) => (
          <p class="verevon-room-entry--card__tone">
            <span class="verevon-room-entry--card__icon" aria-hidden="true">
              {tone() === 'progress' ? <Loader size={14} /> : null}
              {tone() === 'done' ? <CircleCheck size={14} /> : null}
              {tone() === 'blocked' ? <CircleAlert size={14} /> : null}
              {tone() === 'info' ? <Info size={14} /> : null}
            </span>
            <span>{toneWord(tone())}</span>
          </p>
        )}
      </Show>
      <Show when={props.body}>
        <p class="verevon-room-entry--card__body">{props.body}</p>
      </Show>
      <Show when={props.entry.fields.length > 0}>
        <dl class="verevon-room-entry--card__fields">
          <For each={props.entry.fields}>
            {(field) => (
              <div class="verevon-room-entry--card__field">
                <dt>{field.label}</dt>
                <dd>{field.value}</dd>
              </div>
            )}
          </For>
        </dl>
      </Show>
      <p class="verevon-room-entry--card__note">
        {i18n.tr('Skrevet av en agent i dette rommet', 'Written by an agent in this room')}
      </p>
    </div>
  )
}

registerTurnEntryCard<AgentCardEntryType>('agent_card', AgentCardEntry, { replacesBody: true })
