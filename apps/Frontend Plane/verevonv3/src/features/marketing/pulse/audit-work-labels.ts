import type { AuditRule, AuditCheckStatus } from '../../../shared/api/marketing/audit-client'

type Translate = (nb: string, en: string) => string
export const auditRuleLabel = (rule: AuditRule, tr: Translate): string => ({
  missing_title_signal: tr('Sidetittel mangler', 'Missing page title'),
  missing_description_signal: tr('Metabeskrivelse mangler', 'Missing meta description'),
  http_error: tr('HTTP-feil', 'HTTP error'),
  duplicate_title_signal: tr('Duplisert sidetittel', 'Duplicate page title'),
  canonical_target_review: tr('Vurder kanonisk mål', 'Review canonical target'),
  broken_checked_link: tr('Brutt kontrollert lenke', 'Broken checked link'),
 h1_structure: tr('H1-struktur', 'H1 structure'),
 heading_sequence: tr('Overskriftsrekkefølge', 'Heading sequence'),
 noindex_directive: tr('Vurder noindex-direktiv', 'Review noindex directive'),
 redirect_chain_review: tr('Vurder omdirigeringskjede', 'Review redirect chain'),
 structured_data_parse: tr('Strukturerte data: JSON-LD-parsing', 'Structured data: JSON-LD parsing'),
 hreflang_syntax: tr('Hreflang-syntaks', 'Hreflang syntax'),
 hreflang_reciprocity: tr('Hreflang-returlenker', 'Hreflang return links'),
 canonical_outside_site: tr('Kanonisk mål utenfor nettstedet', 'Canonical target outside the site'),
})[rule]

export const auditStatusLabel = (status: AuditCheckStatus, tr: Translate): string => ({
  passed: tr('Bestått', 'Passed'), failed: tr('Ikke bestått', 'Failed'),
  review: tr('Manuell vurdering', 'Manual review'), inconclusive: tr('Ikke avgjort', 'Inconclusive'),
})[status]
