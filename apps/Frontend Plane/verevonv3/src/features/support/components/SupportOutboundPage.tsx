import { useLocation } from '@solidjs/router'
import { AlertTriangle, CheckCircle2, Mail, MessageSquareText, Send, ShieldCheck, UsersRound } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { listOrganizationOutboundIntents, type OrganizationOutboundIntentFilter, type OutboundIntent } from '@/shared/api/inbox-client'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'
import { OutboundVerevonRail } from './OutboundVerevonRail'

type OutboundCenterTab = 'message' | 'audience' | 'delivery'
type OutboundRailTab = 'details' | 'verevon' | 'actions' | 'audit'
type OutboundFilter = Pick<OrganizationOutboundIntentFilter, 'status' | 'provider' | 'deliveryStatus'>

const statusLabels: Record<OutboundIntent['status'], [string, string]> = {
  sending: ['Sender', 'Sending'],
  retryable: ['Kan prøves igjen', 'Retryable'],
  submitted: ['Godtatt av leverandør', 'Provider accepted'],
  failed: ['Mislyktes', 'Failed'],
  unknown: ['Ukjent utfall', 'Unknown outcome'],
}

const deliveryLabels: Record<NonNullable<OutboundIntent['delivery_status']>, [string, string]> = {
  unconfirmed: ['Levering ikke bekreftet', 'Delivery unconfirmed'],
  delivered: ['Levert', 'Delivered'],
  read: ['Lest', 'Read'],
  failed: ['Levering mislyktes', 'Delivery failed'],
}

function filterFromSearch(search: string): OutboundFilter {
  const query = new URLSearchParams(search)
  const status = query.get('outbound_status')
  const deliveryStatus = query.get('outbound_delivery_status')
  return {
    status: status === 'sending' || status === 'retryable' || status === 'submitted' || status === 'failed' || status === 'unknown' ? status : undefined,
    provider: query.get('outbound_provider')?.trim() || undefined,
    deliveryStatus: deliveryStatus === 'unconfirmed' || deliveryStatus === 'delivered' || deliveryStatus === 'read' || deliveryStatus === 'failed' ? deliveryStatus : undefined,
  }
}

/** A read-only, organization-scoped reconciliation ledger. Standalone
 * campaigns and recipient management remain in their owner surfaces. */
