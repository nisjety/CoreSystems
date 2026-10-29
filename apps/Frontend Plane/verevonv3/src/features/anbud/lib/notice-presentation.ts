import type { TenderMatch, TenderMoney } from '@/shared/api/anbud-client'
type Translate = (no: string, en: string) => string

export const tenderColumns = [
  { id: 'buyer', no: 'Innkjøper', en: 'Buyer' },
  { id: 'noticeType', no: 'Kunngjøringstype', en: 'Notice type' },
  { id: 'arrangement', no: 'Ordningstype', en: 'Arrangement' },
  { id: 'status', no: 'Status', en: 'Status' },
  { id: 'estimatedValue', no: 'Kunngjort verdi', en: 'Estimated value' },
  { id: 'awardedValue', no: 'Faktisk kontraktsverdi', en: 'Awarded value' },
  { id: 'deadline', no: 'Tilbudsfrist', en: 'Deadline' },
  { id: 'published', no: 'Publisert', en: 'Published' },
  { id: 'winners', no: 'Vinnere', en: 'Winners' },
  { id: 'duration', no: 'Kontraktsvarighet', en: 'Contract duration' },
  { id: 'endDate', no: 'Sluttdato', en: 'End date' },
  { id: 'issued', no: 'Utstedt', en: 'Issued' },
  { id: 'match', no: 'Match', en: 'Match' },
] as const
export type TenderColumn = typeof tenderColumns[number]['id']
export type TenderColumnVisibility = Record<TenderColumn, boolean>
export const defaultTenderColumns: TenderColumnVisibility = { buyer: true, noticeType: true, arrangement: true, status: true, estimatedValue: true, awardedValue: true, deadline: true, published: true, winners: true, duration: true, endDate: true, issued: false, match: false }

export function noticeTypeLabel(notice: TenderMatch, tr: Translate): string {
  const code = notice.noticeType ?? ''
  if (code.startsWith('cn-')) return tr('Konkurranse', 'Competition')
  if (code.startsWith('pin-')) return tr('Veiledende kunngjøring', 'Prior information notice')
  if (code === 'pmc') return tr('Markedsdialog', 'Pre-market consultation')
  if (code === 'can-modif') return tr('Kontraktsendring', 'Contract modification')
  if (code.startsWith('can-')) return tr('Resultat', 'Result')
  if (code === 'veat') return tr('Intensjonskunngjøring', 'Voluntary ex-ante transparency notice')
  if (code === 'qu-sy') return tr('Kvalifikasjonsordning', 'Qualification system')
  return code || tr('Ikke oppgitt', 'Not stated')
}
export function arrangementLabel(notice: TenderMatch, tr: Translate): string {
  return (notice.arrangementTypes ?? []).map(code => code === 'framework' ? tr('Rammeavtale', 'Framework agreement') : code === 'dps' ? tr('Dynamisk innkjøpsordning', 'Dynamic purchasing system') : code).join(' · ') || '—'
}
export function noticeStatus(notice: TenderMatch, tr: Translate) {
  const value = notice.status ?? (notice.deadline ? (notice.deadline.daysLeft < 0 ? 'expired' : 'active') : 'unknown')
  const labels: Record<string, string> = { active: tr('Aktiv', 'Active'), expired: tr('Utgått', 'Expired'), planning: tr('Planlagt', 'Planned'), result: tr('Resultat', 'Result'), unknown: tr('Ikke oppgitt', 'Not stated') }
  return { value, label: labels[value] ?? value }
}
/** Where a deadline would be: the day a prior information notice plans its call for tenders for. */
export function plannedNoticeLabel(notice: TenderMatch, tr: Translate): string | undefined {
  return notice.plannedNotice ? tr(`Utlysning planlagt ${notice.plannedNotice}`, `Call planned ${notice.plannedNotice}`) : undefined
}
const dashes = /[\u2010-\u2015\u2212\uFE58\uFE63\uFF0D]/g
/** The form search text and the text it is looked for in are compared in, as the anbud core compares
 * them: lower case, every dash a hyphen (a title has "–", a user types "-"), white space collapsed. */
export function searchFold(value: string): string {
  return value.toLocaleLowerCase().replace(dashes, '-').replace(/\s+/g, ' ').trim()
}
/** Whether term, folded, is found in text, folded. */
export function searchMatches(text: string, term: string): boolean {
  return searchFold(text).includes(searchFold(term))
}
/** The source a notice was fetched from, by the evidence's source label. */
export function sourceName(source?: string): string {
  if (source === 'ted') return 'TED'
  if (source === 'boamp') return 'BOAMP'
  if (source === 'fts') return 'Find a Tender'
  if (source === 'bzp') return 'BZP'
  return 'Doffin'
}
export function formatTenderMoney(value?: TenderMoney): string {
  if (!value) return '—'
  const [integer = '', fraction] = value.amount.split('.')
  return `${integer.replace(/\B(?=(\d{3})+(?!\d))/g, ' ')}${fraction && fraction !== '00' ? `,${fraction}` : ''} ${value.currency}`
}
export function validDocumentUrl(value: string): string | null {
  try { const url = new URL(value); return ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password ? url.href : null } catch { return null }
}

export function winnersLabel(notice: TenderMatch, tr: Translate): string {
  const names = (notice.winners ?? []).filter(winner => winner.status === 'organisation' && winner.name).map(winner => winner.name!)
  if (names.length) return names.join(', ')
  return (notice.winners?.length ?? 0) > 0 ? tr('Ikke verifisert', 'Not verified') : '—'
}
export function durationLabel(notice: TenderMatch, tr: Translate): string {
  const units: Record<string, string> = { DAY: tr('dager', 'days'), WEEK: tr('uker', 'weeks'), MONTH: tr('måneder', 'months'), YEAR: tr('år', 'years') }
  return [...new Set((notice.contracts ?? []).flatMap(contract => contract.duration ? [`${contract.duration.value} ${units[contract.duration.unit] ?? contract.duration.unit}`] : contract.unlimited ? [tr('Ubegrenset', 'Unlimited')] : []))].join(' · ') || '—'
}
export function endDateLabel(notice: TenderMatch, tr?: Translate): string {
  return [...new Set((notice.contracts ?? []).flatMap(contract => contract.endDate ? [contract.endDate + (contract.endBasis !== 'stated' ? (tr?.(' (beregnet)', ' (calculated)') ?? ' (beregnet)') : '')] : []))].join(' · ') || '—'
}
