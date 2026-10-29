import { For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import type { AlgoliaTerms } from '@/shared/api/marketing/site-sources-client'
export function AlgoliaTermsTable(props:{title:string;terms?:AlgoliaTerms}) {
  const i18n=useI18n()
  const coverageNote=()=>i18n.tr('Opptil 20 rapporterte søkefraser per liste. Antallene er et utvalg, ikke alle søk eller en totalsum.','Up to 20 reported terms per list; counts are a shortlist, not all searches and not a total.')
  return <section class="site-source__terms"><h3>{props.title}</h3><p>{coverageNote()}</p><Show when={props.terms?.status==='connected'} fallback={<p>{i18n.tr('Denne listen er ikke tilgjengelig. Andre rapportdeler kan fortsatt brukes.','This list is unavailable. Other report components may still be used.')}</p>}><Show when={props.terms?.rows?.length} fallback={<p>{i18n.tr('Ingen søkefraser rapportert for denne perioden.','No search terms reported for this window.')}</p>}><table><thead><tr><th>{i18n.tr('Søkefrase','Search term')}</th><th>{i18n.tr('Antall søk','Search count')}</th><th>{i18n.tr('Gjennomsnittlige resultater','Average results')}</th></tr></thead><tbody><For each={props.terms?.rows}>{row=><tr><td>{row.query===''?i18n.tr('(Tomt søk)','(Blank search)'):row.query}</td><td>{row.count}</td><td>{row.averageResults??'—'}</td></tr>}</For></tbody></table></Show></Show></section>
}
