import { For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getBillingAllowances } from '@/shared/api/billing-allowances'
import { billingAllowancesActionSchema } from '@/shared/actions/billing-allowances'
import { useI18n } from '@/shared/i18n'
import './usage-analytics.css'

export function BillingAllowancesSection(props: { identity: string; refreshKey: number }) {
  const i18n = useI18n()
  const [result] = createResource(() => ({ identity: props.identity, refresh: props.refreshKey }), async key => {
    billingAllowancesActionSchema.parse({ type: 'billing.allowances.read', scope: 'organization' })
    try { return { identity: key.identity, data: await getBillingAllowances(), available: true } }
    catch { return { identity: key.identity, data: [], available: false } }
  })
  const current = () => result()?.identity === props.identity ? result() : undefined
  const number = (value: number) => value.toLocaleString()
  const date = (value: string) => new Date(value).toLocaleDateString()
  return <section class="usage-analytics" aria-label={i18n.tr('Brukskvoter', 'Usage allowances')}>
    <header class="usage-analytics__header"><div><span>{i18n.tr('Organisasjonens abonnement', 'Organization plan')}</span><h2>{i18n.tr('Inkludert bruk og overføring', 'Allowances & carry-over')}</h2></div><a href="/settings/billing" link>{i18n.tr('Administrer abonnement', 'Manage plan')}</a></header>
    <Show when={current()} fallback={<p>{i18n.tr('Henter kvoter…', 'Loading allowances…')}</p>}>
      <Show when={current()?.available} fallback={<p role="status">{i18n.tr('Kvoter er midlertidig utilgjengelige.', 'Allowances are temporarily unavailable.')}</p>}>
        <Show when={current()?.data.length} fallback={<p>{i18n.tr('Abonnementet har ingen bruksbaserte kvoter konfigurert.', 'No usage allowances are configured for this plan.')}</p>}>
          <For each={current()?.data}>{item => <article>
            <h3>{item.policy.name}</h3>
            <Show when={item.next_change}>{change => <p class="usage-analytics__note">{i18n.tr('Fra', 'From')} {date(change().effective_at)}: {number(change().included_units)} {item.policy.unit} {i18n.tr('inkludert per periode', 'included per period')}.</p>}</Show>
            <Show when={item.active} fallback={<p>{i18n.tr('Ikke aktiv', 'Not active')} · {date(item.policy.starts_at)}</p>}>
              <p class="usage-analytics__note">{date(item.current.from)} – {date(item.current.to)} · {item.policy.unit}</p>
              <dl class="usage-analytics__meters">
                <div><dt>{i18n.tr('Inkludert', 'Included')}</dt><dd>{number(item.current.included)}</dd></div>
                <div><dt>{i18n.tr('Overført', 'Carried over')}</dt><dd>{number(item.current.carried)}</dd></div>
                <div><dt>{i18n.tr('Brukt', 'Used')}</dt><dd>{number(item.current.used)}</dd></div>
                <div><dt>{i18n.tr('Gjenstår', 'Remaining')}</dt><dd>{number(item.current.remaining)}</dd></div>
              </dl>
              <Show when={item.current.granted}><p>{i18n.tr('Ekstra enheter', 'Additional units')}: {number(item.current.granted)}</p></Show>
              <Show when={item.current.overage}><p>{i18n.tr('Bruk utover kvoten', 'Usage beyond allowance')}: {number(item.current.overage)}. {i18n.tr('Dette er bruksenheter, ikke en fakturert kostnad.', 'These are usage units, not an invoiced charge.')}</p></Show>
              <Show when={item.next_expiry}><p class="usage-analytics__note">{i18n.tr('Neste utløp for ubrukte enheter', 'Next unused-unit expiry')}: {date(item.next_expiry!)}</p></Show>
            </Show>
            <Show when={item.history.length}><details><summary>{i18n.tr('Tidligere perioder', 'Previous periods')}</summary>
              <div class="usage-analytics__table" tabindex={0} role="region" aria-label={i18n.tr('Kvotehistorikk', 'Allowance history')}><table><thead><tr><th>{i18n.tr('Periode', 'Period')}</th><th>{i18n.tr('Brukt', 'Used')}</th><th>{i18n.tr('Utover kvoten', 'Beyond allowance')}</th></tr></thead><tbody><For each={item.history}>{period => <tr><th scope="row">{date(period.from)} – {date(period.to)}</th><td>{number(period.used)}</td><td>{number(period.overage)}</td></tr>}</For></tbody></table></div>
            </details></Show>
          </article>}</For>
        </Show>
      </Show>
    </Show>
  </section>
}
