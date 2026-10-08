import { For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getUserCostUsage } from '@/shared/api/cost-client'
import { listOrganizationMembers } from '@/shared/api/organization-client'
import { insightsUserReportActionSchema } from '@/shared/actions/insights-user-report'
import { useI18n } from '@/shared/i18n'
import './user-usage.css'

export function UserUsageSection(props: { identity: string; orgId: string; from?: string; to?: string; refreshKey: number; viewer?: { id: string; name: string } | null }) {
  const i18n = useI18n()
  const [read] = createResource(() => ({ identity: props.identity, orgId: props.orgId, from: props.from, to: props.to, revision: props.refreshKey }), async (query) => {
    insightsUserReportActionSchema.parse({ type: 'insights.users.read', from: query.from, to: query.to })
    try {
      const [rows, members] = await Promise.all([
        getUserCostUsage(query), listOrganizationMembers(query.orgId).catch(() => []),
      ])
      return { identity: query.identity, from: query.from, to: query.to, rows, members, available: true }
    } catch {
      return { identity: query.identity, from: query.from, to: query.to, rows: [], members: [], available: false }
    }
  })
  const current = () => {
    const report = read()
    return report?.identity === props.identity && report.from === props.from && report.to === props.to ? report : undefined
  }
  const count = (value: number) => value.toLocaleString()
  return (
    <section class="verevon-insights-users" aria-label={i18n.tr('AI-bruk per bruker', 'AI usage by user')}>
      <div class="verevon-insights-section-heading">
        <div><span>{i18n.tr('Teamets bruk', 'Team usage')}</span><h2>{i18n.tr('AI-bruk per bruker', 'AI usage by user')}</h2></div>
        <p>{i18n.tr('Bruk i valgt periode.', 'Usage in the selected period.')}</p>
      </div>
      <Show when={current()} fallback={<p>{i18n.tr('Henter brukerdata…', 'Loading user usage…')}</p>}>
        {(report) => <Show when={report().available} fallback={<p role="status">{i18n.tr('Brukerdata er midlertidig utilgjengelig. Prøv å oppdatere.', 'User usage is temporarily unavailable. Try refreshing.')}</p>}>
          <Show when={report().rows.length > 0} fallback={<p>{i18n.tr('Ingen registrert AI-bruk i perioden.', 'No recorded AI usage in this period.')}</p>}>
            <div class="verevon-insights-users__scroll" role="region" aria-label={i18n.tr('Brukertabell, rull for å se alle kolonner', 'User table, scroll to see all columns')} tabindex={0}><table>
              <thead><tr>
                <th>{i18n.tr('Bruker', 'User')}</th><th>{i18n.tr('Forespørsler', 'Requests')}</th><th>Tokens</th>
                <th>{i18n.tr('Abonnement', 'Subscription')}</th><th>{i18n.tr('API-estimat', 'API estimate')}</th><th>{i18n.tr('Status', 'Status')}</th>
              </tr></thead>
              <tbody><For each={report().rows}>{(row) => {
                const member = () => report().members.find((person) => person.userId === row.user_id)
                return <tr>
                  <th scope="row">{row.user_id ? member()?.name || (props.viewer?.id === row.user_id ? props.viewer.name : undefined) || member()?.email || `${i18n.tr('Medlem', 'Member')} · ${row.user_id.slice(-6)}` : i18n.tr('Automatisering', 'Automation')}</th>
                  <td>{count(row.entry_count)}</td><td>{count(row.input_tokens + row.output_tokens)}</td><td>{count(row.subscription_entries)}</td>
                  <td>{row.metered_entries > 0 ? row.metered_cost_usd > 0 && row.metered_cost_usd < 0.01 ? '< $0.01' : `≈ $${row.metered_cost_usd.toFixed(2)}` : '—'}</td>
                  <td>{row.unknown_basis_entries || row.unmeasured_token_entries
                    ? i18n.tr('Noen detaljer mangler', 'Some details unavailable')
                    : i18n.tr('Oppdatert', 'Up to date')}</td>
                </tr>
              }}</For></tbody>
            </table></div>
          </Show>
        </Show>}
      </Show>
      <p class="verevon-insights-users__note">{i18n.tr('API-beløp er estimater. Abonnementsbruk faktureres ikke per forespørsel.', 'API amounts are estimates. Subscription usage is not billed per request.')}</p>
    </section>
  )
}
