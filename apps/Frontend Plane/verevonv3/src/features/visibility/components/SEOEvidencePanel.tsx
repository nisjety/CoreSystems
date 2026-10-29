import { createSignal, For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import type { SEOEvidence } from '@/shared/api/visibility/seo-evidence'

/** Lazy structural evidence: a closed table row does not mount 200 headings. */
export function SEOEvidencePanel(props: { evidence: SEOEvidence }) {
  const { tr } = useI18n()
  const [open, setOpen] = createSignal(false)
  return <details class="visibility-seo-evidence" onToggle={e => setOpen(e.currentTarget.open)}>
    <summary>{tr('Struktur og indekseringssignaler', 'Structure and indexing signals')}</summary>
    <Show when={open()}>
      <dl class="visibility-page-detail">
        <dt>robots.txt</dt><dd>{props.evidence.robotsStatus ? `HTTP ${props.evidence.robotsStatus}` : tr('Ikke rapportert', 'Not reported')}</dd>
        <dt>noindex</dt><dd>{props.evidence.noindex ? tr('Deklarert – vurder hensikten', 'Declared — review the intent') : tr('Ikke observert', 'Not observed')}</dd>
        <dt>nofollow</dt><dd>{props.evidence.nofollow ? tr('Deklarert', 'Declared') : tr('Ikke observert', 'Not observed')}</dd>
        <dt>{tr('Ord i HTML-tekst', 'Words in HTML text')}</dt><dd>{props.evidence.wordCount.toLocaleString()}</dd>
        <dt>JSON-LD</dt><dd>{props.evidence.structuredData.documents} {tr('dokumenter', 'documents')} · {props.evidence.structuredData.invalid} {tr('parsefeil', 'parse errors')}<br />{props.evidence.structuredData.types.join(', ') || '—'}</dd>
      </dl>
      <Show when={props.evidence.canonicalOutsideSite}><p>{tr('Et kanonisk mål utenfor nettstedet er observert. Vurder om dette er tilsiktet.', 'A canonical target outside the site was observed. Review whether this is intentional.')}</p></Show>
      <h3>{tr('Overskrifter', 'Headings')}</h3><ol><For each={props.evidence.headings}>{h => <li><strong>H{h.level}</strong> {h.text || '—'}</li>}</For></ol>
      <Show when={props.evidence.headingsPartial}><p>{tr('Overskriftsutvalget er avkortet.', 'The heading sample is truncated.')}</p></Show>
      <h3>Hreflang</h3><ul><For each={props.evidence.alternates}>{a => <li><strong>{a.lang}</strong> <span class="visibility-url">{a.url}</span></li>}</For></ul>
      <Show when={props.evidence.alternatesPartial}><p>{tr('Språkvarianter utenfor nettstedet eller grensen inngår ikke i denne kontrollen.', 'Variants outside the site or evidence limit are not covered by this check.')}</p></Show>
      <h3>{tr('Observerte omdirigeringer', 'Observed redirects')}</h3><ol><For each={props.evidence.redirects}>{r => <li><span class="visibility-url">{r.url} → {r.target}</span> HTTP {r.status}</li>}</For></ol>
      <p class="visibility-note">{tr('Dette er observert HTML, ikke Googles indeksstatus. JSON-LD-parsing bekrefter ikke kvalifikasjon for utvidede søkeresultater. Robotsregler respekteres av Quarry; blokkerte sider får ingen oppdiktet innholdsvurdering.', 'This is observed HTML, not Google’s index status. JSON-LD parsing does not confirm rich-result eligibility. Quarry respects robots rules; blocked pages receive no invented content assessment.')}</p>
    </Show>
  </details>
}
