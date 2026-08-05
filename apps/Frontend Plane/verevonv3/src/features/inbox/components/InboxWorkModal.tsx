import { X } from 'lucide-solid'
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'
import { customerName, type ZammadTicket } from '@/features/inbox/lib/inbox-model'
import type { SupportTicket } from '@/shared/api/tickets-client'
import { useI18n } from '@/shared/i18n'

export type InboxModalRequest = {
  type: 'work'
  title: string
  description: string
  primaryAction?: string
}

export type InboxTicketLinkRequest = {
  type: 'link-ticket'
  conversationId: string
  title: string
}

export function InboxWorkModal(props: {
  modal: InboxModalRequest | InboxTicketLinkRequest | null
  onClose: () => void
  onLinkExistingTicket: (ticket: SupportTicket) => Promise<boolean>
  selectedTicket: ZammadTicket | null
  ticketCandidates: readonly SupportTicket[]
}) {
  const i18n = useI18n()
  const [selectedTargetID, setSelectedTargetID] = createSignal('')
  const [linking, setLinking] = createSignal(false)
  const workModal = () => props.modal as InboxModalRequest
  const linkRequest = () => props.modal as InboxTicketLinkRequest

  createEffect(() => {
    if (!props.modal) return

    setSelectedTargetID('')

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }

    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => document.removeEventListener('keydown', closeOnEscape))
  })

  return (
    <Show when={props.modal}>
      {(modal) => (
        <dialog open class="verevon-inbox-modal" aria-label={modal().title}>
          <button type="button" aria-label={i18n.tr('Lukk modalbakgrunn', 'Dismiss modal backdrop')} class="verevon-inbox-modal__scrim" onClick={props.onClose} />
          <div class="verevon-inbox-modal__shell verevon-inbox-modal__shell--md">
            <div class="verevon-inbox-modal__header">
              <div>
                <h2>{modal().title}</h2>
                <p>{i18n.tr('Innboks-konteksten forblir aktiv.', 'Inbox context stays active.')}</p>
              </div>
              <button type="button" onClick={props.onClose} class="verevon-inbox-icon-button" aria-label={i18n.tr('Lukk modal', 'Close modal')} title={i18n.tr('Lukk modal', 'Close modal')}>
                <X class="size-4" />
              </button>
            </div>

            <Show
              when={modal().type === 'link-ticket'}
              fallback={<div class="verevon-inbox-modal-panel">
              <div class="verevon-inbox-modal-card">
                <p>{workModal().description}</p>
                <Show when={props.selectedTicket}>
                  {(ticket) => (
                    <div class="verevon-inbox-modal-ticket">
                      <strong>{ticket().title}</strong>
                      <span>#{ticket().number} · {ticket().customer?.email ?? customerName(ticket())}</span>
                    </div>
                  )}
                </Show>
              </div>

              <div class="verevon-inbox-modal-card" role="status">
                <strong>{i18n.tr('Ingen endring lagret', 'No change saved')}</strong>
                <p>{i18n.tr(
                  'Denne arbeidsflyten har ikke en varig, verifiserbar backend-operasjon ennå. Verevon vil ikke late som om saken ble overvåket, koblet eller oppdatert.',
                  'This workflow does not yet have a durable, verifiable backend operation. Verevon will not claim the ticket was watched, linked, or updated.',
                )}</p>
                <button type="button" class="verevon-inbox-button verevon-inbox-button--secondary verevon-inbox-button--sm" onClick={props.onClose}>
                  {i18n.tr('Tilbake til innboks', 'Return to inbox')}
                </button>
              </div>
              </div>}
            >
              <div class="verevon-inbox-modal-panel">
                <div class="verevon-inbox-modal-card">
                  <p>{i18n.tr('Knytt denne samtalen til én eksisterende sak. Samtalen blir en sporbart kildereferanse; sakens opprinnelige samtale og livssyklus endres ikke.', 'Attach this conversation to one existing ticket. The conversation becomes a traceable source reference; the ticket’s primary conversation and lifecycle do not change.')}</p>
                  <label class="verevon-inbox-field-row">
                    <span>{i18n.tr('Eksisterende sak', 'Existing ticket')}</span>
                    <select value={selectedTargetID()} onInput={(event) => setSelectedTargetID(event.currentTarget.value)} aria-label={i18n.tr('Velg eksisterende sak', 'Select existing ticket')}>
                      <option value="">{i18n.tr('Velg en sak …', 'Select a ticket…')}</option>
                      <For each={props.ticketCandidates.filter((ticket) => ticket.conversation_id !== linkRequest().conversationId)}>
                        {(ticket) => <option value={ticket.id}>{ticket.ticket_key} · {ticket.conversation?.title ?? ticket.category ?? i18n.tr('Uten tittel', 'Untitled')}</option>}
                      </For>
                    </select>
                  </label>
                </div>
                <div class="verevon-inbox-modal-card" role="status">
                  <strong>{i18n.tr('Krever bekreftelse', 'Requires confirmation')}</strong>
                  <p>{i18n.tr('Verevon endrer ingenting før du velger én sak og bekrefter. Tilknytningen blir laget gjennom den auditerte Ticketing-handlingen.', 'Verevon changes nothing until you choose one ticket and confirm. The attachment is created through the audited Ticketing action.')}</p>
                  <button
                    type="button"
                    class="verevon-inbox-button verevon-inbox-button--primary verevon-inbox-button--sm"
                    disabled={!selectedTargetID() || linking()}
                    onClick={() => {
                      const target = props.ticketCandidates.find((ticket) => ticket.id === selectedTargetID())
                      if (!target) return
                      setLinking(true)
                      void props.onLinkExistingTicket(target).finally(() => setLinking(false))
                    }}
                  >
                    {linking() ? i18n.tr('Knytter …', 'Attaching…') : i18n.tr('Knytt samtale', 'Attach conversation')}
                  </button>
                </div>
              </div>
            </Show>
          </div>
        </dialog>
      )}
    </Show>
  )
}