export default function SupportOutboundPage() {
  const i18n = useI18n()
  const location = useLocation()
  const session = getSession()
  const [centerTab, setCenterTab] = createSignal<OutboundCenterTab>('message')
  const [railTab, setRailTab] = createSignal<OutboundRailTab>('details')
  const [selectedID, setSelectedID] = createSignal<string | null>(null)
  const filter = createMemo(() => filterFromSearch(location.search))
  const orgID = createMemo(() => session.activeOrg?.id ?? '')
  const [intents, { refetch }] = createResource(
    () => ({ orgID: orgID(), filter: filter() }),
    ({ orgID: id, filter: current }) => id ? listOrganizationOutboundIntents(id, { ...current, limit: 100 }) : Promise.resolve([] as OutboundIntent[]),
  )
  // Match the Inbox/Ticketing tri-pane behavior: once the authoritative list
  // has loaded, expose the newest receipt without asking an operator to make a
  // redundant first selection. Do not change selection while a refresh or an
  // unavailable response is in flight; that would turn a transient read fault
  // into a misleading empty detail pane.
  createEffect(
    () => ({ loading: intents.loading, error: intents.error, available: intents(), selectedId: selectedID() }),
    (state) => {
      if (state.loading || state.error) return
      if (!state.available?.length) {
        setSelectedID(null)
        return
      }
      if (!state.available.some((intent) => intent.id === state.selectedId)) {
        setSelectedID(state.available[0]!.id)
      }
    },
  )
  const selected = createMemo(() => intents()?.find((intent) => intent.id === selectedID()) ?? null)
  const queueTitle = createMemo(() => {
    const current = filter()
    if (current.status) return i18n.tr(...statusLabels[current.status])
    if (current.deliveryStatus) return i18n.tr(...deliveryLabels[current.deliveryStatus])
    return i18n.tr('Alle leveringskvitteringer', 'All delivery receipts')
  })

  return (
    <div class="verevon-support-outbound" aria-label={i18n.tr('Utgående arbeidsområde', 'Outbound workspace')}>
      <aside class="verevon-support-outbound__queue" aria-label={i18n.tr('Utgående køer', 'Outbound queues')}>
        <header>
          <span>{i18n.tr('Leveringslogg', 'Delivery ledger')}</span>
          <button type="button" disabled aria-label={i18n.tr('Opprett utgående melding', 'Create outbound message')} title={i18n.tr('Kampanje- og meldingsopprettelse eies av sine autoritative arbeidsflater.', 'Campaign and message creation remain owned by their authoritative workspaces.')}><Send class="size-4" /></button>
        </header>
        <nav>
          <Show when={intents.loading}><p class="verevon-support-outbound__availability">{i18n.tr('Laster kvitteringer …', 'Loading receipts…')}</p></Show>
          <Show when={intents.error}><p class="verevon-support-outbound__availability" role="alert">{i18n.tr('Utgående kvitteringer er midlertidig utilgjengelige.', 'Outbound receipts are temporarily unavailable.')} <button type="button" onClick={() => void refetch()}>{i18n.tr('Prøv igjen', 'Retry')}</button></p></Show>
          <For each={intents() ?? []}>{(intent) => (
            <button type="button" class={{ 'is-active': selectedID() === intent.id }} onClick={() => setSelectedID(intent.id)}>
              <span>{i18n.tr(...statusLabels[intent.status])}</span>
              <small>{intent.provider || i18n.tr('Ukjent kanal', 'Unknown channel')}</small>
            </button>
          )}</For>
          <Show when={!intents.loading && !intents.error && (intents()?.length ?? 0) === 0}><p class="verevon-support-outbound__availability">{i18n.tr('Ingen kvitteringer matcher dette filteret.', 'No receipts match this filter.')}</p></Show>
        </nav>
      </aside>

      <main class="verevon-support-outbound__center">
        <div class="verevon-support-pane-tabs" role="tablist" aria-label={i18n.tr('Utgående innhold', 'Outbound content')}>
          <OutboundCenterTabButton active={centerTab() === 'message'} id="message" label={i18n.tr('Kvittering', 'Receipt')} onSelect={setCenterTab} />
          <OutboundCenterTabButton active={centerTab() === 'audience'} id="audience" label={i18n.tr('Målgruppe', 'Audience')} onSelect={setCenterTab} />
          <OutboundCenterTabButton active={centerTab() === 'delivery'} id="delivery" label={i18n.tr('Levering', 'Delivery')} onSelect={setCenterTab} />
        </div>
        <Show when={centerTab() === 'message'} fallback={<Show when={centerTab() === 'audience'} fallback={<DeliveryPanel intent={selected()} />}><OutboundEmptyPanel id="audience" icon={UsersRound} title={i18n.tr('Målgruppe holdes i kilden', 'Audience remains in its source')} body={i18n.tr('Denne Support-visningen viser ikke en lokal mottakerliste. Åpne det autoritative kampanje- eller meldingsarbeidet for mottakerstyring.', 'This Support view does not expose a local recipient list. Open the authoritative campaign or messaging workspace for recipient management.')} /></Show>}>
          <ReceiptPanel intent={selected()} queueTitle={queueTitle()} />
        </Show>
      </main>

      <aside class="verevon-support-outbound__rail">
        <div class="verevon-support-pane-tabs" role="tablist" aria-label={i18n.tr('Utgående kontekst', 'Outbound context')}>
          <OutboundRailTabButton active={railTab() === 'details'} id="details" label={i18n.tr('Detaljer', 'Details')} onSelect={setRailTab} />
          <OutboundRailTabButton active={railTab() === 'verevon'} id="verevon" label="Verevon" onSelect={setRailTab} />
          <OutboundRailTabButton active={railTab() === 'actions'} id="actions" label={i18n.tr('Handlinger', 'Actions')} onSelect={setRailTab} />
          <OutboundRailTabButton active={railTab() === 'audit'} id="audit" label={i18n.tr('Revisjon', 'Audit')} onSelect={setRailTab} />
        </div>
        <div id={`outbound-rail-panel-${railTab()}`} class="verevon-support-outbound__rail-empty" role="tabpanel" aria-labelledby={`outbound-rail-tab-${railTab()}`}>
          <Show when={railTab() === 'details'}><OutboundDetailsRail intent={selected()} /></Show>
          <Show when={railTab() === 'verevon'}><OutboundVerevonRail intent={selected()} orgId={orgID()} userId={session.user?.id ?? ''} /></Show>
          <Show when={railTab() === 'actions'}><OutboundActionsRail intent={selected()} /></Show>
          <Show when={railTab() === 'audit'}><OutboundAuditRail intent={selected()} /></Show>
        </div>
      </aside>
    </div>
  )
}

