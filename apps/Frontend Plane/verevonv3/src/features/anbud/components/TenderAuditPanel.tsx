import { For, type JSX, Show } from 'solid-js'
import type { TenderAudit, TenderCheck } from '@/shared/api/anbud-client'
import { useI18n } from '@/shared/i18n'

type Props = {
  audit: TenderAudit | null
  busy: boolean
  error: string | null
  onClose: () => void
}

/** One tender's audit sheet: each gate and fit check with what it was judged on, where the text came from, and
 *  the buyer's history. The organisation's own uploaded documents are not shown here. */
export function TenderAuditPanel(props: Props) {
  const { tr } = useI18n()

  const outcomeText = (check: TenderCheck) => {
    const labels: Record<TenderCheck['outcome'], string> = {
      pass: tr('Oppfylt', 'Met'),
      conditional: tr('Forbehold', 'Conditional'),
      fail: tr('Ikke oppfylt', 'Not met'),
      unknown: tr('Ukjent', 'Unknown'),
    }
    return labels[check.outcome]
  }

  const checks = (title: string, list: TenderCheck[]): JSX.Element => (
    <section class="verevon-anbud__checks-group">
      <h3>{title}</h3>
      <ul class="verevon-anbud__checks">
        <For each={list}>
          {(check) => (
            <li class="verevon-anbud__check">
              <span class={`verevon-anbud__badge verevon-anbud__badge--${check.outcome}`}>{outcomeText(check)}</span>
              <span>{check.detail}</span>
            </li>
          )}
        </For>
      </ul>
    </section>
  )

  return (
    <section class="verevon-panel verevon-anbud__audit" aria-label={tr('Detaljer om anbudet', 'Tender details')}>
      <header class="verevon-anbud__audit-head">
        <h2>{props.audit?.result.title ?? tr('Detaljer', 'Details')}</h2>
        <button type="button" class="button button--ghost button--sm" onClick={() => props.onClose()}>
          {tr('Lukk', 'Close')}
        </button>
      </header>
      <Show when={props.error}>
        {(text) => (
          <p class="verevon-anbud__state verevon-anbud__state--error" role="alert">
            {text()}
          </p>
        )}
      </Show>
      <Show when={props.busy && !props.audit}>
        <p class="verevon-anbud__state" role="status">
          {tr('Henter detaljer…', 'Loading details…')}
        </p>
      </Show>
      <Show when={props.audit}>
        {(audit) => (
          <>
            {checks(tr('Krav', 'Requirements'), audit().result.gate)}
            {checks(tr('Passform', 'Fit'), audit().result.fit)}
            <dl class="verevon-anbud__facts">
              <dt>{tr('Kilde', 'Source')}</dt>
              <dd>{`${audit().evidence.source}, ${tr('hentet', 'fetched')} ${audit().evidence.fetchedAt.slice(0, 10)}`}</dd>
              <dt>{tr('Fingeravtrykk', 'Fingerprint')}</dt>
              <dd>
                <code>{audit().evidence.sha256.slice(0, 12)}</code>
              </dd>
              <dt>{tr('Skjema', 'Form')}</dt>
              <dd>{`eForms ${audit().parse.sdkVersion} · ${audit().parse.flavour}`}</dd>
              <Show when={audit().parse.unreadTotal > 0}>
                <dt>{tr('Ikke lest', 'Not read')}</dt>
                <dd>{audit().parse.unread.join(', ')}</dd>
              </Show>
              <Show when={audit().incumbency}>
                {(incumbency) => (
                  <>
                    <dt>{tr('Tidligere tildelinger', 'Earlier awards')}</dt>
                    <dd>
                      {tr(
                        `${incumbency().related} relaterte, ${incumbency().withheldWinners} vinnere skjult`,
                        `${incumbency().related} related, ${incumbency().withheldWinners} winners withheld`,
                      )}
                    </dd>
                  </>
                )}
              </Show>
            </dl>
          </>
        )}
      </Show>
    </section>
  )
}
