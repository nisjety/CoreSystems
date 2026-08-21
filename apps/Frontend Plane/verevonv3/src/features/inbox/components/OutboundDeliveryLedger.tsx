import { createEffect, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { formatTimestamp } from '@/features/inbox/lib/inbox-model'
import { listOutboundIntents, type OutboundIntent } from '@/shared/api/inbox-client'
import { useI18n } from '@/shared/i18n'

export function OutboundDeliveryLedger(props: {
  conversationId: string
  orgId: string
  refreshKey: number
}) {
  const i18n = useI18n()
  const [intents, { refetch }] = createResource(
    () => [props.orgId, props.conversationId, props.refreshKey] as const,
    ([orgId, conversationId]) => listOutboundIntents(orgId, conversationId),
  )

  // Provider delivery/read callbacks arrive asynchronously. Re-read the
  // content-free authority on a modest interval while this conversation is
  // open; a callback is never inferred from local UI state.
  createEffect(
    () => props.refreshKey,
    () => {
      const interval = window.setInterval(() => void refetch(), 30_000)
      return () => window.clearInterval(interval)
    },
  )

  return (
    <Show when={intents.error} fallback={
      <Show when={(intents() ?? []).length > 0}>
        <section class="verevon-inbox-delivery-ledger" aria-label={i18n.tr('Leveringsutfall', 'Delivery outcomes')}>
          <h3>{i18n.tr('Leveringsutfall', 'Delivery outcomes')}</h3>
          <For each={intents()}>{(intent) => <DeliveryOutcome intent={intent} />}</For>
        </section>
      </Show>
    }>
      <p role="alert" class="verevon-inbox-notice">{i18n.tr('Leveringsutfall kunne ikke lastes. Ingen leveringspåstand vises.', 'Delivery outcomes could not be loaded. No delivery claim is shown.')}</p>
    </Show>
  )
}

function DeliveryOutcome(props: { intent: OutboundIntent }) {
  const i18n = useI18n()
  const provider = () => props.intent.provider || i18n.tr('leverandøren', 'the provider')
  const message = () => {
    if (props.intent.status === 'submitted') {
      switch (props.intent.delivery_status ?? 'unconfirmed') {
        case 'delivered':
          return i18n.tr(
            `Leverandøren rapporterer levering via ${provider()}. Lest-status er ikke bekreftet.`,
            `The provider reports delivery via ${provider()}. Read status is not confirmed.`,
          )
        case 'read':
          return i18n.tr(
            `Leverandøren rapporterer at meldingen er lest via ${provider()}.`,
            `The provider reports the message was read via ${provider()}.`,
          )
        case 'failed':
          return i18n.tr(
            `Leverandøren rapporterer at meldingen ikke ble levert via ${provider()}. Leverandøraksept var tidligere registrert.`,
            `The provider reports the message was not delivered via ${provider()}. Provider acceptance was recorded earlier.`,
          )
        default:
          return i18n.tr(
            `Sendt til ${provider()}; leverandøren godtok forespørselen. Levering er ikke bekreftet.`,
            `Submitted to ${provider()}; the provider accepted the request. Delivery is not confirmed.`,
          )
      }
    }
    switch (props.intent.status) {
      case 'unknown':
        return i18n.tr(
          'Utfallet er ukjent. Avstemming kreves; ikke prøv automatisk på nytt.',
          'The outcome is unknown. Reconciliation is required; do not retry automatically.',
        )
      case 'failed':
        return i18n.tr(
          'Leverandøren avviste eller feilet forespørselen. Ingen akseptkvittering er registrert.',
          'The provider rejected or failed the request. No acceptance receipt is recorded.',
        )
      case 'retryable':
        return i18n.tr(
          'Det forrige forsøket kan prøves på nytt etter menneskelig gjennomgang.',
          'The previous attempt may be retried after human review.',
        )
      default:
        return i18n.tr(
          'Forespørselen behandles fortsatt. Vent på et utfall før du sender et duplikat.',
          'The submission is still being processed. Wait for an outcome before sending a duplicate.',
        )
    }
  }

  return (
    <div class={`verevon-inbox-delivery-ledger__item verevon-inbox-delivery-ledger__item--${props.intent.status}`}>
      <strong>{message()}</strong>
      <span>{formatTimestamp(props.intent.updated_at)}</span>
    </div>
  )
}
