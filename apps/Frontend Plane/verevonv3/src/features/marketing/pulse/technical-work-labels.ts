import type { PageSpeedCheckId, PageSpeedCheckStatus } from '../../../shared/actions/marketing-pagespeed'

type Translate = (nb: string, en: string) => string
export const technicalCheckLabel = (id: PageSpeedCheckId, tr: Translate): string => ({
  'document-title': tr('Sidetittel', 'Document title'), 'meta-description': tr('Metabeskrivelse', 'Meta description'),
  'http-status-code': tr('HTTP-status', 'HTTP status'), 'is-crawlable': tr('Lighthouse-gjennomsøkbarhet', 'Lighthouse crawlability'),
  'robots-txt': 'robots.txt', canonical: tr('Kanonisk adresse', 'Canonical URL'), hreflang: 'hreflang',
  'link-text': tr('Beskrivende lenketekst', 'Descriptive link text'), 'crawlable-anchors': tr('Gjennomsøkbare lenker', 'Crawlable links'),
  'image-alt': tr('Alternativ bildetekst', 'Image alternative text'), 'structured-data': tr('Strukturerte data', 'Structured data'),
})[id]
export const technicalStatusLabel = (status: PageSpeedCheckStatus, tr: Translate): string => ({
  passed: tr('Bestått', 'Passed'), failed: tr('Ikke bestått', 'Failed'), manual: tr('Manuell vurdering', 'Manual review'),
  error: tr('Feil ved kontroll', 'Check error'), informative: tr('Informasjon', 'Informative'),
  not_applicable: tr('Ikke relevant', 'Not applicable'), not_reported: tr('Ikke rapportert', 'Not reported'),
})[status]
