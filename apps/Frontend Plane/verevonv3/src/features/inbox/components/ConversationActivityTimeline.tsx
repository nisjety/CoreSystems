import { createResource, For, Show } from 'solid-js'
import { formatTimestamp } from '@/features/inbox/lib/inbox-model'
import { listConversationActivity, type ConversationActivity } from '@/shared/api/inbox-client'
import { useI18n } from '@/shared/i18n'

/**
 * The Activity tab is a narrow operational read model, not a second transcript
 * or raw audit viewer. conversation-core supplies only allow-listed lifecycle
 * fields and this component never derives an event from browser state.
 */
export function ConversationActivityTimeline(props: {
  conversationId: string
  orgId: string
  refreshKey: number
}) {
  const i18n = useI18n()
  const [activity] = createResource(
    () => [props.orgId, props.conversationId, props.refreshKey] as const,
    ([orgId, conversationId]) => listConversationActivity(orgId, conversationId),
  )

  return (
    <Show when={activity.error} fallback={
      <section class="verevon-inbox-activity-timeline" aria-label={i18n.tr('Arbeidsaktivitet', 'Work activity')}>
        <h3>{i18n.tr('Arbeidsaktivitet', 'Work activity')}</h3>
        <Show when={(activity() ?? []).length > 0} fallback={
          <p class="verevon-inbox-activity-timeline__empty">
            {i18n.tr('Ingen arbeidsaktivitet er registrert ennå.', 'No work activity has been recorded yet.')}
          </p>
        }>
          <ol>
            <For each={activity()}>{(event) => <ConversationActivityItem event={event} />}</For>
          </ol>
        </Show>
      </section>
    }>
      <p role="alert" class="verevon-inbox-notice">
        {i18n.tr('Arbeidsaktivitet kunne ikke lastes. Ingen tom tidslinje vises.', 'Work activity could not be loaded. No empty timeline is shown.')}
      </p>
    </Show>
  )
}

function ConversationActivityItem(props: { event: ConversationActivity }) {
  const i18n = useI18n()
  return (
    <li>
      <span aria-hidden="true" class="verevon-inbox-activity-timeline__marker" />
      <div>
        <strong>{activityLabel(props.event, i18n.tr)}</strong>
        <time dateTime={props.event.created_at}>{formatTimestamp(props.event.created_at)}</time>
      </div>
    </li>
  )
}

export function activityLabel(event: ConversationActivity, tr: (no: string, en: string) => string): string {
  switch (event.action) {
    case 'conversation.created': return tr('Samtalen ble opprettet', 'Conversation opened')
    case 'message.received': return tr('Kundemelding mottatt', 'Customer message received')
    case 'message.sent':
    case 'message.submitted': return tr('Svar sendt til leverandør', 'Reply submitted to provider')
    case 'note.created': return tr('Internt notat lagt til', 'Internal note added')
    case 'outbound.delivery_recorded': return tr('Leveringskvittering registrert', 'Delivery receipt recorded')
    case 'status.changed': return tr('Samtalestatus oppdatert', 'Conversation status updated')
    case 'assignment.changed': return tr('Tildeling oppdatert', 'Assignment updated')
    case 'tag.added': return tr('Tagg lagt til', 'Tag added')
    case 'tag.removed': return tr('Tagg fjernet', 'Tag removed')
    case 'ticket.created': return tr('Sak opprettet', 'Ticket created')
    case 'ticket.updated': return tr('Sak oppdatert', 'Ticket updated')
    case 'ticket.linked': return tr('Sak koblet til arbeid', 'Ticket linked to work')
    case 'ticket.macro_run': return tr('Makro kjørt', 'Macro run')
    case 'ticket.checklist_created': return tr('Sjekkliste lagt til', 'Checklist added')
    case 'ticket.checklist_item_updated': return tr('Sjekklistestatus oppdatert', 'Checklist status updated')
    default: return tr('Arbeidsaktivitet registrert', 'Work activity recorded')
  }
}
