import { For, Show, createSignal } from 'solid-js'
import type { ConnectedAccountView, ConnectionLaneView } from '@/features/onboarding/lib/connection-truth'
import { useI18n } from '@/shared/i18n'
import { cn } from '@/shared/lib/cn'
import { Button } from '@/shared/ui/Button'
// The picker itself is shared with Settings -> Integrations, which offers the
// identical recovery when integration-core refuses a Microsoft sync with
// `409 no_sources_registered`.
import { SharePointLibraryPicker, type LibraryPickerActions } from '@/shared/integrations/SharePointLibraryPicker'

export type { LibraryPickerActions }

type ConnectedAccountsPanelProps = {
  accounts: ConnectedAccountView[]
  loading?: boolean
  unavailable?: boolean
  onRefresh?: () => void
  onReconnect?: (account: ConnectedAccountView) => void
  library?: LibraryPickerActions
}

/**
 * The connect step's source of truth: what integration-core says is connected
 * — provider, account, granted capabilities, each pipeline's sync health and
 * the one next step — rendered above the connector catalogue so it is the
 * first thing the user sees after coming back from a provider consent screen.
 */
export function ConnectedAccountsPanel(props: ConnectedAccountsPanelProps) {
  const i18n = useI18n()
  const [dismissedPickers, setDismissedPickers] = createSignal<ReadonlySet<string>>(new Set())
  const [openPickers, setOpenPickers] = createSignal<ReadonlySet<string>>(new Set())

  const pickerOpen = (account: ConnectedAccountView) =>
    Boolean(props.library)
    && account.provider === 'microsoft'
    && (openPickers().has(account.connectionId)
      || (account.nextStep.kind === 'pick_library' && !dismissedPickers().has(account.connectionId)))

  const togglePicker = (connectionId: string, open: boolean) => {
    setOpenPickers((current) => {
      const next = new Set(current)
      if (open) next.add(connectionId)
      else next.delete(connectionId)
      return next
    })
    if (!open) setDismissedPickers((current) => new Set(current).add(connectionId))
  }

  return (
    <Show when={props.accounts.length > 0 || props.unavailable}>
      <section class="onboarding-connected" aria-label={i18n.tr('Tilkoblede kontoer', 'Connected accounts')} aria-live="polite">
        <header class="onboarding-connected__header">
          <p class="onboarding-connected__eyebrow">
            {i18n.tr('Tilkoblet nå', 'Connected now')}
            <Show when={props.accounts.length > 0}> · {props.accounts.length}</Show>
          </p>
          <Show when={props.onRefresh}>
            <button type="button" class="onboarding-connected__refresh" onClick={() => props.onRefresh?.()} disabled={props.loading}>
              {props.loading ? i18n.tr('Oppdaterer …', 'Refreshing…') : i18n.tr('Oppdater', 'Refresh')}
            </button>
          </Show>
        </header>

        <Show when={props.unavailable}>
          <p class="onboarding-connected-next onboarding-connected-next--attention" role="alert">
            {i18n.tr(
              'Kunne ikke hente tilkoblingsstatus fra integrasjonstjenesten. Prøv å oppdatere.',
              'Could not load connection status from the integration service. Try refreshing.',
            )}
          </p>
        </Show>

        <For each={props.accounts}>
          {(account) => (
            <article class="onboarding-connected-account" data-connection-id={account.connectionId}>
              <div class="onboarding-connected-account__title">
                <strong>{account.providerLabel}</strong>
                <small>{account.account}</small>
                <span class={cn('onboarding-lane-status', account.status === 'active' ? 'onboarding-lane-status--synced' : 'onboarding-lane-status--failed')}>
                  {account.status === 'active' ? i18n.tr('Tilkoblet', 'Connected') : i18n.tr('Trenger ny godkjenning', 'Needs re-authorization')}
                </span>
              </div>

              <Show when={account.grants.length > 0}>
                <ul class="onboarding-connected-account__grants" aria-label={i18n.tr('Tilganger gitt', 'Granted access')}>
                  <For each={account.grants}>{(grant) => <li class="onboarding-grant-chip">{grant}</li>}</For>
                </ul>
              </Show>

              <Show when={account.lanes.length > 0}>
                <ul class="onboarding-connected-lanes" aria-label={i18n.tr('Synkroniseringsstatus', 'Sync status')}>
                  <For each={account.lanes}>
                    {(lane) => (
                      <li class="onboarding-connected-lane">
                        <span>{lane.label}</span>
                        <span class={cn('onboarding-lane-status', `onboarding-lane-status--${lane.status}`)} title={lane.detail ?? undefined}>
                          {laneStatusLabel(lane, i18n.tr)}
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>

              <div class={cn('onboarding-connected-next', `onboarding-connected-next--${account.nextStep.kind}`)}>
                <p>{account.nextStep.message}</p>
                <Show when={account.nextStep.kind === 'reconnect' && props.onReconnect}>
                  <Button size="sm" variant="primary" onClick={() => props.onReconnect?.(account)}>
                    {i18n.tr('Godkjenn på nytt', 'Re-authorize')}
                  </Button>
                </Show>
                <Show when={account.nextStep.kind === 'pick_library' && props.library && !pickerOpen(account)}>
                  <Button size="sm" variant="primary" onClick={() => togglePicker(account.connectionId, true)}>
                    {i18n.tr('Velg bibliotek', 'Pick library')}
                  </Button>
                </Show>
                <Show when={account.provider === 'microsoft' && account.nextStep.kind !== 'pick_library' && account.nextStep.kind !== 'reconnect' && props.library && !pickerOpen(account)}>
                  <button type="button" class="onboarding-connected__refresh" onClick={() => togglePicker(account.connectionId, true)}>
                    {i18n.tr('Legg til bibliotek', 'Add library')}
                  </button>
                </Show>
              </div>

              <Show when={pickerOpen(account) && props.library}>
                {(library) => (
                  <SharePointLibraryPicker
                    connectionId={account.connectionId}
                    library={library()}
                    onDone={() => togglePicker(account.connectionId, false)}
                    onSkip={() => togglePicker(account.connectionId, false)}
                  />
                )}
              </Show>
            </article>
          )}
        </For>
      </section>
    </Show>
  )
}

function laneStatusLabel(lane: ConnectionLaneView, tr: (no: string, en: string) => string): string {
  switch (lane.status) {
    case 'synced':
      return tr('Synkronisert', 'Synced')
    case 'running':
      return tr('Synkroniserer', 'Syncing')
    case 'pending':
      return tr('Venter på første synk', 'Awaiting first sync')
    case 'failed':
      return lane.failureCode === 'no_sources_registered'
        ? tr('Mangler bibliotek', 'No library yet')
        : tr('Feilet', 'Failed')
    case 'cancelled':
      return tr('Avbrutt', 'Cancelled')
    default:
      return tr('Ukjent', 'Unknown')
  }
}