function ReceiptPanel(props: { intent: OutboundIntent | null; queueTitle: string }) {
  const i18n = useI18n()
  return <Show when={props.intent} fallback={<OutboundEmptyPanel id="message" icon={Mail} title={props.queueTitle} body={i18n.tr('Velg en utgående kvittering fra listen. Denne arbeidsflaten viser bare kanoniske resultatdata, ikke kampanjeinnhold.', 'Select an outbound receipt from the list. This workspace shows canonical outcome data only, not campaign content.')} />}>
    {(intent) => <section id="outbound-center-panel-message" class="verevon-support-outbound__empty" role="tabpanel" aria-labelledby="outbound-center-tab-message"><Mail class="size-6" /><h2>{i18n.tr(...statusLabels[intent().status])}</h2><p>{i18n.tr('Kanal', 'Channel')}: {intent().provider || i18n.tr('Ukjent', 'Unknown')}</p><p>{i18n.tr('Samtale', 'Conversation')}: {intent().conversation_id}</p><p>{i18n.tr('Oppdatert', 'Updated')}: {new Date(intent().updated_at).toLocaleString()}</p><Show when={intent().status === 'unknown'}><p>{i18n.tr('Utfallet krever avstemming i kildesamtalen. Ikke prøv automatisk på nytt.', 'The outcome requires reconciliation in the source conversation. Do not retry automatically.')}</p></Show><div><a href={`/support?view=all&conversation_id=${encodeURIComponent(intent().conversation_id)}`} link>{i18n.tr('Åpne kildesamtale', 'Open source conversation')}</a></div></section>}
  </Show>
}

function DeliveryPanel(props: { intent: OutboundIntent | null }) {
  const i18n = useI18n()
  return <Show when={props.intent} fallback={<OutboundEmptyPanel id="delivery" icon={CheckCircle2} title={i18n.tr('Velg en kvittering', 'Select a receipt')} body={i18n.tr('Leveringsdetaljer er knyttet til én autoritativ utgående kvittering.', 'Delivery details belong to one authoritative outbound receipt.')} />}>
    {(intent) => <section id="outbound-center-panel-delivery" class="verevon-support-outbound__empty" role="tabpanel" aria-labelledby="outbound-center-tab-delivery"><CheckCircle2 class="size-6" /><h2>{i18n.tr(...deliveryLabels[intent().delivery_status ?? 'unconfirmed'])}</h2><p>{intent().status === 'submitted' ? i18n.tr('Leverandøren godtok innsendingen. Dette er ikke bevis på levering eller lesing.', 'The provider accepted submission. This is not proof of delivery or read.') : i18n.tr('Utgående arbeidsstatus og leveringsstatus holdes atskilt.', 'Outbound work status and delivery status are kept separate.')}</p><Show when={intent().delivery_occurred_at}><p>{i18n.tr('Registrert', 'Recorded')}: {new Date(intent().delivery_occurred_at!).toLocaleString()}</p></Show><Show when={intent().delivery_error_code || intent().error_code}><p><AlertTriangle class="size-4" /> {intent().delivery_error_code || intent().error_code}</p></Show></section>}
  </Show>
}

