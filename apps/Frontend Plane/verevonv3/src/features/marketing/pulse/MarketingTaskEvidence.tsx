import { For, Show } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import type { MarketingTask } from '../../../shared/api/marketing/marketing-work-client.ts'
import { technicalStatusLabel } from './technical-work-labels.ts'
import { auditStatusLabel } from './audit-work-labels.ts'

export function MarketingTaskEvidence(props: { task: MarketingTask }) {
  const i18n = useI18n()
  const stamp = (value: string) => new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value))
  return <>
    <Show when={props.task.evidence.kind==='search_group_review'?props.task.evidence:undefined}>{e=><><p>{e().review.rule.name} · {e().impressions} {i18n.tr('visninger','impressions')} · {i18n.tr('posisjon','position')} {e().position.toFixed(1)}</p><small>{e().period.startDate} – {e().period.endDate} · {stamp(e().observedAt)}</small><Show when={props.task.verification&&'review' in props.task.verification?props.task.verification:undefined}>{v=><p>{auditStatusLabel(v().status,i18n.tr)} · {stamp(v().observedAt)} · {i18n.tr('Kontrollerer den opprinnelige grupperegelen i en nyere rapport. Fravær forblir uavklart.','Checks the original group rule against a newer report. Absence remains inconclusive.')}</p>}</Show></>}</Show>
    <Show when={props.task.evidence.kind === 'gsc_opportunity' ? props.task.evidence : undefined}>{(evidence) => <>
      <p>{({ snippet_review: i18n.tr('Søkeutdrag', 'Search snippet'), title_meta: i18n.tr('Tittel og metabeskrivelse', 'Title and meta description'), title_test: i18n.tr('Titteltest', 'Title test'), content_expansion: i18n.tr('Innholdsutvidelse', 'Content expansion') })[evidence().opportunityType]} · {evidence().clicks} {i18n.tr('klikk', 'clicks')} / {evidence().impressions} {i18n.tr('visninger', 'impressions')} · {i18n.tr('posisjon', 'position')} {evidence().position.toFixed(1)}</p>
      <small>{evidence().period.startDate} – {evidence().period.endDate} · {i18n.tr('observert', 'observed')} {stamp(evidence().observedAt)}</small>
    </>}</Show>
    <Show when={props.task.evidence.kind === 'pagespeed_check' ? props.task.evidence : undefined}>{(evidence) => <>
      <p>PageSpeed · {evidence().strategy === 'mobile' ? i18n.tr('Mobil', 'Mobile') : i18n.tr('Datamaskin', 'Desktop')} · {i18n.tr('Opprinnelig observasjon', 'Original observation')}: {technicalStatusLabel(evidence().checkStatus, i18n.tr)}</p>
      <small>{stamp(evidence().observedAt)} · Lighthouse {evidence().lighthouseVersion}</small>
      <Show when={props.task.verification && !('review' in props.task.verification)&&!('auditId' in props.task.verification) ? props.task.verification : undefined}>{(verification) => <div class="marketing-work__verification" data-testid="marketing-work-verification"><p>{i18n.tr('Siste kontroll', 'Latest verification')}: {technicalStatusLabel(verification().status, i18n.tr)}</p><small>{stamp(verification().observedAt)} · Lighthouse {verification().lighthouseVersion}</small><small>{i18n.tr('Kontrollresultatet endrer ikke oppgavestatus. Fullfør manuelt etter vurdering.', 'The check result does not change task status. Complete manually after review.')}</small></div>}</Show>
    </>}</Show>
    <Show when={props.task.evidence.kind === 'site_audit_finding' ? props.task.evidence : undefined}>{(evidence) => <>
      <p>{i18n.tr('Nettstedsrevisjon', 'Website audit')} · {({ error: i18n.tr('Feil', 'Error'), warning: i18n.tr('Advarsel', 'Warning'), review: i18n.tr('Vurdering', 'Review') })[evidence().severity]}</p>
      <a href={evidence().pageUrl} target="_blank" rel="noopener noreferrer" title={evidence().pageUrl}>{evidence().pageUrl}</a>
      <small>{i18n.tr('Observert', 'Observed')} {stamp(evidence().observedAt)}</small>
      <details><summary>{i18n.tr('Vis revisjonsbevis', 'Show audit evidence')}</summary><small>{i18n.tr('Revisjon', 'Audit')}: {evidence().auditId}</small><small>{i18n.tr('Quarry-kjøring', 'Quarry run')}: <code>{evidence().runId}</code></small><small title={evidence().fingerprint}>{i18n.tr('Fingeravtrykk', 'Fingerprint')}: <code>{evidence().fingerprint}</code></small><For each={evidence().relatedUrls}>{(url) => <a href={url} target="_blank" rel="noopener noreferrer">{url}</a>}</For></details>
      <Show when={props.task.verification && 'auditId' in props.task.verification ? props.task.verification : undefined}>{(verification) => <div class="marketing-work__verification" data-testid="marketing-work-verification"><p>{i18n.tr('Siste kontroll', 'Latest verification')}: {auditStatusLabel(verification().status, i18n.tr)}</p><small>{stamp(verification().observedAt)} · {verification().auditId}</small><small title={verification().fingerprint}>{verification().runId} · {verification().fingerprint}</small><small>{i18n.tr('Kontrollresultatet endrer ikke oppgavestatus. Fullfør manuelt etter vurdering.', 'The check result does not change task status. Complete manually after review.')}</small></div>}</Show>
    </>}</Show>
  </>
}
