import type { BrandingSignals, BrregEnhet } from '@/features/onboarding/lib/api'
import type { OnboardingState, OrgSize, Step } from '@/features/onboarding/lib/model'
import { onboardingCrawlPhases } from '@/features/onboarding/lib/model'

export function activeCrawlPhase(website: OnboardingState['website']): number {
  if (website.status === 'completed') return onboardingCrawlPhases.length
  if (website.status === 'starting') return 0

  if (website.status === 'running') {
    if (website.snippets.length > 6) return 3
    if (website.elements > 0) return 2
    if (website.pages > 0) return 1
    return 0
  }

  return -1
}

export function brandHost(url: string | undefined): string | undefined {
  if (!url) return undefined

  try {
    return new URL(url).host
  } catch {
    return undefined
  }
}

export function graphNodePosition(index: number, total: number): { left: string; top: string } {
  if (index === 0) return { left: '50%', top: '50%' }

  const angle = ((index - 1) / Math.max(total - 1, 1)) * Math.PI * 2 - Math.PI / 2
  const radius = total > 5 ? 31 : 24

  return {
    left: `${50 + Math.cos(angle) * radius}%`,
    top: `${50 + Math.sin(angle) * radius}%`,
  }
}

export function hasBrandSignals(branding: BrandingSignals | undefined): boolean {
  return Boolean(
    branding?.siteName ||
      branding?.favicon ||
      branding?.themeColor ||
      branding?.logoCandidate ||
      branding?.palette?.length,
  )
}

/**
 * Derive a likely organization name to seed the Brreg lookup from the website
 * crawl — the detected brand/site name first, falling back to the registrable
 * label of the host (e.g. `www.aquatiq.com` → `Aquatiq`). Ported from velion v2
 * so the organization step auto-fills suggestions from the website findings.
 */
export function inferOrganizationQuery(website: OnboardingState['website']): string {
  const brandName = cleanBrandName(website.branding?.siteName)
  if (brandName) return brandName

  const rawUrl = website.url?.trim()
  if (!rawUrl) return ''
  try {
    const parsed = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(rawUrl) ? rawUrl : `https://${rawUrl}`)
    const labels = parsed.hostname.replace(/^www\./i, '').split('.').filter(Boolean)
    const candidate = labels.length > 1 ? labels[labels.length - 2] : labels[0]
    return cleanBrandName(candidate?.replace(/[-_]+/g, ' ') ?? '')
  } catch {
    return cleanBrandName(rawUrl.replace(/^https?:\/\//i, '').split(/[/?#]/)[0] ?? '')
  }
}

function cleanBrandName(value: string | undefined): string {
  if (!value) return ''
  const firstPart = value
    .replace(/\s+/g, ' ')
    .split(/\s[|·\-–—]\s/)
    .at(0)
    ?.trim()
  return firstPart && firstPart.length >= 2 ? titleCase(firstPart).slice(0, 80) : ''
}

function titleCase(value: string): string {
  if (/[A-ZÆØÅ]/.test(value.slice(1))) return value
  return value.replace(/\b[\p{L}\p{N}]/gu, (char) => char.toLocaleUpperCase('nb-NO'))
}

/**
 * Rank Brreg matches against the query: exact/prefix/substring name hits score
 * highest, with small boosts for a matching website host and headcount. Bankrupt
 * or dissolving entities are dropped. Ported from velion v2.
 */
export function rankBrregSuggestions(results: BrregEnhet[], query: string): BrregEnhet[] {
  const needle = normalizeSearch(query)
  return [...results]
    .filter((item) => !item.konkurs && !item.underAvvikling)
    .filter((item) => scoreBrreg(item, needle) > 0)
    .sort((a, b) => scoreBrreg(b, needle) - scoreBrreg(a, needle))
}

function scoreBrreg(item: BrregEnhet, needle: string): number {
  const name = normalizeSearch(item.navn)
  let score = 0
  if (name === needle) score += 100
  if (name.startsWith(needle)) score += 50
  if (name.includes(needle)) score += 25
  if (item.hjemmeside && normalizeSearch(item.hjemmeside).includes(needle)) score += 20
  if (item.antallAnsatte && item.antallAnsatte > 0) {
    score += Math.min(10, Math.log10(item.antallAnsatte + 1) * 3)
  }
  return score
}

function normalizeSearch(value: string): string {
  return value
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/æ/g, 'ae')
    .replace(/ø/g, 'o')
    .replace(/å/g, 'a')
    .replace(/[^a-z0-9]+/g, ' ')
    .trim()
}

export function sizeFromEmployees(count?: number): OrgSize {
  if (!count || count <= 1) return 'solo'
  if (count <= 10) return 'small'
  if (count <= 50) return 'medium'
  if (count <= 250) return 'large'
  return 'enterprise'
}

export function sizeLabel(size: OrgSize): string {
  switch (size) {
    case 'solo':
      return '1'
    case 'small':
      return '2-10'
    case 'medium':
      return '11-50'
    case 'large':
      return '51-250'
    case 'enterprise':
      return '250+'
  }
}

export function stepNumberFor(step: Step): number {
  switch (step) {
    case 'post-signin':
      return 1
    case 'website':
      return 2
    case 'organization':
      return 3
    case 'connect':
      return 4
    case 'social-proof':
      return 5
    case 'paywall':
    case 'assembly':
      return 6
  }
}

export function stripUrlProtocol(value: string): string {
  return value.replace(/^https?:\/\//, '')
}

export function truncateGraphLabel(value: string): string {
  const trimmed = value.trim()
  return trimmed.length > 16 ? `${trimmed.slice(0, 15)}…` : trimmed
}

export function websiteProgressPercent(website: OnboardingState['website']): number {
  if (website.status === 'completed') return 100

  if (website.status === 'failed' || website.status === 'cancelled') {
    return Math.min(100, Math.max(0, website.pages * 34 || website.snippets.length * 25))
  }

  if (website.status === 'starting') return 12
  if (website.status === 'running') return Math.min(94, Math.max(18, website.pages * 34, website.snippets.length * 18))
  return 0
}
