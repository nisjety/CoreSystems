import { For, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { AddressedAgent } from '../lib/mention-model'
import { AgentAvatar } from './AgentAvatar'

/**
 * The address bar (WS08-S6): the agents this post is addressed to, in the order they will answer
 * (up to three). A chip can be locked, which keeps the agent addressed for every later post in
 * this conversation until it is unlocked or removed; an unlocked chip lasts for this post and
 * goes when its `@name` leaves the words. Presentation only: the server decides who may answer.
 */

export interface AddressedAgentsBarProps {
  readonly agents: () => readonly AddressedAgent[]
  readonly onToggleLock: (subjectId: string) => void
  readonly onRemove: (subjectId: string) => void
  readonly disabled?: () => boolean
}

export function AddressedAgentsBar(props: AddressedAgentsBarProps) {
  const i18n = useI18n()
  return (
    <Show when={props.agents().length > 0}>
      <ul class="verevon-space-addressed" aria-label={i18n.tr('Adressert til', 'Addressed to')}>
        <For each={props.agents()}>
          {(agent, index) => {
            const locked = () => agent.source === 'locked'
            return (
              <li class="verevon-space-addressed__chip" data-locked={locked() ? 'true' : undefined}>
                <span class="verevon-space-addressed__order" aria-hidden="true">{index() + 1}</span>
                <AgentAvatar kind="agent" size={16} name={agent.label} color={agent.color} />
                <span class="verevon-space-addressed__name">{agent.label}</span>
                <button
                  type="button"
                  class="verevon-space-addressed__lock"
                  aria-pressed={locked() ? 'true' : 'false'}
                  disabled={props.disabled?.()}
                  title={locked()
                    ? i18n.tr('Låst: agenten får alle meldinger i samtalen', 'Locked: the agent gets every message in this conversation')
                    : i18n.tr('Lås til denne agenten', 'Lock to this agent')}
                  aria-label={`${locked() ? i18n.tr('Lås opp', 'Unlock') : i18n.tr('Lås', 'Lock')} ${agent.label}`}
                  onClick={() => props.onToggleLock(agent.subjectId)}
                >
                  {locked() ? '🔒' : '🔓'}
                </button>
                <button
                  type="button"
                  class="verevon-space-addressed__remove"
                  disabled={props.disabled?.()}
                  aria-label={`${i18n.tr('Fjern', 'Remove')} ${agent.label}`}
                  onClick={() => props.onRemove(agent.subjectId)}
                >
                  ✕
                </button>
              </li>
            )
          }}
        </For>
      </ul>
    </Show>
  )
}