function OutboundDetailsRail(props: { intent: OutboundIntent | null }) {
  const i18n = useI18n()
  return <Show when={props.intent} fallback={<OutboundRailEmpty icon={MessageSquareText} body={i18n.tr('Velg en kvittering for leveringsdetaljer.', 'Select a receipt for delivery details.')} />}>
    {(intent) => (
      <section class="verevon-support-outbound__rail-section">
        <span><MessageSquareText class="size-5" />{i18n.tr('Kanonisk kvittering', 'Canonical receipt')}</span>
        <h2>{i18n.tr('Kvitteringsdetaljer', 'Receipt details')}</h2>
        <dl class="verevon-support-outbound__receipt-facts">
          <ReceiptFact label={i18n.tr('Kanal', 'Channel')} value={intent().provider || i18n.tr('Ukjent', 'Unknown')} />
          <ReceiptFact label={i18n.tr('Arbeidsstatus', 'Work status')} value={i18n.tr(...statusLabels[intent().status])} />
          <ReceiptFact label={i18n.tr('Leveringsstatus', 'Delivery status')} value={i18n.tr(...deliveryLabels[intent().delivery_status ?? 'unconfirmed'])} />
          <ReceiptFact label={i18n.tr('Samtale', 'Conversation')} value={intent().conversation_id} />
          <Show when={intent().provider_message_id}><ReceiptFact label={i18n.tr('Leverandørkvittering', 'Provider receipt ID')} value={intent().provider_message_id!} /></Show>
          <Show when={intent().delivery_occurred_at}><ReceiptFact label={i18n.tr('Levering registrert', 'Delivery recorded')} value={formatOutboundTimestamp(intent().delivery_occurred_at!)} /></Show>
          <Show when={intent().delivery_error_code || intent().error_code}><ReceiptFact label={i18n.tr('Feilkode', 'Error code')} value={intent().delivery_error_code || intent().error_code || ''} /></Show>
        </dl>
        <p class="verevon-support-outbound__rail-note">{detailBoundary(intent(), i18n.tr)}</p>
      </section>
    )}
  </Show>
}

function OutboundActionsRail(props: { intent: OutboundIntent | null }) {
  const i18n = useI18n()
  return <Show when={props.intent} fallback={<OutboundRailEmpty icon={Send} body={i18n.tr('Velg en kvittering før du vurderer neste steg.', 'Select a receipt before reviewing the next step.')} />}>
    {(intent) => (
      <section class="verevon-support-outbound__rail-section">
        <span><Send class="size-5" />{i18n.tr('Trygt neste steg', 'Safe next step')}</span>
        <h2>{actionTitle(intent(), i18n.tr)}</h2>
        <p>{actionBoundary(intent(), i18n.tr)}</p>
        <a class="verevon-support-outbound__rail-link" href={sourceConversationHref(intent())} link>{i18n.tr('Åpne kildesamtale', 'Open source conversation')}</a>
        <p class="verevon-support-outbound__rail-note">{i18n.tr('Ingen ny utsending eller automatisk nytt forsøk kan startes fra leveringsloggen.', 'No new send or automatic retry can be started from the delivery ledger.')}</p>
      </section>
    )}
  </Show>
}

function OutboundAuditRail(props: { intent: OutboundIntent | null }) {
  const i18n = useI18n()
  return <Show when={props.intent} fallback={<OutboundRailEmpty icon={ShieldCheck} body={i18n.tr('Velg en kvittering for revisjonskontekst.', 'Select a receipt for audit context.')} />}>
    {(intent) => (
      <section class="verevon-support-outbound__rail-section">
        <span><ShieldCheck class="size-5" />{i18n.tr('Innholdsfri revisjonskontekst', 'Content-free audit context')}</span>
        <p>{i18n.tr('Viser bare den kanoniske kvitteringens identifikatorer, tilstander og tidspunkt. Kundeinnhold og mottakere blir i sine beskyttede kilder.', 'Shows only canonical receipt identifiers, states, and timestamps. Customer content and recipients remain in their protected sources.')}</p>
        <dl class="verevon-support-outbound__receipt-facts">
          <ReceiptFact label={i18n.tr('Kvitterings-ID', 'Receipt ID')} value={intent().id} />
          <ReceiptFact label={i18n.tr('Opprettet', 'Recorded')} value={formatOutboundTimestamp(intent().created_at)} />
          <ReceiptFact label={i18n.tr('Sist oppdatert', 'Last updated')} value={formatOutboundTimestamp(intent().updated_at)} />
          <Show when={intent().provider_message_id}><ReceiptFact label={i18n.tr('Leverandørkvittering', 'Provider receipt ID')} value={intent().provider_message_id!} /></Show>
        </dl>
      </section>
    )}
  </Show>
}

function OutboundRailEmpty(props: { icon: typeof Mail; body: string }) {
  return <><props.icon class="size-5" /><p>{props.body}</p></>
}

function ReceiptFact(props: { label: string; value: string }) {
  return <div><dt>{props.label}</dt><dd>{props.value}</dd></div>
}

function sourceConversationHref(intent: OutboundIntent) {
  return `/support?view=all&conversation_id=${encodeURIComponent(intent.conversation_id)}`
}

