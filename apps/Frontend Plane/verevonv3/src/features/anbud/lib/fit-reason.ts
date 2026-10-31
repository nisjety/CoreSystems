import type { TenderCheck, TenderMatch } from '@/shared/api/anbud-client'

type Translate = (no: string, en: string) => string

/** What a passing check says about the tender, in the words a bidder reads. */
const passed: Record<string, { no: string; en: string }> = {
  cpv: { no: 'bransjekoden', en: 'the industry code' },
  region: { no: 'regionen', en: 'the region' },
  value: { no: 'verdien', en: 'the value' },
  keywords: { no: 'nøkkelordene', en: 'the keywords' },
  company_seo: { no: 'selskapets SEO-søkeord', en: 'company SEO keywords' },
  user_focus: { no: 'arbeidsrollen din', en: 'your work role' },
  deadline: { no: 'fristen', en: 'the deadline' },
}

/** What a failing check says: why the company cannot bid. */
const blocked: Record<string, { no: string; en: string }> = {
  deadline: { no: 'fristen er passert', en: 'the deadline has passed' },
  company: { no: 'selskapet er under avvikling', en: 'the company is being wound up' },
  excluded_cpv: { no: 'bransjen er utelukket', en: 'the industry is excluded' },
  exclusion_grounds: { no: 'det finnes utelukkelsesgrunner', en: 'there are grounds for exclusion' },
}

/** What a check that is open says: something the company has to check before it bids. */
const open: Record<string, { no: string; en: string }> = {
  requirement: { no: 'kvalifiseringskrav', en: 'qualification requirements' },
  document_requirement: { no: 'krav i dokumentene', en: 'requirements in the documents' },
  criteria_in_documents: { no: 'kriterier i dokumentene', en: 'criteria in the documents' },
  exclusion_grounds: { no: 'utelukkelsesgrunner', en: 'grounds for exclusion' },
}

function joined(parts: string[], tr: Translate): string {
  if (parts.length <= 1) return parts[0] ?? ''
  return `${parts.slice(0, -1).join(', ')} ${tr('og', 'and')} ${parts[parts.length - 1]}`
}

/**
 * One plain sentence on why a tender fits, or why it cannot be bid on, from the checks the match already made. The
 * sentence names the checks in words; it never repeats a check's own detail, which is written for the audit sheet.
 */
export function fitReason(match: TenderMatch, tr: Translate): string {
  const checks: TenderCheck[] = [...match.gate, ...match.fit]
  const blockers = checks.filter(c => c.outcome === 'fail' && blocked[c.rule]).map(c => blocked[c.rule]!)
  if (blockers.length > 0) {
    return tr(`Kan ikke by: ${joined(blockers.map(b => b.no), tr)}.`, `Cannot bid: ${joined(blockers.map(b => b.en), tr)}.`)
  }
  const passes = [...new Set(checks.filter(c => c.outcome === 'pass' && passed[c.rule]).map(c => c.rule))].map(rule => passed[rule]!)
  const pending = [...new Set(checks.filter(c => (c.outcome === 'conditional' || c.outcome === 'unknown') && open[c.rule]).map(c => c.rule))].map(rule => open[rule]!)
  const need = pending.length > 0 ? ` ${tr(`Må sjekkes: ${joined(pending.map(p => p.no), tr)}.`, `To check: ${joined(pending.map(p => p.en), tr)}.`)}` : ''
  if (passes.length > 0) {
    return tr(`Passer: ${joined(passes.map(p => p.no), tr)}.`, `Fits: ${joined(passes.map(p => p.en), tr)}.`) + need
  }
  if (pending.length > 0) {
    return tr(`Må sjekkes: ${joined(pending.map(p => p.no), tr)}.`, `To check: ${joined(pending.map(p => p.en), tr)}.`)
  }
  return tr('Ikke nok data til å vurdere.', 'Not enough data to assess.')
}
