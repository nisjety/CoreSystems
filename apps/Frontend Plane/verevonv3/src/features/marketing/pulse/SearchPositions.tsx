import { For, Show } from 'solid-js'
import { useI18n, localeDateTime } from '@/shared/i18n'
import type { SearchSnapshot } from '@/shared/api/marketing/search-client'

/** Distribution of the returned query sample, not a market-wide ranking count.
 * GSC positions are impression-weighted averages; decimals are never rounded
 * into a better bucket. Zero-position/no-impression rows are unknown. */
export function SearchPositions(props: { snapshot: SearchSnapshot | null; onExplore: () => void }) {
  const i18n = useI18n()
  const rows = () => props.snapshot?.status === 'connected' ? props.snapshot.queries : undefined
  const buckets = () => {
    const counts = [0, 0, 0, 0, 0]
    for (const row of rows() ?? []) {
      const index = row.impressions <= 0 || row.position <= 0 ? 4 : row.position <= 3 ? 0 : row.position <= 10 ? 1 : row.position <= 20 ? 2 : 3
      counts[index]! += 1
    }
    return counts
  }
  const names = () => ['≤ 3', '> 3–10', '> 10–20', '> 20', i18n.tr('Ukjent', 'Unknown')]
  const count = (index: number) => rows() ? new Intl.NumberFormat(localeDateTime(i18n.locale())).format(buckets()[index]!) : '—'
  return <section class="search-positions" aria-label={i18n.tr('Søkeordenes posisjoner', 'Query positions')}>
    <header><div><span class="search-dashboard__eyebrow">Google Search Console · SEO</span><h3>{i18n.tr('Plasseringer i Google', 'Positions in Google')}</h3><p>{i18n.tr('Fordeling etter gjennomsnittlig posisjon i den valgte perioden.', 'Distribution by average position in the selected period.')}</p></div><button type="button" onClick={() => props.onExplore()}>{i18n.tr('Se søkeordene', 'Explore the queries')}</button></header>
    <dl><For each={names()}>{(name, index) => <div><dt>{name}</dt><dd>{count(index())}</dd><span class="search-positions__track" aria-hidden="true"><span style={{width: `${rows()?.length ? buckets()[index()]! / rows()!.length * 100 : 0}%`}}/></span></div>}</For></dl>
    <Show when={rows()} fallback={<p role="status">{i18n.tr('Venter på en tilgjengelig søkeordrapport.', 'Waiting for an available query report.')}</p>}><p>{i18n.tr('Gjelder bare rapporterte søkeord, ikke alle søk eller en fast rangering.', 'Only returned queries are included; this is not all searches or a fixed rank.')} <Show when={props.snapshot?.queriesAtLimit}>{i18n.tr('Radgrensen er nådd; flere søkeord kan finnes.', 'The row limit was reached; additional queries may exist.')}</Show></p></Show>
  </section>
}
