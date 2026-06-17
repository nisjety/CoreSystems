import type { BrandingSignals } from '@/features/onboarding/lib/api'
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
