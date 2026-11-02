import { Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { TimelineEntry } from '../../lib/entry-registry'
import './space-entries.css'

type SystemEntry = Extract<TimelineEntry, { kind: 'system' }>

/**
 * One room-level system row (a member joined, an agent was paused, the room was renamed). The row
 * carries ids and closed words only; names are resolved by the caller from the roster and passed in,
 * so this component never reads identity itself. A code this version does not know reads as a generic
 * room change rather than being hidden or printed raw.
 */
export function SystemNoticeRow(props: {
  readonly entry: SystemEntry
  readonly actorName?: string
  readonly subjectName?: string
}) {
  const i18n = useI18n()
  const actor = () => props.actorName?.trim() || i18n.tr('Noen', 'Someone')
  const subject = () => props.subjectName?.trim() || i18n.tr('et medlem', 'a member')
  const label = () => {
    switch (props.entry.code) {
      case 'member_added':
        return i18n.tr(`${actor()} la til ${subject()}`, `${actor()} added ${subject()}`)
      case 'member_joined':
        return i18n.tr(`${subject()} ble med i rommet`, `${subject()} joined the room`)
      case 'member_removed':
        return i18n.tr(`${actor()} fjernet ${subject()}`, `${actor()} removed ${subject()}`)
      case 'member_left':
        return i18n.tr(`${subject()} forlot rommet`, `${subject()} left the room`)
      case 'role_changed':
        return i18n.tr(`${actor()} endret rollen til ${subject()}`, `${actor()} changed ${subject()}'s role`)
      case 'archived':
        return i18n.tr(`${actor()} arkiverte rommet`, `${actor()} archived the room`)
      case 'unarchived':
        return i18n.tr(`${actor()} gjenopprettet rommet`, `${actor()} restored the room`)
      case 'visibility_changed':
        return i18n.tr(`${actor()} endret hvem som kan finne rommet`, `${actor()} changed who can find the room`)
      case 'settings_changed':
        return i18n.tr(`${actor()} endret rominnstillingene`, `${actor()} changed the room settings`)
      case 'agent_bound':
        return i18n.tr(`${actor()} la til en agent`, `${actor()} added an agent`)
      case 'agent_paused':
        return i18n.tr(`${actor()} satte en agent på pause`, `${actor()} paused an agent`)
      case 'agent_resumed':
        return i18n.tr(`${actor()} gjenopptok en agent`, `${actor()} resumed an agent`)
      case 'agent_revoked':
        return i18n.tr(`${actor()} fjernet en agent`, `${actor()} removed an agent`)
      case 'room_renamed':
        return i18n.tr(`${actor()} endret navnet på rommet`, `${actor()} renamed the room`)
      case 'topic_changed':
        return i18n.tr(`${actor()} endret emnet`, `${actor()} changed the topic`)
      default:
        return i18n.tr('Rommet ble endret', 'The room changed')
    }
  }
  const time = () => {
    const ms = Date.parse(props.entry.at)
    return Number.isFinite(ms) ? new Date(ms) : undefined
  }
  return (
    <div class="verevon-room-system-row" role="note" data-entry-code={props.entry.code}>
      <span class="verevon-room-system-row__text">{label()}</span>
      <Show when={time()}>
        {(at) => (
          <time class="verevon-room-system-row__time" datetime={at().toISOString()}>
            {at().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </time>
        )}
      </Show>
    </div>
  )
}
