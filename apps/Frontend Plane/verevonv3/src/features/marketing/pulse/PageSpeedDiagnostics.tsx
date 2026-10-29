import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { pageSpeedCheckIds, type PageSpeedCheck, type PageSpeedCheckId, type PageSpeedCheckStatus } from '../../../shared/actions/marketing-pagespeed'
import './pagespeed-diagnostics.css'

/** Fixed, product-owned explanations; no provider titles, URLs or HTML are rendered. */
export function PageSpeedDiagnostics(props: { checks?: readonly PageSpeedCheck[]; canSave?: boolean; savedIds?: readonly PageSpeedCheckId[]; savingId?: PageSpeedCheckId | null; onSave?: (id: PageSpeedCheckId) => void }) {
  const i18n = useI18n()
  const id = createUniqueId()
  const [reviewOnly, setReviewOnly] = createSignal(false)
  const copy = (): Record<PageSpeedCheckId, { label: string; detail: string }> => ({
    'document-title': { label: i18n.tr('Sidetittel', 'Document title'), detail: i18n.tr('Lighthouse sjekker at title finnes. Vurder selv om teksten er beskrivende og unik.', 'Lighthouse checks for a title. Review whether the text is descriptive and unique yourself.') },
    'meta-description': { label: i18n.tr('Metabeskrivelse', 'Meta description'), detail: i18n.tr('Lighthouse sjekker at metabeskrivelse finnes. Vurder relevansen; Google kan vise annen tekst.', 'Lighthouse checks for a meta description. Review its relevance; Google may display different text.') },
    'http-status-code': { label: i18n.tr('HTTP-status', 'HTTP status'), detail: i18n.tr('Kontroller HTTP-responsen for den målte siden, ikke alle lenker på nettstedet.', 'Check the measured page’s HTTP response, not every link on the website.') },
    'is-crawlable': { label: i18n.tr('Lighthouse-gjennomsøkbarhet', 'Lighthouse crawlability'), detail: i18n.tr('Se over noindex og tilgangsregler. Denne kontrollen bekrefter ikke faktisk indeksering.', 'Review noindex and access rules. This check does not confirm actual indexing.') },
    'robots-txt': { label: 'robots.txt', detail: i18n.tr('Lighthouse sjekker robots.txt-syntaks, ikke full tilgang for hver robot. Vurder robotreglene separat.', 'Lighthouse checks robots.txt syntax, not full access for every crawler. Review crawler rules separately.') },
    canonical: { label: i18n.tr('Kanonisk adresse', 'Canonical URL'), detail: i18n.tr('Lighthouse vurderer canonical-taggen, men bekrefter ikke hvilken adresse Google indekserer.', 'Lighthouse checks the canonical tag but does not confirm which URL Google indexes.') },
    hreflang: { label: 'hreflang', detail: i18n.tr('Se over språk- og regionreferanser når siden har språkversjoner.', 'Review language and region references when the page has language variants.') },
    'link-text': { label: i18n.tr('Beskrivende lenketekst', 'Descriptive link text'), detail: i18n.tr('Bruk lenketekst som forklarer hvor lenken fører.', 'Use link text that explains the destination.') },
    'crawlable-anchors': { label: i18n.tr('Gjennomsøkbare lenker', 'Crawlable links'), detail: i18n.tr('Bruk tilgjengelige href-lenker. Dette er ikke en kontroll av brutte lenker på hele nettstedet.', 'Use accessible href links. This is not a whole-site broken-link check.') },
    'image-alt': { label: i18n.tr('Alternativ bildetekst', 'Image alternative text'), detail: i18n.tr('Gi informative bilder passende alternativ tekst; dekorative bilder kan ha tom alt.', 'Give informative images appropriate alternative text; decorative images may use empty alt.') },
    'structured-data': { label: i18n.tr('Strukturerte data', 'Structured data'), detail: i18n.tr('Krever manuell validering. Lighthouse garanterer ikke gyldig schema eller utvidede søkeresultater.', 'Requires manual validation. Lighthouse does not guarantee valid schema or rich results.') },
  })
  const statusLabel = (status: PageSpeedCheckStatus): string => ({
    passed: i18n.tr('Bestått', 'Passed'), failed: i18n.tr('Ikke bestått', 'Failed'),
    not_applicable: i18n.tr('Ikke relevant', 'Not applicable'), manual: i18n.tr('Manuell vurdering', 'Manual review'),
    informative: i18n.tr('Informasjon', 'Informative'), error: i18n.tr('Feil ved kontroll', 'Check error'),
    not_reported: i18n.tr('Ikke rapportert', 'Not reported'),
  })[status]
  const rows = createMemo(() => pageSpeedCheckIds.map((checkId) => ({ id: checkId, status: props.checks?.find((check) => check.id === checkId)?.status ?? 'not_reported' as PageSpeedCheckStatus })))
  const needsReview = (status: PageSpeedCheckStatus) => status === 'failed' || status === 'manual' || status === 'error'
  const reviewCount = createMemo(() => rows().filter((row) => needsReview(row.status)).length)
  const passedCount = createMemo(() => rows().filter((row) => row.status === 'passed').length)
  const unavailableCount = createMemo(() => rows().filter((row) => row.status === 'not_reported').length)
  const visible = createMemo(() => rows().filter((row) => !reviewOnly() || needsReview(row.status)))
  return <section class="pagespeed-diagnostics" aria-labelledby={`${id}-title`}>
    <header><div><span class="pagespeed-panel__eyebrow">{i18n.tr('Tekniske kontroller', 'Technical checks')}</span><h3 id={`${id}-title`}>{i18n.tr('Forsidens SEO-grunnlag', 'Homepage SEO foundations')}</h3></div><span class="pagespeed-diagnostics__scope">{i18n.tr('11 Lighthouse-kontroller · Lab', '11 Lighthouse checks · Lab')}</span></header>
    <p>{i18n.tr('Gjelder kun labmålingen startet fra nettstedets forside. PageSpeed følger eventuelle omdirigeringer; målt opphav vises i rapporten når det er tilgjengelig. Dette er ikke en full nettstedsrevisjon.', 'Applies only to the lab audit started from the website’s homepage. PageSpeed follows possible redirects; the measured origin appears in the report when available. This is not a full site audit.')}</p>
    <div class="pagespeed-diagnostics__summary"><span>{i18n.tr('Bestått', 'Passed')}: {passedCount()}</span><span>{i18n.tr('Trenger vurdering', 'Needs review')}: {reviewCount()}</span><span>{i18n.tr('Ikke rapportert', 'Not reported')}: {unavailableCount()}</span></div>
    <div class="pagespeed-diagnostics__filters" role="group" aria-label={i18n.tr('Filtrer tekniske kontroller', 'Filter technical checks')}>
      <button type="button" aria-pressed={!reviewOnly()} onClick={() => setReviewOnly(false)}>{i18n.tr('Alle', 'All')} (11)</button>
      <button type="button" aria-pressed={reviewOnly()} onClick={() => setReviewOnly(true)}>{i18n.tr('Trenger vurdering', 'Needs review')} ({reviewCount()})</button>
    </div>
    <Show when={visible().length > 0} fallback={<p role="status">{i18n.tr('Det er ingen rapporterte kontroller som trenger vurdering. Manglende resultater er ikke beståtte kontroller.', 'There are no reported checks needing review. Missing results are not passed checks.')}</p>}>
      <ul class="pagespeed-diagnostics__list"><For each={visible()}>{(row) => <li><div><h4>{copy()[row.id].label}</h4><p>{copy()[row.id].detail}</p></div><span class="pagespeed-diagnostics__status" data-status={row.status}>{statusLabel(row.status)}</span><Show when={props.canSave && needsReview(row.status)}><button type="button" disabled={!!props.savingId || props.savedIds?.includes(row.id)} onClick={() => props.onSave?.(row.id)}>{props.savedIds?.includes(row.id) ? i18n.tr('Lagret', 'Saved') : props.savingId === row.id ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Lagre i oppgavelisten', 'Save to backlog')}</button></Show></li>}</For></ul>
    </Show>
    <p class="pagespeed-diagnostics__note">{i18n.tr('Ikke rapportert betyr at kontrollen mangler i denne målingen eller en eldre lagret rapport. Ingen ekstra måling kjøres ved visning eller filtrering.', 'Not reported means the check is absent from this measurement or an older saved report. Viewing or filtering does not run another audit.')}</p>
  </section>
}