function formatOutboundTimestamp(value: string) {
  return new Date(value).toLocaleString()
}

function detailBoundary(intent: OutboundIntent, tr: (norwegian: string, english: string) => string) {
  if (intent.status === 'submitted') return tr('Leverandøren godtok innsendingen. Dette er ikke bevis på levering eller lesing.', 'The provider accepted submission. This is not proof of delivery or read.')
  if (intent.status === 'unknown') return tr('Utfallet må avstemmes i kildesamtalen. Ikke prøv automatisk på nytt.', 'Reconcile the outcome in the source conversation. Do not retry automatically.')
  if (intent.status === 'retryable') return tr('Kontroller kildesamtalen og leverandørens feilkode før en operatør eventuelt gjør en ny innsending i den autoritative arbeidsflaten.', 'Check the source conversation and provider error before an operator considers a new submission in its authoritative workspace.')
  if (intent.status === 'failed') return tr('Leverandøren rapporterte feil. Dette er ikke et leveringsutfall.', 'The provider reported an error. This is not a delivery outcome.')
  return tr('Avventer en autoritativ kvittering fra leverandøren.', 'Awaiting an authoritative receipt from the provider.')
}

function actionTitle(intent: OutboundIntent, tr: (norwegian: string, english: string) => string) {
  if (intent.status === 'unknown' || intent.status === 'retryable') return tr('Avstem i kildesamtalen', 'Reconcile in the source conversation')
  if (intent.status === 'failed') return tr('Undersøk leverandørfeilen', 'Investigate the provider error')
  if (intent.status === 'sending') return tr('Avvent leverandørkvittering', 'Wait for a provider receipt')
  return tr('Følg opp kun fra kilden', 'Follow up only from the source')
}

function actionBoundary(intent: OutboundIntent, tr: (norwegian: string, english: string) => string) {
  if (intent.status === 'unknown') return tr('Kontroller den opprinnelige samtalen før du foreslår eller utfører nytt arbeid.', 'Check the original conversation before proposing or performing further work.')
  if (intent.status === 'retryable') return tr('Kontroller årsaken og den opprinnelige samtalen før en operatør eventuelt starter en ny innsending i kilden.', 'Check the cause and original conversation before an operator considers a new submission in the source.')
  if (intent.status === 'failed') return tr('Undersøk den registrerte feilkoden i kilden. Ikke utled eller krev en leveringstilstand.', 'Investigate the recorded error code in the source. Do not infer or require a delivery state.')
  if (intent.status === 'sending') return tr('Vent på en autoritativ leverandørkvittering før du gjør oppfølgingsarbeid.', 'Wait for an authoritative provider receipt before performing follow-up work.')
  return tr('Koordiner eventuelt videre arbeid i kildesamtalen. Godtatt innsending er ikke bevis på levering eller lesing.', 'Coordinate any further work in the source conversation. Accepted submission is not proof of delivery or read.')
}

function OutboundCenterTabButton(props: { active: boolean; id: OutboundCenterTab; label: string; onSelect: (tab: OutboundCenterTab) => void }) { return <button type="button" role="tab" id={`outbound-center-tab-${props.id}`} aria-controls={`outbound-center-panel-${props.id}`} aria-selected={props.active ? 'true' : 'false'} tabindex={props.active ? 0 : -1} onKeyDown={handleTabKeyDown} onClick={() => props.onSelect(props.id)}>{props.label}</button> }
function OutboundRailTabButton(props: { active: boolean; id: OutboundRailTab; label: string; onSelect: (tab: OutboundRailTab) => void }) { return <button type="button" role="tab" id={`outbound-rail-tab-${props.id}`} aria-controls={`outbound-rail-panel-${props.id}`} aria-selected={props.active ? 'true' : 'false'} tabindex={props.active ? 0 : -1} onKeyDown={handleTabKeyDown} onClick={() => props.onSelect(props.id)}>{props.label}</button> }
function OutboundEmptyPanel(props: { id: OutboundCenterTab; icon: typeof Mail; title: string; body: string }) { return <section id={`outbound-center-panel-${props.id}`} class="verevon-support-outbound__empty" role="tabpanel" aria-labelledby={`outbound-center-tab-${props.id}`}><props.icon class="size-6" /><h2>{props.title}</h2><p>{props.body}</p></section> }
