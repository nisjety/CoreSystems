import { For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { browserErrorMessage } from '@/shared/api/browser-client'
import { useI18n } from '@/shared/i18n'
import { useBrowserEgress, useBrowserEvidenceScope } from './use-browser-evidence'
import { decisionLabel, decisionTone, formatEvidenceTime } from './receipts-view'
import './browser-evidence.css'

/**
 * Devtools "Egress" tab (WS9.D): the decisions the browser made about outgoing requests, as
 * the gateway redacts them (method, origin and path, policy, time). Polled every few seconds
 * while visible. A ZDR session shows the live read only, and the copy says nothing is kept.
 */
export function EgressReceiptsPanel(props: { sessionId: string; zdr: boolean }): JSX.Element {
  const i18n = useI18n()
  const scope = useBrowserEvidenceScope(
    () => props.sessionId,
    () => props.zdr,
  )
  const egress = useBrowserEgress(scope)
  const newestFirst = () => egress.state().rows.slice().reverse()

  return (
    <section class="browser-evidence-panel" aria-label={i18n.tr('Utgående forespørsler', 'Outbound requests')}>
      <header class="browser-evidence-panel__head">
        <span>{i18n.tr('Utgående', 'Outbound')}</span>
        <span class="browser-evidence-panel__counts">
          <strong data-decision="allow">
            {i18n.tr('tillatt', 'allowed')} {egress.state().counts.allow}
          </strong>
          <strong data-decision="block">
            {i18n.tr('blokkert', 'blocked')} {egress.state().counts.block}
          </strong>
        </span>
        <Show when={props.zdr}>
          <span class="browser-evidence-chip browser-evidence-chip--zdr">
            {i18n.tr('Kun live, lagres ikke', 'Live only, not retained')}
          </span>
        </Show>
      </header>
      <Show when={egress.error()}>
        {(failure) => (
          <p class="browser-evidence-panel__hint" role="alert">
            {browserErrorMessage(failure(), i18n.tr, {
              no: 'Utgående forespørsler kunne ikke lastes.',
              en: 'Outbound requests could not be loaded.',
            })}
          </p>
        )}
      </Show>
      <ol class="browser-evidence-list" aria-label={i18n.tr('Utgående forespørsler, nyeste først', 'Outbound requests, newest first')}>
        <For
          each={newestFirst()}
          fallback={<li class="browser-evidence-panel__hint">{i18n.tr('Ingen utgående forespørsler ennå.', 'No outbound requests yet.')}</li>}
        >
          {(row) => (
            <li class="browser-evidence-row" data-decision={decisionTone(row.decision)}>
              <span class="browser-evidence-row__method">{row.method}</span>
              <code class="browser-evidence-row__url" title={row.url}>
                {row.url}
              </code>
              <span class="browser-evidence-row__policy">{row.policy}</span>
              <time class="browser-evidence-row__time">{formatEvidenceTime(row.timestampMs)}</time>
              <strong class={`browser-evidence-row__decision browser-evidence-row__decision--${decisionTone(row.decision)}`}>
                {decisionLabel(row.decision, i18n.tr)}
              </strong>
            </li>
          )}
        </For>
      </ol>
    </section>
  )
}
